//! A GRBL machine driven through a [`Transport`], one [`Controller::poll`] at a time: no threads
//! and no clock of its own (the caller passes `now`), so tests run it against the simulator
//! step by step.

use std::time::{Duration, Instant};

use crate::Transport;
use crate::grbl::{self, Progress, Response, Settings, Status, StreamMode, Streamer, codes, realtime};

/// The longest jog, and its fastest feed.
pub const MAX_JOG_MM: f64 = 10.0;
pub const MAX_JOG_FEED: f64 = 1000.0;
/// A job line unanswered this long stops the job (the controller's state is then unknown).
pub const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// Status reports are asked for this often.
pub const STATUS_EVERY: Duration = Duration::from_millis(500);
/// The longest line kept from the controller (a stream of bytes without line ends is noise).
const MAX_INCOMING: usize = 4096;

/// What the operator (or a test) asks of the machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `$I`: the firmware's build information.
    Identify,
    /// `$$`: the settings (kept in [`Controller::settings`]).
    Settings,
    /// Feed hold.
    Hold,
    /// Resume after a feed hold.
    Resume,
    Jog {
        axis: char,
        distance_mm: f64,
        feed_mm_min: f64,
    },
    StartJob(String),
    /// Stop the job: no more lines, and a feed hold.
    CancelJob,
}

/// What happened, for the caller to show.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A line from the controller, errors and alarms with their meaning.
    Line(String),
    Status(Box<Status>),
    JobProgress(Progress),
    /// The link failed: the caller closes it. The machine's state is unknown.
    Lost(String),
}

pub struct Controller<T: Transport> {
    link: T,
    mode: StreamMode,
    job: Option<Streamer>,
    incoming: Vec<u8>,
    last_status_query: Option<Instant>,
    /// What `$$` reported.
    pub settings: Settings,
    /// The last status report.
    pub status: Option<Status>,
    /// The last work offset reported (reports carry it now and then).
    pub offset: Option<[f64; 3]>,
    lost: bool,
}

impl<T: Transport> Controller<T> {
    pub fn new(link: T, mode: StreamMode) -> Self {
        Self {
            link,
            mode,
            job: None,
            incoming: vec![],
            last_status_query: None,
            settings: Settings::default(),
            status: None,
            offset: None,
            lost: false,
        }
    }

    pub fn link(&self) -> &T {
        &self.link
    }

    pub fn job_active(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.progress.active())
    }

    pub fn progress(&self) -> Option<&Progress> {
        self.job.as_ref().map(|j| &j.progress)
    }

    /// Writes `bytes`; a failure loses the link (and stops the job).
    fn send(&mut self, bytes: &[u8], events: &mut Vec<Event>) -> bool {
        if self.lost {
            return false;
        }
        match self.link.write_all(bytes) {
            Ok(()) => true,
            Err(e) => self.lose(format!("write failed: {e}"), events),
        }
    }

    fn lose(&mut self, why: String, events: &mut Vec<Event>) -> bool {
        if let Some(j) = self.job.as_mut().filter(|j| j.progress.active()) {
            j.fail(&format!("{why}; the machine's state is unknown"));
            events.push(Event::JobProgress(j.progress.clone()));
        }
        self.lost = true;
        events.push(Event::Lost(why));
        false
    }

    /// Carries out `command`: an error (nothing sent) when it isn't allowed now; events for what
    /// it changed.
    pub fn command(&mut self, command: Command, now: Instant) -> Result<Vec<Event>, String> {
        let mut events = vec![];
        if self.lost {
            return Err("the machine is not connected".into());
        }
        let busy = self.job_active();
        match command {
            Command::Identify | Command::Settings if busy => return Err("not while a job runs".into()),
            Command::Identify => {
                self.send(b"$I\n", &mut events);
            }
            Command::Settings => {
                self.send(b"$$\n", &mut events);
            }
            Command::Hold => {
                self.send(&[realtime::HOLD], &mut events);
            }
            Command::Resume => {
                self.send(&[realtime::RESUME], &mut events);
            }
            Command::Jog { .. } if busy => return Err("jogging is off while a job runs".into()),
            Command::Jog { axis, distance_mm, feed_mm_min } => {
                let line = grbl::jog_line(axis, distance_mm, feed_mm_min, MAX_JOG_MM, MAX_JOG_FEED)?;
                self.send(format!("{line}\n").as_bytes(), &mut events);
            }
            Command::StartJob(_) if busy => return Err("a job is already running".into()),
            Command::StartJob(code) => {
                let job = Streamer::new(&code, self.mode)?;
                events.push(Event::JobProgress(job.progress.clone()));
                self.job = Some(job);
                self.pump(now, &mut events);
            }
            Command::CancelJob => {
                if let Some(j) = self.job.as_mut().filter(|j| j.progress.active()) {
                    j.fail("cancelled: feed hold sent");
                    events.push(Event::JobProgress(j.progress.clone()));
                }
                self.send(&[realtime::HOLD], &mut events);
            }
        }
        Ok(events)
    }

    /// Sends the job's next lines while they may go.
    fn pump(&mut self, now: Instant, events: &mut Vec<Event>) {
        let mut sent = false;
        while let Some(line) = self.job.as_mut().and_then(|j| j.next_line(now)) {
            if !self.send(line.as_bytes(), events) {
                return;
            }
            sent = true;
        }
        if sent && let Some(j) = &self.job {
            events.push(Event::JobProgress(j.progress.clone()));
        }
    }

    /// One step: ask for status when due, read what arrived and act on it, send the job's next
    /// lines, and stop a job whose answer is overdue.
    pub fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut events = vec![];
        if self.lost {
            return events;
        }
        if self.last_status_query.is_none_or(|t| now.saturating_duration_since(t) >= STATUS_EVERY) {
            self.last_status_query = Some(now);
            if !self.send(&[realtime::STATUS], &mut events) {
                return events;
            }
        }
        let mut buf = [0u8; 512];
        loop {
            match self.link.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for &b in buf.iter().take(n) {
                        if b == b'\n' {
                            let line = String::from_utf8_lossy(&self.incoming).trim().to_string();
                            self.incoming.clear();
                            self.take(&line, now, &mut events);
                        } else if self.incoming.len() < MAX_INCOMING {
                            self.incoming.push(b);
                        } else {
                            self.incoming.clear();
                        }
                    }
                    if n < buf.len() {
                        break;
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => break,
                Err(e) => {
                    self.lose(format!("read failed: {e}"), &mut events);
                    return events;
                }
            }
        }
        self.pump(now, &mut events);
        if let Some(j) = self.job.as_mut()
            && j.timed_out(now, ACK_TIMEOUT)
        {
            events.push(Event::JobProgress(j.progress.clone()));
            self.lose("no answer from the controller: reconnect and check the machine before going on".into(), &mut events);
        }
        events
    }

    /// One line from the controller.
    fn take(&mut self, line: &str, now: Instant, events: &mut Vec<Event>) {
        if line.is_empty() {
            return;
        }
        match grbl::parse_response(line) {
            Response::Status(s) => {
                if s.offset.is_some() {
                    self.offset = s.offset;
                }
                self.status = Some((*s).clone());
                events.push(Event::Status(s));
                return;
            }
            Response::Ok => {
                if let Some(j) = self.job.as_mut().filter(|j| j.progress.active()) {
                    j.ok(now);
                    events.push(Event::JobProgress(j.progress.clone()));
                }
                return;
            }
            Response::Error(_) | Response::Alarm(_) => {
                if let Some(j) = self.job.as_mut().filter(|j| j.progress.active()) {
                    j.fail(&codes::explain(line));
                    events.push(Event::JobProgress(j.progress.clone()));
                }
            }
            Response::Setting(id, v) => self.settings.set(id, v),
            Response::Welcome(_) => {
                // The controller restarted: whatever it had of the job is gone.
                if let Some(j) = self.job.as_mut().filter(|j| j.progress.active()) {
                    j.fail("the controller restarted during the job");
                    events.push(Event::JobProgress(j.progress.clone()));
                }
            }
            Response::Feedback(_) | Response::Other(_) => {}
        }
        events.push(Event::Line(codes::explain(line)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimulatedGrbl;

    fn job(n: usize) -> String {
        let mut s = String::from("G21\nG90\nM4 S0\n");
        for i in 0..n {
            s.push_str(&format!("G1 X{}.000 Y{}.000 F1200 S500\n", i % 100, (i * 7) % 100));
        }
        s.push_str("M5\n");
        s
    }

    /// Polls until the job ends (or `max` polls), the clock moving 10 ms a poll.
    fn run(c: &mut Controller<SimulatedGrbl>, t: &mut Instant, max: usize) -> Vec<Event> {
        let mut all = vec![];
        for _ in 0..max {
            *t += Duration::from_millis(10);
            all.extend(c.poll(*t));
            if !c.job_active() {
                break;
            }
        }
        all
    }

    #[test]
    fn streams_a_job_both_ways_without_overflowing_the_buffer() {
        for mode in [StreamMode::LineByLine, StreamMode::CharacterCounting] {
            let mut t = Instant::now();
            let mut c = Controller::new(SimulatedGrbl::default(), mode);
            c.command(Command::StartJob(job(200)), t).unwrap();
            run(&mut c, &mut t, 10_000);
            let p = c.progress().unwrap();
            assert!(p.finished && p.failed.is_none(), "{mode:?}: {p:?}");
            assert_eq!(p.acknowledged, 204);
            assert_eq!(c.link().lines.len(), 204);
            assert!(!c.link().overflowed, "{mode:?}");
        }
        // Character counting needs fewer round trips when the controller is quick.
        let polls = |mode| {
            let mut t = Instant::now();
            let mut c = Controller::new(SimulatedGrbl { lines_per_read: 8, ..SimulatedGrbl::default() }, mode);
            c.command(Command::StartJob(job(200)), t).unwrap();
            let mut n = 0;
            while c.job_active() && n < 10_000 {
                t += Duration::from_millis(10);
                c.poll(t);
                n += 1;
            }
            n
        };
        assert!(polls(StreamMode::CharacterCounting) * 2 < polls(StreamMode::LineByLine));
    }

    #[test]
    fn errors_and_alarms_stop_the_job_with_their_meaning() {
        let mut t = Instant::now();
        let mut c = Controller::new(SimulatedGrbl { reject_at: Some(5), ..SimulatedGrbl::default() }, StreamMode::LineByLine);
        c.command(Command::StartJob(job(20)), t).unwrap();
        let events = run(&mut c, &mut t, 1000);
        let p = c.progress().unwrap();
        assert_eq!(p.failed.as_deref(), Some("error:2 (a number in the line is malformed)"));
        assert_eq!(p.acknowledged, 4);
        assert!(events.iter().any(|e| matches!(e, Event::Line(l) if l.starts_with("error:2 ("))));
        let mut c = Controller::new(SimulatedGrbl { alarm_at: Some((3, 2)), ..SimulatedGrbl::default() }, StreamMode::CharacterCounting);
        c.command(Command::StartJob(job(20)), t).unwrap();
        run(&mut c, &mut t, 1000);
        assert!(c.progress().unwrap().failed.as_deref().unwrap().contains("soft limit"));
        // Locked by the alarm: the controller refuses the next job's lines.
        c.command(Command::StartJob(job(2)), t).unwrap();
        run(&mut c, &mut t, 1000);
        assert!(c.progress().unwrap().failed.as_deref().unwrap().starts_with("error:9"));
    }

    #[test]
    fn a_lost_answer_times_the_job_out_and_loses_the_link() {
        let mut t = Instant::now();
        let mut c = Controller::new(SimulatedGrbl { silence_at: Some(2), ..SimulatedGrbl::default() }, StreamMode::LineByLine);
        c.command(Command::StartJob(job(5)), t).unwrap();
        let events = run(&mut c, &mut t, 100);
        assert!(c.job_active(), "still waiting within the timeout");
        assert!(!events.iter().any(|e| matches!(e, Event::Lost(_))));
        t += ACK_TIMEOUT;
        let events = c.poll(t);
        assert!(events.iter().any(|e| matches!(e, Event::Lost(_))));
        assert!(c.progress().unwrap().failed.as_deref().unwrap().contains("unknown"));
        assert!(c.command(Command::Hold, t).is_err(), "nothing more is sent");
    }

    #[test]
    fn status_settings_and_operator_commands() {
        let mut t = Instant::now();
        let mut c = Controller::new(SimulatedGrbl::default(), StreamMode::LineByLine);
        let events = c.poll(t);
        assert!(events.iter().any(|e| matches!(e, Event::Status(s) if s.state == grbl::State::Idle)));
        c.command(Command::Settings, t).unwrap();
        t += Duration::from_millis(10);
        c.poll(t);
        assert_eq!(c.settings.laser_mode(), Some(true));
        assert_eq!(c.settings.travel(), Some((400.0, 300.0)));
        c.command(Command::Jog { axis: 'x', distance_mm: 5.0, feed_mm_min: 500.0 }, t).unwrap();
        assert!(c.command(Command::Jog { axis: 'x', distance_mm: 50.0, feed_mm_min: 500.0 }, t).is_err());
        t += Duration::from_millis(10);
        c.poll(t);
        assert!(c.link().lines.iter().any(|l| l == "$J=G91 G21 X5.000 F500.0"));
        // During a job: no jogging or queries; cancel holds the machine.
        c.command(Command::StartJob(job(50)), t).unwrap();
        assert!(c.command(Command::Jog { axis: 'x', distance_mm: 1.0, feed_mm_min: 100.0 }, t).is_err());
        assert!(c.command(Command::Settings, t).is_err());
        assert!(c.command(Command::StartJob(job(1)), t).is_err());
        let events = c.command(Command::CancelJob, t).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::JobProgress(p) if p.failed.is_some())));
        assert_eq!(c.link().realtime.last(), Some(&realtime::HOLD));
        assert!(!c.job_active());
        // Status asked for no more often than STATUS_EVERY.
        let before = c.link().status_queries;
        c.poll(t);
        c.poll(t + STATUS_EVERY / 2);
        assert_eq!(c.link().status_queries, before);
        c.poll(t + STATUS_EVERY * 2);
        assert_eq!(c.link().status_queries, before + 1);
    }

    #[test]
    fn a_restart_or_a_broken_link_ends_the_job() {
        struct Broken;
        impl Transport for Broken {
            fn write_all(&mut self, _: &[u8]) -> std::io::Result<()> {
                Err(std::io::Error::other("unplugged"))
            }
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }
        let t = Instant::now();
        let mut c = Controller::new(Broken, StreamMode::LineByLine);
        let events = c.poll(t);
        assert!(matches!(events.last(), Some(Event::Lost(w)) if w.contains("unplugged")));
        assert!(c.poll(t).is_empty());
        // A soft reset mid-job (a welcome line) fails the job.
        let mut t = Instant::now();
        let mut c = Controller::new(SimulatedGrbl::default(), StreamMode::LineByLine);
        c.command(Command::StartJob(job(10)), t).unwrap();
        c.poll(t);
        let sim_reset = |c: &mut Controller<SimulatedGrbl>| c.link.write_all(&[realtime::SOFT_RESET]).unwrap();
        sim_reset(&mut c);
        run(&mut c, &mut t, 100);
        assert!(c.progress().unwrap().failed.as_deref().unwrap().contains("restarted"));
    }
}
