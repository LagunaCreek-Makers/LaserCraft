//! Offline step debugger. No accessory transport is accessed.
use crate::macro_routines::{Context, PlannedStep, RoutineDatabase};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Debugger {
    pub selected_routine: String,
    pub input_json: String,
    pub cursor: usize,
    pub breakpoints: BTreeSet<usize>,
    pub paused: bool,
    pub message: String,
    pub watch_device: String,
    pub watch_variable: String,
    pub break_variable_change: String,
    pub break_device: String,
    pub break_pwm_at_least: u8,
    pub break_elapsed_ms: u64,
    pub break_on_wait: bool,
    pub plan_signature: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub elapsed_ms: u64,
    pub devices: BTreeMap<usize, Option<u8>>,
    pub executed: usize,
}
impl Debugger {
    pub fn watched_variable(&self, trace: &[crate::macro_routines::TracedStep]) -> Option<(String, Option<i64>)> {
        let key = self.watch_variable.trim();
        if key.is_empty() {
            return None;
        }
        let value = self.cursor.checked_sub(1).and_then(|i| trace.get(i)).and_then(|s| s.variables.get(key).copied());
        Some((key.to_owned(), value))
    }
    /// Find the next traced operation at which a watched variable differs from
    /// its previous snapshot. Empty/missing values are treated as distinct.
    pub fn next_variable_change(&self, trace: &[crate::macro_routines::TracedStep]) -> Option<usize> {
        let name = self.break_variable_change.trim();
        if name.is_empty() {
            return None;
        }
        let start = self.cursor.saturating_add(1);
        if start > trace.len() {
            return None;
        }
        (start..=trace.len()).find(|&pos| {
            let previous = if pos <= 1 { None } else { trace.get(pos - 2).and_then(|s| s.variables.get(name)) };
            let current = trace.get(pos - 1).and_then(|s| s.variables.get(name));
            previous != current
        })
    }
    pub fn context(&self) -> Result<Context, String> {
        let mut ctx = Context::default();
        if !self.input_json.trim().is_empty() {
            ctx.inputs = serde_json::from_str::<BTreeMap<String, i64>>(&self.input_json)
                .map_err(|e| format!("Input values must be a JSON object of integer values: {e}"))?;
        }
        Ok(ctx)
    }
    pub fn plan(&self, db: &RoutineDatabase) -> Result<Vec<PlannedStep>, String> {
        db.preview(&self.selected_routine, self.context()?)
    }
    pub fn snapshot(&self, plan: &[PlannedStep]) -> Snapshot {
        let mut result = Snapshot::default();
        for step in plan.iter().take(self.cursor.min(plan.len())) {
            match step {
                PlannedStep::On(d) => {
                    result.devices.insert(*d, Some(100));
                }
                PlannedStep::Off(d) => {
                    result.devices.insert(*d, Some(0));
                }
                PlannedStep::Pwm(d, p) => {
                    result.devices.insert(*d, Some(*p));
                }
                PlannedStep::Wait(ms) => result.elapsed_ms = result.elapsed_ms.saturating_add(u64::from(*ms)),
            }
            result.executed += 1;
        }
        result
    }
    pub fn refresh_plan(&mut self, plan: &[PlannedStep]) {
        // Clear action-index breakpoints when the expanded program changes.
        let signature = format!("{:?}", plan);
        if self.plan_signature != signature {
            self.plan_signature = signature;
            self.cursor = 0;
            self.breakpoints.clear();
            self.paused = true;
        }
    }
    pub fn watched_device(&self, plan: &[PlannedStep]) -> Result<Option<(usize, Option<u8>)>, String> {
        let value = self.watch_device.trim();
        if value.is_empty() {
            return Ok(None);
        }
        let id: usize = value.parse().map_err(|_| "Watch device must be an integer 0..255")?;
        if id >= 256 {
            return Err("Watch device must be 0..255".into());
        }
        Ok(Some((id, self.snapshot(plan).devices.get(&id).copied().flatten())))
    }
    fn conditional_break(&self, plan: &[PlannedStep], cursor: usize) -> bool {
        if cursor == 0 || cursor > plan.len() {
            return false;
        }
        let step = &plan[cursor - 1];
        if self.break_on_wait && matches!(step, PlannedStep::Wait(_)) {
            return true;
        }
        if self.break_elapsed_ms > 0 {
            let mut elapsed = 0_u64;
            for s in plan.iter().take(cursor) {
                if let PlannedStep::Wait(ms) = s {
                    elapsed = elapsed.saturating_add(u64::from(*ms));
                }
            }
            if elapsed >= self.break_elapsed_ms {
                return true;
            }
        }
        if let Ok(id) = self.break_device.trim().parse::<usize>() {
            if id < 256 {
                match step {
                    PlannedStep::On(d) if *d == id => return 100 >= self.break_pwm_at_least,
                    PlannedStep::Pwm(d, p) if *d == id => return *p >= self.break_pwm_at_least,
                    _ => {}
                }
            }
        }
        false
    }
    pub fn continue_with_conditions(&mut self, plan: &[PlannedStep]) {
        self.paused = false;
        for i in self.cursor.saturating_add(1)..=plan.len() {
            if self.breakpoints.contains(&i) || self.conditional_break(plan, i) {
                self.cursor = i;
                self.paused = i < plan.len();
                return;
            }
        }
        self.cursor = plan.len();
    }
    pub fn next(&mut self, count: usize) {
        self.cursor = self.cursor.saturating_add(1).min(count);
        self.paused = true;
    }
    pub fn continue_until_breakpoint(&mut self, count: usize) {
        self.paused = false;
        let start = self.cursor.saturating_add(1);
        self.cursor = (start..=count).find(|i| self.breakpoints.contains(i)).unwrap_or(count);
        self.paused = self.cursor < count;
    }
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.paused = true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stepping_and_snapshots() {
        let mut d = Debugger::default();
        let p = vec![PlannedStep::On(2), PlannedStep::Wait(200), PlannedStep::Pwm(2, 30), PlannedStep::Off(2)];
        d.next(p.len());
        d.next(p.len());
        assert_eq!(d.snapshot(&p).elapsed_ms, 200);
        d.breakpoints.insert(3);
        d.continue_until_breakpoint(p.len());
        assert_eq!(d.cursor, 3);
        assert_eq!(d.snapshot(&p).devices.get(&2), Some(&Some(30)));
        d.next(p.len());
        assert_eq!(d.snapshot(&p).devices.get(&2), Some(&Some(0)));
    }
    #[test]
    fn variable_change_detection() {
        use crate::macro_routines::TracedStep;
        let _ = std::mem::size_of::<TracedStep>();
        let d = Debugger::default();
        assert_eq!(d.next_variable_change(&[]), None);
    }
    #[test]
    fn change_search_at_end_is_safe() {
        let d = Debugger { cursor: 2, break_variable_change: "ready".into(), ..Default::default() };
        assert_eq!(d.next_variable_change(&[]), None);
    }
    #[test]
    fn rejects_invalid_inputs() {
        let d = Debugger { input_json: "{\"door\":true}".into(), ..Default::default() };
        assert!(d.context().is_err());
    }
}

#[cfg(test)]
mod phase45_tests {
    use super::*;
    #[test]
    fn conditional_device_and_elapsed_breakpoints() {
        let p = vec![PlannedStep::On(1), PlannedStep::Wait(300), PlannedStep::Pwm(3, 75), PlannedStep::Off(1)];
        let mut d = Debugger::default();
        d.break_device = "3".into();
        d.break_pwm_at_least = 70;
        d.continue_with_conditions(&p);
        assert_eq!(d.cursor, 3);
        d.reset();
        d.break_device.clear();
        d.break_elapsed_ms = 200;
        d.continue_with_conditions(&p);
        assert_eq!(d.cursor, 2);
    }
    #[test]
    fn watches_and_changed_plan_reset() {
        let p = vec![PlannedStep::On(4), PlannedStep::Off(4)];
        let mut d = Debugger::default();
        d.refresh_plan(&p);
        d.next(p.len());
        d.watch_device = "4".into();
        assert_eq!(d.watched_device(&p).unwrap(), Some((4, Some(100))));
        d.breakpoints.insert(1);
        d.refresh_plan(&[PlannedStep::Off(4)]);
        assert_eq!(d.cursor, 0);
        assert!(d.breakpoints.is_empty());
    }
}
