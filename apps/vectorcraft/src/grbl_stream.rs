//! GRBL 1.1 conservative one-line-at-a-time streaming state machine.
//! This avoids planner-buffer assumptions. Only send a new line after `ok`.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub sent: usize,
    pub acknowledged: usize,
    pub total: usize,
    pub failed: Option<String>,
    pub finished: bool,
}
pub struct Stream {
    queue: VecDeque<String>,
    inflight: bool,
    sent_at: Option<Instant>,
    pub progress: Progress,
}
impl Stream {
    pub fn new(code: &str) -> Result<Self, String> {
        let mut queue = VecDeque::new();
        for raw in code.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('(') {
                continue;
            }
            if !line.is_ascii() || line.len() > 70 || line.contains('\r') || line.contains('\0') {
                return Err("Invalid or oversized G-code line".into());
            }
            // Block firmware/system commands and real-time characters from generated jobs.
            if line.contains('$') || line.contains('!') || line.contains('~') || line.contains('?') || line.contains('\u{18}') {
                return Err("System/real-time command prohibited in job".into());
            }
            queue.push_back(line.to_owned());
        }
        if queue.is_empty() {
            return Err("No executable G-code".into());
        }
        if queue.len() > 2_000_000 {
            return Err("Job exceeds line limit".into());
        }
        let total = queue.len();
        Ok(Self { queue, inflight: false, sent_at: None, progress: Progress { total, ..Default::default() } })
    }
    pub fn next_line(&mut self) -> Option<String> {
        if self.inflight || self.progress.failed.is_some() || self.progress.finished {
            return None;
        }
        let line = self.queue.pop_front()?;
        self.inflight = true;
        self.sent_at = Some(Instant::now());
        self.progress.sent += 1;
        Some(format!("{line}\n"))
    }
    pub fn response(&mut self, line: &str) {
        if !self.inflight {
            return;
        }
        if line == "ok" {
            self.inflight = false;
            self.sent_at = None;
            self.progress.acknowledged += 1;
            if self.queue.is_empty() {
                self.progress.finished = true;
            }
        } else if line.starts_with("error:") || line.starts_with("ALARM:") {
            self.progress.failed = Some(line.into());
            self.inflight = false;
            self.sent_at = None;
            self.queue.clear();
        }
    }
    pub fn timed_out(&mut self, limit: Duration) -> bool {
        if self.inflight && self.sent_at.is_some_and(|t| t.elapsed() >= limit) {
            self.abort("GRBL acknowledgement timeout; controller state uncertain");
            true
        } else {
            false
        }
    }
    pub fn has_inflight(&self) -> bool {
        self.inflight
    }
    pub fn abort(&mut self, reason: &str) {
        self.queue.clear();
        self.progress.failed = Some(reason.into());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ack_flow() {
        let mut s = Stream::new("G21\nG90\nG0 X1").unwrap();
        assert_eq!(s.next_line().unwrap(), "G21\n");
        assert!(s.next_line().is_none());
        s.response("ok");
        assert_eq!(s.next_line().unwrap(), "G90\n");
        s.response("ok");
        s.next_line();
        s.response("ok");
        assert!(s.progress.finished);
        assert_eq!(s.progress.acknowledged, 3);
    }
    #[test]
    fn errors_stop() {
        let mut s = Stream::new("G1 X1\nG1 X2").unwrap();
        s.next_line();
        s.response("error:2");
        assert!(s.next_line().is_none());
    }
    #[test]
    fn unsolicited_ack_does_not_advance() {
        let mut s = Stream::new("G21").unwrap();
        s.response("ok");
        assert_eq!(s.progress.acknowledged, 0);
    }
    #[test]
    fn timeout_aborts() {
        let mut s = Stream::new("G21").unwrap();
        s.next_line();
        assert!(s.timed_out(Duration::ZERO));
        assert!(s.progress.failed.is_some());
    }
    #[test]
    fn rejects_system() {
        assert!(Stream::new("$X").is_err());
    }
}
