//! Job streaming: which line to send next, and what each `ok` or `error` acknowledges.
//!
//! GRBL answers every line it takes with `ok` or `error:N`, in order. Line by line, the next line
//! goes only after the last one's answer (slow, but nothing is ever queued unseen). Character
//! counting keeps the controller's serial receive buffer full: lines are sent while the bytes
//! of those not yet answered, plus the next one, fit in it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::job_lines;

/// GRBL's serial receive buffer (bytes).
pub const RX_BUFFER: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StreamMode {
    /// One line at a time: the next after the last one's answer.
    #[default]
    LineByLine,
    /// As many lines as fit the receive buffer (`RX_BUFFER` bytes).
    CharacterCounting,
}

/// How far a job is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub sent: usize,
    pub acknowledged: usize,
    pub total: usize,
    pub failed: Option<String>,
    pub finished: bool,
}

impl Progress {
    pub fn active(&self) -> bool {
        !self.finished && self.failed.is_none()
    }
}

pub struct Streamer {
    mode: StreamMode,
    queue: VecDeque<String>,
    /// Lines sent and not yet answered: their byte counts (with the line end).
    inflight: VecDeque<usize>,
    /// When the oldest unanswered line was sent (or last answered).
    waiting_since: Option<Instant>,
    pub progress: Progress,
}

impl Streamer {
    pub fn new(code: &str, mode: StreamMode) -> Result<Self, String> {
        let queue: VecDeque<String> = job_lines(code)?.into();
        let total = queue.len();
        Ok(Self { mode, queue, inflight: VecDeque::new(), waiting_since: None, progress: Progress { total, ..Progress::default() } })
    }

    /// The bytes not yet answered.
    pub fn unanswered_bytes(&self) -> usize {
        self.inflight.iter().sum()
    }

    /// The next line to send now (with its line end), if one may go.
    pub fn next_line(&mut self, now: Instant) -> Option<String> {
        if !self.progress.active() {
            return None;
        }
        let next = self.queue.front()?.len() + 1;
        let fits = match self.mode {
            StreamMode::LineByLine => self.inflight.is_empty(),
            StreamMode::CharacterCounting => self.unanswered_bytes() + next <= RX_BUFFER,
        };
        if !fits {
            return None;
        }
        let line = self.queue.pop_front()?;
        if self.inflight.is_empty() {
            self.waiting_since = Some(now);
        }
        self.inflight.push_back(next);
        self.progress.sent += 1;
        Some(format!("{line}\n"))
    }

    /// An `ok`: the oldest line sent was taken. Answers with nothing sent are ignored.
    pub fn ok(&mut self, now: Instant) {
        if self.inflight.pop_front().is_none() {
            return;
        }
        self.progress.acknowledged += 1;
        self.waiting_since = (!self.inflight.is_empty()).then_some(now);
        if self.queue.is_empty() && self.inflight.is_empty() && self.progress.failed.is_none() {
            self.progress.finished = true;
        }
    }

    /// An `error` or `ALARM`: the job stops (the lines already in the controller may still run;
    /// the caller holds or resets the machine).
    pub fn fail(&mut self, reason: &str) {
        if self.progress.active() {
            self.progress.failed = Some(reason.to_string());
        }
        self.queue.clear();
        self.inflight.clear();
        self.waiting_since = None;
    }

    /// Stops the job when the oldest unanswered line has waited `limit`: the controller's state
    /// is then unknown.
    pub fn timed_out(&mut self, now: Instant, limit: Duration) -> bool {
        if self.waiting_since.is_some_and(|t| now.saturating_duration_since(t) >= limit) {
            self.fail("no answer from the controller in time: its state is unknown");
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_by_line_waits_for_each_answer() {
        let t = Instant::now();
        let mut s = Streamer::new("G21\nG90\nG0 X1", StreamMode::LineByLine).unwrap();
        assert_eq!(s.next_line(t).unwrap(), "G21\n");
        assert!(s.next_line(t).is_none());
        s.ok(t);
        assert_eq!(s.next_line(t).unwrap(), "G90\n");
        s.ok(t);
        s.next_line(t);
        s.ok(t);
        assert!(s.progress.finished);
        assert_eq!(s.progress.acknowledged, 3);
    }

    #[test]
    fn character_counting_fills_the_receive_buffer_and_no_more() {
        let t = Instant::now();
        let code: String = (0..100).map(|i| format!("G1 X{i}.000 Y{i}.000 F1000\n")).collect();
        let mut s = Streamer::new(&code, StreamMode::CharacterCounting).unwrap();
        let mut sent = 0;
        while let Some(l) = s.next_line(t) {
            sent += l.len();
        }
        assert!(sent <= RX_BUFFER && sent + 27 > RX_BUFFER, "{sent}");
        assert!(s.progress.sent > 1);
        let before = s.progress.sent;
        s.ok(t);
        assert!(s.next_line(t).is_some(), "an answer frees room");
        assert!(s.unanswered_bytes() <= RX_BUFFER);
        assert_eq!(s.progress.sent, before + 1);
    }

    #[test]
    fn errors_timeouts_and_stray_answers() {
        let t = Instant::now();
        let mut s = Streamer::new("G1 X1\nG1 X2", StreamMode::CharacterCounting).unwrap();
        s.ok(t);
        assert_eq!(s.progress.acknowledged, 0, "an answer to nothing");
        s.next_line(t);
        s.fail("error:2");
        assert!(s.next_line(t).is_none());
        assert_eq!(s.progress.failed.as_deref(), Some("error:2"));
        let mut s = Streamer::new("G21", StreamMode::LineByLine).unwrap();
        assert!(!s.timed_out(t + Duration::from_secs(60), Duration::from_secs(1)), "nothing sent, nothing awaited");
        s.next_line(t);
        assert!(!s.timed_out(t + Duration::from_millis(500), Duration::from_secs(1)));
        assert!(s.timed_out(t + Duration::from_secs(2), Duration::from_secs(1)));
        assert!(!s.progress.active());
    }
}
