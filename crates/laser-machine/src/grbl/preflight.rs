//! Before a job starts: is the machine ready, and does the job suit it? Blockers stop the start;
//! warnings are shown and the job may go.

use super::{Settings, State, Status};

/// What the job was made for (from the machine profile).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Expect {
    /// The S value the job's full power is (the profile's maxS).
    pub max_s: Option<f64>,
    /// The bed the job was planned on (mm).
    pub bed: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preflight {
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

impl Preflight {
    pub fn ok(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// The smallest and the largest (X, Y) a job reaches.
pub type Extent = ([f64; 2], [f64; 2]);

/// The smallest and largest X and Y a job's absolute moves reach (work coordinates); `None` for a
/// job without X/Y moves, an error for one that moves relatively (G91), whose extent isn't known
/// from its words alone.
pub fn job_extent(lines: &[String]) -> Result<Option<Extent>, String> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut at: [Option<f64>; 2] = [None, None];
    for line in lines {
        let code = line.split(';').next().unwrap_or("").to_ascii_uppercase();
        let words: Vec<&str> = code.split_whitespace().collect();
        if words.contains(&"G91") {
            return Err("the job moves relatively (G91): its extent can't be checked".into());
        }
        for w in &words {
            let axis = match w.as_bytes().first() {
                Some(b'X') => 0,
                Some(b'Y') => 1,
                _ => continue,
            };
            if let Some(v) = w.get(1..).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
                at[axis] = Some(v);
            }
        }
        for axis in 0..2 {
            if let Some(v) = at[axis] {
                lo[axis] = lo[axis].min(v);
                hi[axis] = hi[axis].max(v);
            }
        }
    }
    Ok((lo[0].is_finite() && lo[1].is_finite()).then_some((lo, hi)))
}

/// Checks a job (`lines`) against the machine: its last `status` (`fresh`: recent enough to
/// trust), its `settings` and what the job was made for.
pub fn preflight(lines: &[String], status: Option<&Status>, fresh: bool, settings: &Settings, expect: &Expect) -> Preflight {
    let mut p = Preflight::default();
    match status {
        Some(s) if fresh => {
            match &s.state {
                State::Idle => {}
                State::Alarm => p.blockers.push("the machine is in alarm: check it, then unlock ($X) or home ($H)".into()),
                State::Door(_) => p.blockers.push("the safety door is open".into()),
                State::Hold(_) => p.blockers.push("the machine is in feed hold: resume or stop it first".into()),
                State::Sleep => p.blockers.push("the machine is asleep: reset it".into()),
                State::Check => p.blockers.push("the machine is in check mode ($C): nothing would be cut".into()),
                other => p.blockers.push(format!("the machine is busy ({})", other.label())),
            }
            if s.pins.contains('D') && !matches!(s.state, State::Door(_)) {
                p.blockers.push("the safety door input is active".into());
            }
        }
        _ => p.blockers.push("no recent status from the machine: check the connection".into()),
    }
    if settings.values.is_empty() {
        p.warnings.push("the machine's settings haven't been read ($$): laser mode, power range and travel weren't checked".into());
    }
    if settings.laser_mode() == Some(false) {
        p.blockers.push("laser mode is off ($32=0): the laser would stay on while moving between cuts".into());
    }
    if let (Some(have), Some(want)) = (settings.max_spindle(), expect.max_s)
        && (have - want).abs() > 0.5
    {
        p.warnings.push(format!(
            "the machine's full power is S{have} ($30) but the job was made for S{want}: power will be off by {:.0} %",
            (want / have.max(1e-9) - 1.0) * 100.0
        ));
    }
    match job_extent(lines) {
        Ok(Some((lo, hi))) => {
            let size = [hi[0] - lo[0], hi[1] - lo[1]];
            if let Some((tx, ty)) = settings.travel()
                && (size[0] > tx + 1e-6 || size[1] > ty + 1e-6)
            {
                p.blockers.push(format!(
                    "the job ({:.1} × {:.1} mm) is larger than the machine's travel ({tx:.1} × {ty:.1} mm, $130/$131)",
                    size[0], size[1]
                ));
            }
            if let Some((w, h)) = expect.bed
                && (lo[0] < -1e-6 || lo[1] < -1e-6 || hi[0] > w + 1e-6 || hi[1] > h + 1e-6)
            {
                p.blockers.push(format!("the job reaches outside the {w:.0} × {h:.0} mm bed it was made for"));
            }
        }
        Ok(None) => {}
        Err(e) => p.warnings.push(e),
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grbl::{Response, parse_response};

    fn status(line: &str) -> Status {
        match parse_response(line) {
            Response::Status(s) => *s,
            other => panic!("{other:?}"),
        }
    }

    fn lines(code: &str) -> Vec<String> {
        code.lines().map(str::to_string).collect()
    }

    fn settings() -> Settings {
        let mut s = Settings::default();
        for (id, v) in [(32, 1.0), (30, 1000.0), (130, 300.0), (131, 200.0)] {
            s.set(id, v);
        }
        s
    }

    #[test]
    fn extent_of_absolute_moves() {
        let l = lines("G21\nG90\nG0 X10 Y20\nG1 X50 F100\nG1 Y5\nM5");
        assert_eq!(job_extent(&l).unwrap(), Some(([10.0, 5.0], [50.0, 20.0])));
        assert_eq!(job_extent(&lines("G21\nM5")).unwrap(), None);
        assert!(job_extent(&lines("G91\nG1 X5")).is_err());
    }

    #[test]
    fn a_ready_machine_and_a_fitting_job_pass() {
        let job = lines("G0 X10 Y10\nG1 X100 Y100 S500");
        let p = preflight(&job, Some(&status("<Idle|MPos:0,0,0>")), true, &settings(), &Expect { max_s: Some(1000.0), bed: Some((300.0, 200.0)) });
        assert!(p.ok() && p.warnings.is_empty(), "{p:?}");
    }

    #[test]
    fn machine_states_that_block_a_start() {
        let job = lines("G0 X10 Y10");
        let s = settings();
        let e = Expect::default();
        for (line, why) in [
            ("<Alarm|MPos:0,0,0>", "alarm"),
            ("<Door:1|MPos:0,0,0>", "door"),
            ("<Hold:0|MPos:0,0,0>", "feed hold"),
            ("<Run|MPos:0,0,0>", "busy"),
            ("<Idle|MPos:0,0,0|Pn:D>", "door input"),
            ("<Check|MPos:0,0,0>", "check mode"),
        ] {
            let p = preflight(&job, Some(&status(line)), true, &s, &e);
            assert!(p.blockers.iter().any(|b| b.contains(why)), "{line}: {p:?}");
        }
        assert!(!preflight(&job, None, true, &s, &e).ok());
        assert!(!preflight(&job, Some(&status("<Idle|MPos:0,0,0>")), false, &s, &e).ok(), "a stale status isn't trusted");
    }

    #[test]
    fn settings_and_extent_checks() {
        let idle = status("<Idle|MPos:0,0,0>");
        let job = lines("G0 X0 Y0\nG1 X250 Y150");
        // Laser mode off blocks; a different power range warns.
        let mut s = settings();
        s.set(32, 0.0);
        s.set(30, 255.0);
        let p = preflight(&job, Some(&idle), true, &s, &Expect { max_s: Some(1000.0), bed: None });
        assert!(p.blockers.iter().any(|b| b.contains("laser mode")));
        assert!(p.warnings.iter().any(|w| w.contains("S255")));
        // Bigger than the travel, or off the bed it was made for: blocked.
        let big = lines("G0 X0 Y0\nG1 X350 Y150");
        assert!(preflight(&big, Some(&idle), true, &settings(), &Expect::default()).blockers.iter().any(|b| b.contains("travel")));
        assert!(
            preflight(&job, Some(&idle), true, &settings(), &Expect { max_s: None, bed: Some((200.0, 200.0)) })
                .blockers
                .iter()
                .any(|b| b.contains("bed"))
        );
        // Settings not read: a warning only.
        let p = preflight(&job, Some(&idle), true, &Settings::default(), &Expect::default());
        assert!(p.ok() && p.warnings.iter().any(|w| w.contains("$$")));
    }
}
