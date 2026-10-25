//! A scripted GRBL 1.1 stand-in for tests: it takes bytes as a controller would, keeps a
//! 128-byte receive buffer (and notes any overflow), answers lines in order, status queries and
//! real-time commands at once. It drives no hardware and models no physics.

use std::collections::VecDeque;

use crate::Transport;
use crate::grbl::realtime;

#[derive(Clone, Debug)]
pub struct SimulatedGrbl {
    /// The state its status reports give.
    pub state: String,
    pub position: [f64; 3],
    /// Lines answered so far.
    pub accepted: usize,
    /// Answer line N (1-based) with `error:2`.
    pub reject_at: Option<usize>,
    /// Never answer line N (a lost acknowledgement).
    pub silence_at: Option<usize>,
    /// Raise this alarm when line N arrives.
    pub alarm_at: Option<(usize, u8)>,
    /// Lines answered per read (how fast it "runs" the job).
    pub lines_per_read: usize,
    /// The receive buffer once held more than 128 bytes (a streamer bug).
    pub overflowed: bool,
    /// Every line taken, in order.
    pub lines: Vec<String>,
    /// Real-time bytes received, in order (status queries counted apart).
    pub realtime: Vec<u8>,
    pub status_queries: usize,
    /// What `$$` lists.
    pub settings: Vec<(u16, f64)>,
    pub(crate) rx: VecDeque<u8>,
    pub(crate) out: VecDeque<u8>,
}

impl Default for SimulatedGrbl {
    fn default() -> Self {
        Self {
            state: "Idle".into(),
            position: [0.0; 3],
            accepted: 0,
            reject_at: None,
            silence_at: None,
            alarm_at: None,
            lines_per_read: 1,
            overflowed: false,
            lines: vec![],
            realtime: vec![],
            status_queries: 0,
            settings: vec![(30, 1000.0), (32, 1.0), (110, 6000.0), (111, 6000.0), (120, 500.0), (121, 500.0), (130, 400.0), (131, 300.0)],
            rx: VecDeque::new(),
            out: VecDeque::new(),
        }
    }
}

impl SimulatedGrbl {
    fn say(&mut self, line: &str) {
        self.out.extend(line.bytes());
        self.out.extend(b"\r\n");
    }

    fn take_line(&mut self) -> Option<String> {
        let end = self.rx.iter().position(|b| *b == b'\n')?;
        let bytes: Vec<u8> = self.rx.drain(..=end).collect();
        Some(String::from_utf8_lossy(&bytes).trim().to_string())
    }

    fn answer(&mut self, line: String) {
        self.accepted += 1;
        let n = self.accepted;
        self.lines.push(line.clone());
        if let Some((at, alarm)) = self.alarm_at
            && at == n
        {
            self.state = "Alarm".into();
            self.say(&format!("ALARM:{alarm}"));
            return;
        }
        if self.silence_at == Some(n) {
            return;
        }
        if self.reject_at == Some(n) {
            self.say("error:2");
            return;
        }
        match line.as_str() {
            "$$" => {
                for (id, v) in self.settings.clone() {
                    self.say(&format!("${id}={v}"));
                }
            }
            "$I" => self.say("[VER:1.1h.20190825:]"),
            "$X" | "$H" => self.state = "Idle".into(),
            _ if self.state == "Alarm" => {
                self.say("error:9");
                return;
            }
            _ => {}
        }
        self.say("ok");
    }
}

impl Transport for SimulatedGrbl {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        for &b in bytes {
            match b {
                realtime::STATUS => {
                    self.status_queries += 1;
                    let [x, y, z] = self.position;
                    let s = format!("<{}|MPos:{x:.3},{y:.3},{z:.3}|FS:0,0>", self.state);
                    self.say(&s);
                }
                realtime::HOLD | realtime::RESUME | realtime::SOFT_RESET | realtime::JOG_CANCEL => {
                    self.realtime.push(b);
                    match b {
                        realtime::HOLD => self.state = "Hold:0".into(),
                        realtime::RESUME => self.state = "Run".into(),
                        realtime::SOFT_RESET => {
                            self.rx.clear();
                            self.state = if self.state == "Alarm" { "Alarm".into() } else { "Idle".into() };
                            self.say("Grbl 1.1h ['$' for help]");
                        }
                        _ => {}
                    }
                }
                _ => {
                    self.rx.push_back(b);
                    if self.rx.len() > crate::grbl::RX_BUFFER {
                        self.overflowed = true;
                    }
                }
            }
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for _ in 0..self.lines_per_read.max(1) {
            match self.take_line() {
                Some(line) => self.answer(line),
                None => break,
            }
        }
        let n = buf.len().min(self.out.len());
        for (slot, b) in buf.iter_mut().zip(self.out.drain(..n)) {
            *slot = b;
        }
        Ok(n)
    }
}
