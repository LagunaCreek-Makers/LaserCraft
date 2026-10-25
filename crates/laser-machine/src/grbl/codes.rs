//! What GRBL's error and alarm numbers mean, in our own words (from its public interface
//! documentation; nothing copied).

/// The meaning of `error:N`.
pub fn error(n: u8) -> &'static str {
    match n {
        1 => "a word in the line has no letter",
        2 => "a number in the line is malformed",
        3 => "the $ command isn't recognised",
        4 => "a value that must be positive is negative",
        5 => "homing isn't enabled in the settings",
        6 => "the step pulse time is too short",
        7 => "the settings memory could not be read: defaults were restored",
        8 => "this $ command only works while the machine is idle",
        9 => "G-code is locked out during an alarm or a jog",
        10 => "soft limits need homing to be enabled",
        11 => "the line is too long",
        12 => "the step rate is too high for the settings",
        13 => "the safety door is open",
        14 => "the line is too long to store",
        15 => "the jog would go past the machine's travel",
        16 => "the jog command is malformed",
        17 => "laser mode needs a PWM spindle output",
        20 => "unsupported or unknown G-code command",
        21 => "two commands from the same group in one line",
        22 => "no feed rate has been set",
        23 => "a value that must be a whole number isn't",
        24 => "two commands in the line both need axis words",
        25 => "a word appears twice in the line",
        26 => "the command needs axis words and has none",
        27 => "the line number is out of range",
        28 => "a command is missing a value it needs",
        29 => "that work coordinate system isn't supported",
        30 => "G53 only works with G0 or G1",
        31 => "the line has axis words nothing uses",
        32 => "the arc has no axis words in its plane",
        33 => "the move's target is invalid",
        34 => "the arc's radius doesn't fit its end points",
        35 => "the arc has no offset in its plane",
        36 => "the line has values nothing uses",
        37 => "the tool length offset isn't on the configured axis",
        38 => "the tool number is too large",
        _ => "unknown error",
    }
}

/// The meaning of `ALARM:N`, and what to do about it.
pub fn alarm(n: u8) -> &'static str {
    match n {
        1 => "a limit switch was hit: the position is lost; check the machine, then home it",
        2 => "the move would go past the machine's travel (soft limit): nothing moved; unlock and fix the job",
        3 => "reset while moving: the position may be lost; home the machine",
        4 => "probe was already triggered before probing",
        5 => "probe didn't touch within its travel",
        6 => "homing stopped: the machine was reset",
        7 => "homing stopped: the safety door opened",
        8 => "homing failed: a limit switch didn't release",
        9 => "homing failed: a limit switch wasn't found",
        _ => "unknown alarm",
    }
}

/// `line` from GRBL with the meaning of its error or alarm number added (other lines as they are).
pub fn explain(line: &str) -> String {
    let t = line.trim();
    if let Some(n) = t.strip_prefix("error:").and_then(|n| n.trim().parse::<u8>().ok()) {
        return format!("{t} ({})", error(n));
    }
    if let Some(n) = t.strip_prefix("ALARM:").and_then(|n| n.trim().parse::<u8>().ok()) {
        return format!("{t} ({})", alarm(n));
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_have_meanings() {
        assert!(error(22).contains("feed rate"));
        assert!(alarm(1).contains("limit switch"));
        assert_eq!(error(200), "unknown error");
        assert_eq!(alarm(0), "unknown alarm");
        assert_eq!(explain("error:9"), "error:9 (G-code is locked out during an alarm or a jog)");
        assert!(explain("ALARM:2").starts_with("ALARM:2 (the move would go past"));
        assert_eq!(explain(" ok "), "ok");
    }
}
