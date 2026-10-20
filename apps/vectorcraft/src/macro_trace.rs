//! Offline execution timeline for the routine preview engine.
//! Never sends commands to attached equipment.
use crate::macro_routines::{Context, PlannedStep, RoutineDatabase};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    pub index: usize,
    pub elapsed_ms: u64,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct TraceReport {
    pub entries: Vec<TraceEntry>,
    pub total_wait_ms: u64,
    pub output_operations: usize,
}

pub fn build_trace(db: &RoutineDatabase, routine: &str, ctx: Context) -> Result<TraceReport, String> {
    let planned = db.preview(routine, ctx)?;
    let mut report = TraceReport::default();
    for (index, step) in planned.iter().enumerate() {
        let description = match step {
            PlannedStep::On(device) => format!("Device {device}: ON"),
            PlannedStep::Off(device) => format!("Device {device}: OFF"),
            PlannedStep::Pwm(device, duty) => format!("Device {device}: PWM {duty}%"),
            PlannedStep::Wait(ms) => format!("WAIT {ms} ms"),
        };
        report.entries.push(TraceEntry { index, elapsed_ms: report.total_wait_ms, description });
        match step {
            PlannedStep::Wait(ms) => report.total_wait_ms = report.total_wait_ms.saturating_add(u64::from(*ms)),
            _ => report.output_operations += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_routines::{Routine, Step};

    #[test]
    fn trace_has_correct_order_and_timestamps() {
        let db = RoutineDatabase {
            routines: vec![Routine { name: "demo".into(), steps: vec![
                Step::SetOn { device: 2 }, Step::WaitMs { ms: 125 },
                Step::SetPwm { device: 3, percent: 40 }, Step::WaitMs { ms: 75 },
                Step::SetOff { device: 2 },
            ] }], bindings: vec![],
        };
        let trace = build_trace(&db, "demo", Context::default()).unwrap();
        assert_eq!(trace.entries.iter().map(|e| e.elapsed_ms).collect::<Vec<_>>(), vec![0, 0, 125, 125, 200]);
        assert_eq!(trace.total_wait_ms, 200);
        assert_eq!(trace.output_operations, 3);
    }

    #[test]
    fn missing_routine_is_rejected() {
        assert!(build_trace(&RoutineDatabase::default(), "missing", Context::default()).is_err());
    }
}
