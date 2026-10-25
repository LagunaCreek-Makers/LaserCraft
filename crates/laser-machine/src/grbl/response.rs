//! What GRBL sends back: acknowledgements, errors, alarms, status reports, settings and messages.

/// The machine state a status report starts with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Run,
    /// Feed hold (0: complete, 1: still stopping).
    Hold(u8),
    Jog,
    /// Locked after an alarm: needs unlocking (`$X`) or homing (`$H`).
    Alarm,
    /// Safety door open (its sub-state: 0 closed and ready to resume … 3 restoring).
    Door(u8),
    /// Check mode (`$C`): lines are parsed, nothing moves.
    Check,
    Home,
    Sleep,
    /// A state this version doesn't know.
    Other(String),
}

impl State {
    pub fn parse(s: &str) -> Self {
        let (name, sub) = s.split_once(':').map_or((s, None), |(n, v)| (n, v.parse::<u8>().ok()));
        match name {
            "Idle" => State::Idle,
            "Run" => State::Run,
            "Hold" => State::Hold(sub.unwrap_or(0)),
            "Jog" => State::Jog,
            "Alarm" => State::Alarm,
            "Door" => State::Door(sub.unwrap_or(0)),
            "Check" => State::Check,
            "Home" => State::Home,
            "Sleep" => State::Sleep,
            _ => State::Other(s.to_string()),
        }
    }
    pub fn label(&self) -> String {
        match self {
            State::Idle => "Idle".into(),
            State::Run => "Run".into(),
            State::Hold(n) => format!("Hold:{n}"),
            State::Jog => "Jog".into(),
            State::Alarm => "Alarm".into(),
            State::Door(n) => format!("Door:{n}"),
            State::Check => "Check".into(),
            State::Home => "Home".into(),
            State::Sleep => "Sleep".into(),
            State::Other(s) => s.clone(),
        }
    }
}

/// A status report (`<Idle|MPos:…|FS:…>`). Fields the report leaves out are `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub state: State,
    /// Machine position (mm).
    pub machine: Option<[f64; 3]>,
    /// Work position (mm).
    pub work: Option<[f64; 3]>,
    /// Work coordinate offset (machine = work + offset).
    pub offset: Option<[f64; 3]>,
    /// Current feed (mm/min) and spindle/laser value.
    pub feed: Option<f64>,
    pub spindle: Option<f64>,
    /// Planner blocks and serial receive bytes free.
    pub buffer: Option<(u32, u32)>,
    /// Input pins active (`Pn:`): X/Y/Z limits, P probe, D door, H hold, R reset, S start.
    pub pins: String,
    /// Feed, rapid and spindle overrides (percent).
    pub overrides: Option<[u32; 3]>,
    pub raw: String,
}

impl Status {
    /// The work position, from the report or from the machine position less the last offset.
    pub fn work_position(&self, last_offset: Option<[f64; 3]>) -> Option<[f64; 3]> {
        self.work.or_else(|| {
            let (m, o) = (self.machine?, self.offset.or(last_offset)?);
            Some([m[0] - o[0], m[1] - o[1], m[2] - o[2]])
        })
    }
    pub fn door_open(&self) -> bool {
        matches!(self.state, State::Door(_)) || self.pins.contains('D')
    }
}

/// One line from GRBL.
#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    Ok,
    /// `error:N`: the line sent was refused.
    Error(u8),
    /// `ALARM:N`: the machine stopped and locked.
    Alarm(u8),
    Status(Box<Status>),
    /// `$N=value` from a settings report.
    Setting(u16, f64),
    /// `Grbl 1.1h ['$' for help]`: the controller (re)started.
    Welcome(String),
    /// `[MSG:…]`, `[GC:…]`, `[VER:…]` and the other bracketed feedback.
    Feedback(String),
    Other(String),
}

fn numbers<const N: usize>(s: &str) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    let mut parts = s.split(',');
    for o in &mut out {
        *o = parts.next()?.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    }
    Some(out)
}

fn parse_status(line: &str) -> Status {
    let content = line.trim().trim_start_matches('<').trim_end_matches('>');
    let mut fields = content.split('|');
    let state = State::parse(fields.next().unwrap_or(""));
    let mut s = Status {
        state,
        machine: None,
        work: None,
        offset: None,
        feed: None,
        spindle: None,
        buffer: None,
        pins: String::new(),
        overrides: None,
        raw: line.to_string(),
    };
    for field in fields {
        let Some((key, value)) = field.split_once(':') else { continue };
        match key {
            "MPos" => s.machine = numbers::<3>(value),
            "WPos" => s.work = numbers::<3>(value),
            "WCO" => s.offset = numbers::<3>(value),
            "F" => s.feed = numbers::<1>(value).map(|[f]| f),
            "FS" => {
                if let Some([f, sp]) = numbers::<2>(value) {
                    (s.feed, s.spindle) = (Some(f), Some(sp));
                }
            }
            "Bf" => {
                let mut p = value.split(',').map(|v| v.trim().parse::<u32>().ok());
                if let (Some(Some(a)), Some(Some(b))) = (p.next(), p.next()) {
                    s.buffer = Some((a, b));
                }
            }
            "Pn" => s.pins = value.chars().filter(char::is_ascii_alphabetic).take(16).collect(),
            "Ov" => {
                let mut p = value.split(',').map(|v| v.trim().parse::<u32>().ok());
                if let (Some(Some(a)), Some(Some(b)), Some(Some(c))) = (p.next(), p.next(), p.next()) {
                    s.overrides = Some([a, b, c]);
                }
            }
            _ => {}
        }
    }
    s
}

/// What `line` (without its line end) from GRBL says.
pub fn parse_response(line: &str) -> Response {
    let line = line.trim();
    if line == "ok" {
        return Response::Ok;
    }
    if let Some(n) = line.strip_prefix("error:") {
        return n.trim().parse().map_or_else(|_| Response::Other(line.to_string()), Response::Error);
    }
    if let Some(n) = line.strip_prefix("ALARM:") {
        return n.trim().parse().map_or_else(|_| Response::Other(line.to_string()), Response::Alarm);
    }
    if line.starts_with('<') && line.ends_with('>') {
        return Response::Status(Box::new(parse_status(line)));
    }
    if let Some(rest) = line.strip_prefix('$')
        && let Some((id, value)) = rest.split_once('=')
        && let (Ok(id), Ok(value)) = (id.trim().parse::<u16>(), value.trim().parse::<f64>())
        && value.is_finite()
    {
        return Response::Setting(id, value);
    }
    if line.starts_with("Grbl ") {
        return Response::Welcome(line.to_string());
    }
    if line.starts_with('[') && line.ends_with(']') {
        return Response::Feedback(line.to_string());
    }
    Response::Other(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_kind_of_line() {
        assert_eq!(parse_response("ok"), Response::Ok);
        assert_eq!(parse_response("error:22"), Response::Error(22));
        assert_eq!(parse_response("ALARM:1"), Response::Alarm(1));
        assert_eq!(parse_response("$130=400.000"), Response::Setting(130, 400.0));
        assert_eq!(parse_response("$32=1"), Response::Setting(32, 1.0));
        assert!(matches!(parse_response("Grbl 1.1h ['$' for help]"), Response::Welcome(_)));
        assert!(matches!(parse_response("[MSG:'$H'|'$X' to unlock]"), Response::Feedback(_)));
        assert!(matches!(parse_response("error:x"), Response::Other(_)));
        assert!(matches!(parse_response("$N0=G21"), Response::Other(_)));
        assert!(matches!(parse_response("$130=inf"), Response::Other(_)));
    }

    #[test]
    fn parses_status_reports() {
        let Response::Status(s) = parse_response("<Run|MPos:10.000,20.500,0.000|Bf:15,128|FS:1200,500|Pn:XD|WCO:1.000,2.000,0.000|Ov:100,100,80>")
        else {
            panic!()
        };
        assert_eq!(s.state, State::Run);
        assert_eq!(s.machine, Some([10.0, 20.5, 0.0]));
        assert_eq!((s.feed, s.spindle), (Some(1200.0), Some(500.0)));
        assert_eq!(s.buffer, Some((15, 128)));
        assert_eq!(s.pins, "XD");
        assert!(s.door_open());
        assert_eq!(s.overrides, Some([100, 100, 80]));
        assert_eq!(s.work_position(None), Some([9.0, 18.5, 0.0]));
        let Response::Status(s) = parse_response("<Hold:1|WPos:1,2,3|F:500>") else { panic!() };
        assert_eq!((s.state, s.work, s.feed), (State::Hold(1), Some([1.0, 2.0, 3.0]), Some(500.0)));
        let Response::Status(s) = parse_response("<Door:2|MPos:a,b,c>") else { panic!() };
        assert_eq!((s.state.clone(), s.machine), (State::Door(2), None));
        assert_eq!(s.state.label(), "Door:2");
        // The offset comes in some reports only: the last one is used.
        let Response::Status(s) = parse_response("<Idle|MPos:5,5,0>") else { panic!() };
        assert_eq!(s.work_position(Some([1.0, 1.0, 0.0])), Some([4.0, 4.0, 0.0]));
        assert_eq!(State::parse("Weird"), State::Other("Weird".into()));
    }
}
