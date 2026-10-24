//! Phase 8: read-only GRBL serial monitoring. No motion or laser commands.
//! Runs serial I/O off the egui thread; events are drained without blocking UI.
use crate::grbl_stream::{Progress, Stream};
use std::{
    io::{Read, Write},
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
        if !matches!(axis, 'X' | 'Y' | 'Z')
            || !distance_mm.is_finite()
            || distance_mm.abs() > 10.0
            || distance_mm == 0.0
            || !feed_mm_min.is_finite()
            || !(1.0..=1000.0).contains(&feed_mm_min)
        {
            return Err("Jog must be X/Y/Z, nonzero within ±10 mm, and 1–1000 mm/min".into());
        }
        self.commands.send(Command::Jog { axis, distance_mm, feed_mm_min }).map_err(|e| e.to_string())
    }
    pub fn start_job(&self, code: String) -> Result<(), String> {
        Stream::new(&code)?;
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
            let mut port = match serialport::new(&port_name, baud).timeout(Duration::from_millis(120)).open() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(Event::Disconnected(format!("Open failed: {e}")));
                    return;
                }
            };
            let _ = tx.send(Event::Connected);
            // GRBL '?' is a read-only real-time status request. Do not reset controller on connect.
            let mut last_poll = Instant::now() - Duration::from_secs(2);
            let mut stream: Option<Stream> = None;
            let mut pending = Vec::<u8>::new();
            let mut buffer = [0u8; 256];
            loop {
                match rx.try_recv() {
                    Ok(Command::Disconnect) => {
                        if let Some(s) = stream.as_mut() {
                            s.abort("Disconnected during job");
                            let _ = tx.send(Event::JobProgress(s.progress.clone()));
                        }
                        break;
                    }
                    Ok(command) => {
                        let result: Result<(), String> = match command {
                            Command::StartJob(code) => {
                                if stream.as_ref().is_some_and(|s| !s.progress.finished && s.progress.failed.is_none()) {
                                    Err("Job already active".into())
                                } else {
                                    match Stream::new(&code) {
                                        Ok(s) => {
                                            let _ = tx.send(Event::JobProgress(s.progress.clone()));
                                            stream = Some(s);
                                            Ok(())
                                        }
                                        Err(e) => Err(e),
                                    }
                                }
                            }
                            Command::CancelJob => {
                                if let Some(s) = stream.as_mut() {
                                    s.abort("Cancelled by operator; controller feed hold requested");
                                    let _ = tx.send(Event::JobProgress(s.progress.clone()));
                                }
                                port.write_all(b"!").map_err(|e| e.to_string())
                            }
                            Command::Identify if stream.as_ref().is_some_and(|s| !s.progress.finished && s.progress.failed.is_none()) => {
                                Err("Cannot query firmware during active job".into())
                            }
                            Command::Identify => port.write_all(b"$I\n").map_err(|e| e.to_string()),
                            Command::Settings if stream.as_ref().is_some_and(|s| !s.progress.finished && s.progress.failed.is_none()) => {
                                Err("Cannot query settings during active job".into())
                            }
                            Command::Settings => port.write_all(b"$$\n").map_err(|e| e.to_string()),
                            Command::Hold => port.write_all(b"!").map_err(|e| e.to_string()),
                            Command::Resume => port.write_all(b"~").map_err(|e| e.to_string()),
                            Command::Jog { axis: _, distance_mm: _, feed_mm_min: _ }
                                if stream.as_ref().is_some_and(|s| !s.progress.finished && s.progress.failed.is_none()) =>
                            {
                                Err("Jogging disabled during active job".into())
                            }
                            Command::Jog { axis, distance_mm, feed_mm_min } => {
                                // GRBL 1.1 jogging command, independent of modal G-code state.
                                let line = format!("$J=G91 G21 {axis}{distance_mm:.3} F{feed_mm_min:.1}\n");
                                port.write_all(line.as_bytes()).map_err(|e| e.to_string())
                            }
                            Command::Disconnect => Ok(()),
                        };
                        if let Err(e) = result {
                            let _ = tx.send(Event::Disconnected(format!("Command write failed: {e}")));
                            return;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
                if let Some(s) = stream.as_mut()
                    && s.timed_out(Duration::from_secs(15))
                {
                    let _ = tx.send(Event::JobProgress(s.progress.clone()));
                    let _ = tx.send(Event::Disconnected("Job acknowledgement timed out; reconnect and inspect controller before retrying".into()));
                    return;
                }
                if let Some(s) = stream.as_mut()
                    && let Some(line) = s.next_line()
                {
                    if let Err(e) = port.write_all(line.as_bytes()) {
                        s.abort(&format!("Write failed: {e}"));
                        let _ = tx.send(Event::JobProgress(s.progress.clone()));
                        let _ = tx.send(Event::Disconnected(e.to_string()));
                        return;
                    }
                    let _ = tx.send(Event::JobProgress(s.progress.clone()));
                }
                if last_poll.elapsed() >= Duration::from_millis(500) {
                    if let Err(e) = port.write_all(b"?") {
                        let _ = tx.send(Event::Disconnected(format!("Status query failed: {e}")));
                        return;
                    }
                    last_poll = Instant::now();
                }
                match port.read(&mut buffer) {
                    Ok(n) => {
                        for byte in &buffer[..n] {
                            if *byte == b'\n' {
                                let line = String::from_utf8_lossy(&pending).trim().to_string();
                                pending.clear();
                                if let Some(s) = stream.as_mut()
                                    && (line == "ok" || line.starts_with("error:") || line.starts_with("ALARM:"))
                                {
                                    s.response(&line);
                                    let _ = tx.send(Event::JobProgress(s.progress.clone()));
                                }
                                if line.is_empty() {
                                    continue;
                                }
                                if line.starts_with('<') {
                                    let _ = tx.send(Event::Status(parse_status(&line)));
                                } else {
                                    let _ = tx.send(Event::Line(line));
                                }
                            } else if pending.len() < 4096 {
                                pending.push(*byte);
                            } else {
                                pending.clear();
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        let _ = tx.send(Event::Disconnected(format!("Read failed: {e}")));
                        return;
                    }
                }
            }
            let _ = tx.send(Event::Disconnected("Disconnected by user".into()));
        });
        Self { events, commands }
    }
}
pub fn list_ports() -> Result<Vec<String>, String> {
    serialport::available_ports().map(|ports| ports.into_iter().map(|p| p.port_name).collect()).map_err(|e| e.to_string())
}
fn xyz(value: &str) -> Option<[f64; 3]> {
    let mut parts = value.split(',');
    Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?])
}
pub fn parse_status(line: &str) -> GrblStatus {
    let mut status = GrblStatus { raw: line.to_string(), ..Default::default() };
    let content = line.trim().trim_start_matches('<').trim_end_matches('>');
    for (i, field) in content.split('|').enumerate() {
        if i == 0 {
            status.state = field.to_string();
            continue;
        }
        if let Some(value) = field.strip_prefix("MPos:") {
            status.machine_position = xyz(value);
        }
        if let Some(value) = field.strip_prefix("WPos:") {
            status.work_position = xyz(value);
        }
    }
    status
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_grbl_status() {
        let s = parse_status("<Idle|MPos:1.000,2.000,3.000|WPos:0.000,1.000,2.000|FS:0,0>");
        assert_eq!(s.state, "Idle");
        assert_eq!(s.machine_position, Some([1., 2., 3.]));
        assert_eq!(s.work_position, Some([0., 1., 2.]));
    }
}
