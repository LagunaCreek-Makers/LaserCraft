//! The GRBL 1.1 serial protocol: what a job may hold ([`job_lines`]), the real-time bytes
//! ([`realtime`]), what the controller says ([`Response`], [`Status`]), its settings
//! ([`Settings`]), the meaning of its error and alarm codes ([`codes`]) and job streaming
//! ([`Streamer`]).

pub mod codes;
mod response;
mod settings;
mod stream;

pub use response::{Response, State, Status, parse_response};
pub use settings::Settings;
pub use stream::{Progress, RX_BUFFER, StreamMode, Streamer};

/// The real-time bytes GRBL acts on as soon as they arrive, outside the line buffer.
pub mod realtime {
    /// Ask for a status report.
    pub const STATUS: u8 = b'?';
    /// Feed hold: slow to a stop, keeping the position.
    pub const HOLD: u8 = b'!';
    /// Resume after a feed hold.
    pub const RESUME: u8 = b'~';
    /// Soft reset: stop at once and clear the buffers (the position may be lost if moving).
    pub const SOFT_RESET: u8 = 0x18;
    /// Cancel the jog in progress.
    pub const JOG_CANCEL: u8 = 0x85;
}

/// The longest job line sent (GRBL's line buffer holds 80 characters; this leaves room).
pub const MAX_LINE: usize = 70;
/// Most lines one job may hold.
pub const MAX_LINES: usize = 2_000_000;

/// The lines of G-code `code` to stream: blank lines and comments dropped, each line checked to
/// be ASCII, short enough, and free of system (`$`) and real-time commands, which a job must
/// never carry.
pub fn job_lines(code: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for raw in code.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('(') {
            continue;
        }
        if !line.is_ascii() || line.len() > MAX_LINE || line.contains(['\r', '\0']) {
            return Err(format!(
                "invalid or oversized G-code line (at most {MAX_LINE} ASCII characters): `{}`",
                line.chars().take(40).collect::<String>()
            ));
        }
        if line.contains(['$', '!', '~', '?']) || line.bytes().any(|b| b == realtime::SOFT_RESET || b >= 0x80) {
            return Err(format!("system or real-time command not allowed in a job: `{line}`"));
        }
        out.push(line.to_owned());
        if out.len() > MAX_LINES {
            return Err(format!("the job has more than {MAX_LINES} lines"));
        }
    }
    if out.is_empty() {
        return Err("no G-code to run".into());
    }
    Ok(out)
}

/// A GRBL jog line (`$J=`, independent of the job's modal state): relative move of `distance_mm`
/// along `axis` at `feed_mm_min`, refused outside ±`max_mm` and 1–`max_feed` mm/min.
pub fn jog_line(axis: char, distance_mm: f64, feed_mm_min: f64, max_mm: f64, max_feed: f64) -> Result<String, String> {
    let axis = axis.to_ascii_uppercase();
    if !matches!(axis, 'X' | 'Y' | 'Z')
        || !distance_mm.is_finite()
        || distance_mm == 0.0
        || distance_mm.abs() > max_mm
        || !feed_mm_min.is_finite()
        || !(1.0..=max_feed).contains(&feed_mm_min)
    {
        return Err(format!("a jog is X, Y or Z, non-zero within ±{max_mm} mm, at 1–{max_feed} mm/min"));
    }
    Ok(format!("$J=G91 G21 {axis}{distance_mm:.3} F{feed_mm_min:.1}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_lines_drop_comments_and_refuse_system_commands() {
        assert_eq!(job_lines("G21\n\n; note\n(note)\n  G90  \nG0 X1").unwrap(), vec!["G21", "G90", "G0 X1"]);
        for bad in ["$X", "$H", "G1 X1 !", "?", "G1 X1\u{18}", "G1 X1\0", &"G1 X1 ".repeat(20), "G1 Xé"] {
            assert!(job_lines(bad).is_err(), "{bad:?}");
        }
        assert!(job_lines("; only a comment\n").is_err());
    }

    #[test]
    fn jog_lines_are_bounded() {
        assert_eq!(jog_line('x', -2.5, 600.0, 10.0, 1000.0).unwrap(), "$J=G91 G21 X-2.500 F600.0");
        for (axis, d, f) in [('A', 1.0, 100.0), ('X', 0.0, 100.0), ('X', 11.0, 100.0), ('X', 1.0, 0.5), ('X', f64::NAN, 100.0), ('Y', 1.0, 5000.0)] {
            assert!(jog_line(axis, d, f, 10.0, 1000.0).is_err(), "{axis} {d} {f}");
        }
    }
}
