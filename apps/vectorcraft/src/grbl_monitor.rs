//! The GRBL serial link: a thread that runs `lasercraft_machine::Controller` over the serial
//! port, so the egui thread never blocks. The protocol, streaming and their tests live in
//! `lasercraft-machine`; this is the port, the thread and the messages to and from the panel.
use crate::grbl_stream::Progress;
use lasercraft_machine::grbl::{Expect, State, StreamMode};
use lasercraft_machine::{Controller, Transport};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default)]
pub struct GrblStatus {
    pub state: String,
    pub machine_position: Option<[f64; 3]>,
    pub work_position: Option<[f64; 3]>,
    pub alarm: Option<String>,
    #[allow(dead_code, reason = "collected from `$I` replies; not shown in the panel yet")]
    pub identity: Vec<String>,
    pub raw: String,
}
#[derive(Debug)]
pub enum Event {
    Connected,
    Disconnected(String),
    Line(String),
    Status(GrblStatus),
    #[allow(dead_code, reason = "the panel handles it; port scans do not send it yet")]
    Ports(Vec<String>),
    JobProgress(Progress),
}
#[derive(Debug)]
pub enum Command {
    Disconnect,
    Identify,
    Settings,
    Hold,
    Resume,
    Jog { axis: char, distance_mm: f64, feed_mm_min: f64 },
    StartJob(String),
    CancelJob,
    Stop,
    Unlock,
    Home,
}

pub struct Monitor {
    pub events: Receiver<Event>,
    commands: Sender<Command>,
    link: Link,
}

/// What a monitor talks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// A real machine on a serial port: automation may not move it (panel only).
    Serial,
    /// The built-in simulator: nothing physical, automation may drive it.
    Simulator,
}

impl Link {
    pub fn id(self) -> &'static str {
        match self {
            Link::Serial => "serial",
            Link::Simulator => "simulator",
        }
    }
}

/// The serial port as the controller's link (reads time out quickly: nothing arrived).
struct Serial(Box<dyn serialport::SerialPort>);
impl Transport for Serial {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        std::io::Write::write_all(&mut self.0, bytes)
    }
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, buf)
    }
}

impl Monitor {
    pub fn identify(&self) {
        let _ = self.commands.send(Command::Identify);
    }
    pub fn settings(&self) {
        let _ = self.commands.send(Command::Settings);
    }
    pub fn hold(&self) {
        let _ = self.commands.send(Command::Hold);
    }
    pub fn resume(&self) {
        let _ = self.commands.send(Command::Resume);
    }
    pub fn jog(&self, axis: char, distance_mm: f64, feed_mm_min: f64) -> Result<(), String> {
        use lasercraft_machine::controller::{MAX_JOG_FEED, MAX_JOG_MM};
        lasercraft_machine::grbl::jog_line(axis, distance_mm, feed_mm_min, MAX_JOG_MM, MAX_JOG_FEED)?;
        self.commands.send(Command::Jog { axis, distance_mm, feed_mm_min }).map_err(|e| e.to_string())
    }
    pub fn start_job(&self, code: String) -> Result<(), String> {
        lasercraft_machine::grbl::job_lines(&code)?;
        self.commands.send(Command::StartJob(code)).map_err(|e| e.to_string())
    }
    pub fn cancel_job(&self) {
        let _ = self.commands.send(Command::CancelJob);
    }
    /// Stop: feed hold, then a soft reset once halted (the laser goes off).
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }
    /// `$X`: clear an alarm lock.
    pub fn unlock(&self) {
        let _ = self.commands.send(Command::Unlock);
    }
    /// `$H`: home the machine (it moves).
    pub fn home(&self) {
        let _ = self.commands.send(Command::Home);
    }
    pub fn disconnect(&self) {
        let _ = self.commands.send(Command::Disconnect);
    }
    /// Opens `port_name`; jobs are checked against `expect` (the machine profile) before they start.
    pub fn connect(port_name: String, baud: u32, expect: Expect) -> Self {
        Self::spawn(Link::Serial, expect, move || {
            serialport::new(&port_name, baud)
                .timeout(Duration::from_millis(120))
                .open()
                .map(|p| Box::new(Serial(p)) as Box<dyn Transport + Send>)
                .map_err(|e| format!("Open failed: {e}"))
        })
    }

    /// The built-in GRBL simulator instead of a machine: nothing physical moves, so automation
    /// may drive it fully (tests, demos, CI).
    pub fn connect_simulator(expect: Expect) -> Self {
        Self::spawn(Link::Simulator, expect, || Ok(Box::new(lasercraft_machine::sim::SimulatedGrbl::default()) as Box<dyn Transport + Send>))
    }

    /// What this monitor is linked to (decided when it was opened, never by a request).
    pub fn link(&self) -> Link {
        self.link
    }

    /// Runs the controller over what `open` gives, on its own thread.
    pub(crate) fn spawn(link: Link, expect: Expect, open: impl FnOnce() -> Result<Box<dyn Transport + Send>, String> + Send + 'static) -> Self {
        let (tx, events) = mpsc::channel();
        let (commands, rx) = mpsc::channel();
        thread::spawn(move || {
            let transport = match open() {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(Event::Disconnected(e));
                    return;
                }
            };
            let _ = tx.send(Event::Connected);
            // One line at a time: nothing is queued in the controller unseen. The controller
            // asks for status ('?', read-only) itself; connecting doesn't reset the machine.
            let mut c = Controller::new(transport, StreamMode::LineByLine);
            c.expect = expect;
            // The settings (read only) let jobs be checked before they start: laser mode,
            // power range, travel.
            if let Ok(events) = c.command(lasercraft_machine::Command::Settings, Instant::now())
                && forward(&tx, events, &c, &mut vec![])
            {
                return;
            }
            let mut last_identity = vec![];
            loop {
                let now = Instant::now();
                match rx.try_recv() {
                    Ok(Command::Disconnect) => {
                        if c.job_active() {
                            let _ = c.command(lasercraft_machine::Command::CancelJob, now);
                        }
                        break;
                    }
                    Ok(command) => {
                        let command = match command {
                            Command::Identify => lasercraft_machine::Command::Identify,
                            Command::Settings => lasercraft_machine::Command::Settings,
                            Command::Hold => lasercraft_machine::Command::Hold,
                            Command::Resume => lasercraft_machine::Command::Resume,
                            Command::Jog { axis, distance_mm, feed_mm_min } => lasercraft_machine::Command::Jog { axis, distance_mm, feed_mm_min },
                            Command::StartJob(code) => lasercraft_machine::Command::StartJob(code),
                            Command::CancelJob => lasercraft_machine::Command::CancelJob,
                            Command::Stop => lasercraft_machine::Command::Stop,
                            Command::Unlock => lasercraft_machine::Command::Unlock,
                            Command::Home => lasercraft_machine::Command::Home,
                            Command::Disconnect => continue,
                        };
                        match c.command(command, now) {
                            Ok(events) => {
                                if forward(&tx, events, &c, &mut last_identity) {
                                    return;
                                }
                            }
                            // Refused (a job runs, a jog out of range…): the panel shows why.
                            Err(e) => {
                                let _ = tx.send(Event::Line(format!("Refused: {e}")));
                            }
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
                if forward(&tx, c.poll(now), &c, &mut last_identity) {
                    return;
                }
                // A serial read waits for its timeout; the simulator answers at once.
                if link == Link::Simulator {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            let _ = tx.send(Event::Disconnected("Disconnected by user".into()));
        });
        Self { events, commands, link }
    }
}

/// Passes the controller's events to the panel; true when the link was lost (the thread ends).
fn forward<T: Transport>(tx: &Sender<Event>, events: Vec<lasercraft_machine::Event>, c: &Controller<T>, identity: &mut Vec<String>) -> bool {
    for e in events {
        match e {
            lasercraft_machine::Event::Line(line) => {
                if line.starts_with("[VER:") || line.starts_with("[OPT:") {
                    identity.push(line.clone());
                    identity.truncate(16);
                }
                let _ = tx.send(Event::Line(line));
            }
            lasercraft_machine::Event::Status(s) => {
                let status = GrblStatus {
                    state: s.state.label(),
                    machine_position: s.machine,
                    work_position: s.work_position(c.offset),
                    alarm: (s.state == State::Alarm).then(|| "ALARM: unlock ($X) or home ($H) after checking the machine".into()),
                    identity: identity.clone(),
                    raw: s.raw.clone(),
                };
                let _ = tx.send(Event::Status(status));
            }
            lasercraft_machine::Event::JobProgress(p) => {
                let _ = tx.send(Event::JobProgress(p));
            }
            lasercraft_machine::Event::Lost(why) => {
                let _ = tx.send(Event::Disconnected(why));
                return true;
            }
        }
    }
    false
}

pub fn list_ports() -> Result<Vec<String>, String> {
    serialport::available_ports().map(|ports| ports.into_iter().map(|p| p.port_name).collect()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lasercraft_machine::sim::SimulatedGrbl;

    /// The controller's events reach the panel as before: status with positions, an alarm state
    /// flagged, job progress, and a lost link as a disconnect.
    #[test]
    fn controller_events_reach_the_panel() {
        let (tx, rx) = mpsc::channel();
        let t = Instant::now();
        let mut sim = SimulatedGrbl::default();
        sim.state = "Alarm".into();
        sim.position = [1.0, 2.0, 0.0];
        let mut c = Controller::new(sim, StreamMode::LineByLine);
        let mut identity = vec![];
        assert!(!forward(&tx, c.poll(t), &c, &mut identity));
        let Ok(Event::Status(s)) = rx.try_recv() else { panic!("a status") };
        assert_eq!((s.state.as_str(), s.machine_position), ("Alarm", Some([1.0, 2.0, 0.0])));
        assert!(s.alarm.is_some());
        // An alarmed machine refuses a job; an idle one runs it.
        assert!(c.command(lasercraft_machine::Command::StartJob("G21\nG90".into()), t).is_err());
        let mut c = Controller::new(SimulatedGrbl::default(), StreamMode::LineByLine);
        assert!(!forward(&tx, c.poll(t), &c, &mut identity));
        let _ = rx.try_recv();
        let events = c.command(lasercraft_machine::Command::StartJob("G21\nG90".into()), t).unwrap();
        assert!(!forward(&tx, events, &c, &mut identity));
        // (After a warning that the settings weren't read.)
        let got: Vec<Event> = rx.try_iter().collect();
        assert!(got.iter().any(|e| matches!(e, Event::JobProgress(_))), "{got:?}");
        assert!(got.iter().any(|e| matches!(e, Event::Line(l) if l.starts_with("Warning:"))), "{got:?}");
        assert!(forward(&tx, vec![lasercraft_machine::Event::Lost("unplugged".into())], &c, &mut identity));
        assert!(rx.try_iter().any(|e| matches!(e, Event::Disconnected(w) if w == "unplugged")));
    }
}
