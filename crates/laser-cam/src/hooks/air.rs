//! Offline air-assist command planning. No GPIO/network packets are sent here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirMode {
    #[default]
    Inherit,
    On,
    Off,
}
impl AirMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inherit => "Inherit material",
            Self::On => "On",
            Self::Off => "Off",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AirConfig {
    pub enabled: bool,
    pub on_command: String,
    pub off_command: String,
    pub default_on: bool,
    pub preflow_ms: u32,
    pub postflow_ms: u32,
}
impl Default for AirConfig {
    fn default() -> Self {
        Self { enabled: false, on_command: "M8".into(), off_command: "M9".into(), default_on: false, preflow_ms: 0, postflow_ms: 0 }
    }
}
fn validate_command(s: &str) -> Result<&str, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.len() > 80 || trimmed.chars().any(|c| matches!(c, '\r' | '\n' | ';' | '(' | ')')) || !trimmed.is_ascii() {
        return Err("Air-assist command must be one ASCII G-code line without comments or separators".into());
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '+' | '-')) {
        return Err("Unsupported character in air-assist command".into());
    }
    Ok(trimmed)
}
impl AirConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let on = validate_command(&self.on_command)?;
        let off = validate_command(&self.off_command)?;
        if on.eq_ignore_ascii_case(off) {
            return Err("Air-assist ON and OFF commands must differ".into());
        }
        if self.preflow_ms > 30000 || self.postflow_ms > 30000 {
            return Err("Air-assist delays must be <= 30000 ms".into());
        }
        Ok(())
    }
    pub fn apply(&self, source: &str, mode: AirMode) -> Result<String, String> {
        if !self.enabled {
            return Ok(source.to_owned());
        }
        self.validate()?;
        if source.trim().is_empty() {
            return Err("Cannot apply air assist to empty G-code".into());
        }
        // Do not double-inject. A file containing explicit air-assist commands needs manual review.
        let on = validate_command(&self.on_command)?;
        let off = validate_command(&self.off_command)?;
        if source.lines().any(|l| l.trim().eq_ignore_ascii_case(on) || l.trim().eq_ignore_ascii_case(off)) {
            return Err("G-code already contains an air-assist command; remove it or disable automatic injection".into());
        }
        let use_air = match mode {
            AirMode::Inherit => self.default_on,
            AirMode::On => true,
            AirMode::Off => false,
        };
        let mut output = String::from("; LaserCraft air-assist policy (verify controller output wiring)\n");
        if use_air {
            output.push_str(on);
            output.push('\n');
            if self.preflow_ms > 0 {
                output.push_str(&format!("G4 P{:.3}\n", self.preflow_ms as f64 / 1000.0));
            }
        } else {
            output.push_str(off);
            output.push('\n');
        }
        // Insert shutdown BEFORE terminal program end, otherwise it may never execute.
        for line in source.lines() {
            let code = line.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
            if code == "M2" || code == "M30" {
                if use_air && self.postflow_ms > 0 {
                    output.push_str(&format!("G4 P{:.3}\n", self.postflow_ms as f64 / 1000.0));
                }
                output.push_str(off);
                output.push('\n');
            }
            output.push_str(line);
            output.push('\n');
        }
        if !source.lines().any(|l| matches!(l.split(';').next().unwrap_or("").trim().to_ascii_uppercase().as_str(), "M2" | "M30")) {
            if use_air && self.postflow_ms > 0 {
                output.push_str(&format!("G4 P{:.3}\n", self.postflow_ms as f64 / 1000.0));
            }
            output.push_str(off);
            output.push('\n');
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_disabled() {
        assert_eq!(AirConfig::default().apply("G1 X1\n", AirMode::On).unwrap(), "G1 X1\n");
    }
    #[test]
    fn on_off_before_end() {
        let c = AirConfig { enabled: true, ..Default::default() };
        let s = c.apply("G1 X1\nM2\n", AirMode::On).unwrap();
        assert!(s.find("M8").unwrap() < s.find("G1").unwrap());
        assert!(s.find("M9").unwrap() < s.find("M2").unwrap());
    }
    #[test]
    fn rejects_multiline() {
        let c = AirConfig { enabled: true, on_command: "M8\nM3".into(), ..Default::default() };
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_existing() {
        let c = AirConfig { enabled: true, ..Default::default() };
        assert!(c.apply("M8\nG1 X1", AirMode::On).is_err());
    }
}
