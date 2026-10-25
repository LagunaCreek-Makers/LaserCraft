//! The GRBL serial link: a thread that runs `lasercraft_machine::Controller` over the serial
//! port, so the egui thread never blocks. The protocol, streaming and their tests live in
//! `lasercraft-machine`; this is the port, the thread and the messages to and from the panel.
use crate::grbl_stream::Progress;
use lasercraft_machine::grbl::{State, StreamMode};
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
}

pub struct Monitor {
    pub events: Receiver<Event>,
    commands: Sender<Command>,
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
    pub fn disconnect(&self) {
        let _ = self.commands.send(Command::Disconnect);
    }
    pub fn connect(port_name: String, baud: u32) -> Self {
        let (tx, events) = mpsc::channel();
        let (commands, rx) = mpsc::channel();
        thread::spawn(move || {
            let port = match serialport::new(&port_name, baud).timeout(Duration::from_millis(120)).open() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(Event::Disconnected(format!("Open failed: {e}")));
                    return;
                }
            };
            let _ = tx.send(Event::Connected);
            // One line at a time: nothing is queued in the controller unseen. The controller
            // asks for status ('?', read-only) itself; connecting doesn't reset the machine.
            let mut c = Controller::new(Serial(port), StreamMode::LineByLine);
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
            }
            let _ = tx.send(Event::Disconnected("Disconnected by user".into()));
        });
        Self { events, commands }
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
        let events = c.command(lasercraft_machine::Command::StartJob("G21\nG90".into()), t).unwrap();
        assert!(!forward(&tx, events, &c, &mut identity));
        assert!(matches!(rx.try_recv(), Ok(Event::JobProgress(_))));
        assert!(forward(&tx, vec![lasercraft_machine::Event::Lost("unplugged".into())], &c, &mut identity));
        assert!(rx.try_iter().any(|e| matches!(e, Event::Disconnected(w) if w == "unplugged")));
    }
}
