//! Configurable *non-safety-critical* auxiliary outputs for offline G-code planning.
//! Never use a G-code accessory to actuate fire suppression or another safety system.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccessoryTransport {
    #[default]
    Gcode,
    Serial,
    ModbusTcp,
    Mqtt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessoryEndpoint {
    pub transport: AccessoryTransport,
    pub address: String, // Serial device path, Modbus host:port, or MQTT broker host:port
    pub baud: u32,
    pub channel: u16, // Modbus coil (zero-based)
    pub unit_id: u8,
    pub mqtt_topic: String,
    pub mqtt_on: String,
    pub mqtt_off: String,
    pub mqtt_pwm_template: String,
}
impl Default for AccessoryEndpoint {
    fn default() -> Self {
        Self {
            transport: AccessoryTransport::Gcode,
            address: String::new(),
            baud: 115200,
            channel: 0,
            unit_id: 1,
            mqtt_topic: String::new(),
            mqtt_on: "ON".into(),
            mqtt_off: "OFF".into(),
            mqtt_pwm_template: "{value}".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Accessory {
    pub endpoint: AccessoryEndpoint,
    pub mode: AccessoryMode,
    pub active_high: bool,
    pub pwm_value: u32,
    pub pwm_max: u32,
    pub pwm_command: String,
    pub name: String,
    pub enabled: bool,
    pub on_command: String,
    pub off_command: String,
    pub pre_delay_ms: u32,
    pub post_delay_ms: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccessoryMode {
    #[default]
    OnOff,
    Pwm,
}
impl Default for Accessory {
    fn default() -> Self {
        Self {
            endpoint: AccessoryEndpoint::default(),
            mode: AccessoryMode::OnOff,
            active_high: true,
            pwm_value: 128,
            pwm_max: 255,
            pwm_command: "M7 S{value}".into(),
            name: "Ventilation fan".into(),
            enabled: false,
            on_command: "M7".into(),
            off_command: "M9".into(),
            pre_delay_ms: 0,
            post_delay_ms: 0,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessoryConfig {
    pub outputs: Vec<Accessory>,
}

fn command(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_uppercase();
    if s.is_empty()
        || s.len() > 80
        || !s.is_ascii()
        || s.chars().any(|c| matches!(c, '\n' | '\r' | ';' | '(' | ')'))
        || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '+' | '-'))
    {
        return Err("Accessory command must be a single ASCII G-code line".into());
    }
    // M3/M4/M5 are laser/spindle controls; G0/G1 and M2/M30 are motion/end commands.
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.is_empty() || !words[0].starts_with('M') || !words[0][1..].chars().all(|c| c.is_ascii_digit()) {
        return Err("Only M-code accessory commands are supported".into());
    }
    let code = words[0][1..].parse::<u32>().map_err(|_| "Invalid M code")?;
    if matches!(code, 2 | 3 | 4 | 5 | 30 | 112) {
        return Err("Reserved laser, stop, or program-end M-code".into());
    }
    if words[1..].iter().any(|w| !w.starts_with('S') || w[1..].parse::<u32>().is_err()) {
        return Err("Only optional integer S parameters are supported".into());
    }
    Ok(s)
}
impl Accessory {
    pub fn activation_command(&self) -> Result<String, String> {
        if self.mode == AccessoryMode::Pwm {
            if self.pwm_max == 0 || self.pwm_value > self.pwm_max {
                return Err("PWM duty must be within 0..=PWM maximum".into());
            }
            if !self.pwm_command.contains("{value}") {
                return Err("PWM command must contain {value}".into());
            }
            let duty = if self.active_high { self.pwm_value } else { self.pwm_max - self.pwm_value };
            command(&self.pwm_command.replace("{value}", &duty.to_string()))
        } else if self.active_high {
            command(&self.on_command)
        } else {
            command(&self.off_command)
        }
    }
    pub fn deactivation_command(&self) -> Result<String, String> {
        if self.mode == AccessoryMode::Pwm {
            if self.pwm_max == 0 {
                return Err("PWM maximum must be positive".into());
            }
            let duty = if self.active_high { 0 } else { self.pwm_max };
            command(&self.pwm_command.replace("{value}", &duty.to_string()))
        } else if self.active_high {
            command(&self.off_command)
        } else {
            command(&self.on_command)
        }
    }
}
impl AccessoryConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.outputs.len() > 256 {
            return Err("At most 256 accessories supported".into());
        }
        let mut active = std::collections::HashSet::new();
        for a in self.outputs.iter().filter(|a| a.enabled) {
            if a.name.trim().is_empty() {
                return Err("Accessory name is required".into());
            }
            if a.pre_delay_ms > 30000 || a.post_delay_ms > 30000 {
                return Err("Delay exceeds 30 seconds".into());
            }
            if a.endpoint.transport != AccessoryTransport::Gcode {
                validate_endpoint(a)?;
                continue;
            }
            let on = a.activation_command()?;
            let off = a.deactivation_command()?;
            if on == off {
                return Err("Accessory ON and OFF commands must differ".into());
            }
            // Commands may not be reused across independently controlled devices.
            if !active.insert(on) || !active.insert(off) {
                return Err("Accessory commands conflict across outputs".into());
            }
        }
        // M9 commonly disables both M7 and M8; refuse ambiguous independent output configurations.
        let active_outputs: Vec<_> = self.outputs.iter().filter(|a| a.enabled && a.endpoint.transport == AccessoryTransport::Gcode).collect();
        if active_outputs.len() > 1 && active_outputs.iter().any(|a| a.deactivation_command().ok().as_deref() == Some("M9")) {
            return Err("M9 may disable multiple coolant outputs; configure independent OFF commands".into());
        }
        Ok(())
    }
    pub fn apply(&self, source: &str) -> Result<String, String> {
        self.validate()?;
        let active: Vec<_> = self.outputs.iter().filter(|a| a.enabled && a.endpoint.transport == AccessoryTransport::Gcode).collect();
        if active.is_empty() {
            return Ok(source.to_owned());
        }
        if source.trim().is_empty() {
            return Err("Cannot add accessories to empty G-code".into());
        }
        let commands: Vec<String> = active
            .iter()
            .map(|a| Ok([a.activation_command()?, a.deactivation_command()?]))
            .collect::<Result<Vec<_>, String>>()?
            .into_iter()
            .flatten()
            .collect();
        for line in source.lines() {
            let clean = line.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
            if commands.contains(&clean) {
                return Err(format!("Existing accessory command {clean}; review overlapping control"));
            }
        }
        let mut result = String::from("; LaserCraft auxiliary outputs - check firmware, wiring and output exclusivity\n");
        for a in &active {
            result.push_str(&format!("{}\n", a.activation_command()?));
            if a.pre_delay_ms > 0 {
                result.push_str(&format!("G4 P{:.3}\n", a.pre_delay_ms as f64 / 1000.0));
            }
        }
        let mut ended = false;
        for line in source.lines() {
            let clean = line.split(';').next().unwrap_or("").trim().to_ascii_uppercase();
            if clean == "M2" || clean == "M30" {
                ended = true;
                for a in active.iter().rev() {
                    if a.post_delay_ms > 0 {
                        result.push_str(&format!("G4 P{:.3}\n", a.post_delay_ms as f64 / 1000.0));
                    }
                    result.push_str(&format!("{}\n", a.deactivation_command()?));
                }
            }
            result.push_str(line);
            result.push('\n');
        }
        if !ended {
            for a in active.iter().rev() {
                if a.post_delay_ms > 0 {
                    result.push_str(&format!("G4 P{:.3}\n", a.post_delay_ms as f64 / 1000.0));
                }
                result.push_str(&format!("{}\n", a.deactivation_command()?));
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_outputs_no_change() {
        assert_eq!(AccessoryConfig::default().apply("M2").unwrap(), "M2");
    }
    #[test]
    fn fan_wrapping() {
        let c = AccessoryConfig { outputs: vec![Accessory { enabled: true, ..Default::default() }] };
        let s = c.apply("G1 X1\nM2").unwrap();
        assert!(s.find("M7").unwrap() < s.find("G1").unwrap());
        assert!(s.find("M9").unwrap() < s.find("M2").unwrap());
    }
    #[test]
    fn reject_laser_commands() {
        let c = AccessoryConfig { outputs: vec![Accessory { enabled: true, on_command: "M3 S100".into(), ..Default::default() }] };
        assert!(c.validate().is_err());
    }
    #[test]
    fn reject_duplicate_outputs() {
        let a = Accessory { enabled: true, ..Default::default() };
        let c = AccessoryConfig { outputs: vec![a.clone(), a] };
        assert!(c.validate().is_err());
    }
    #[test]
    fn pwm_requires_explicit_firmware_command() {
        let c = AccessoryConfig { outputs: vec![Accessory { enabled: true, on_command: "M7 S128".into(), ..Default::default() }] };
        assert!(c.apply("M2").unwrap().contains("M7 S128"));
    }
}

#[cfg(test)]
mod phase30_tests {
    use super::*;
    #[test]
    fn supports_256() {
        let c = AccessoryConfig { outputs: vec![Accessory::default(); 256] };
        assert!(c.validate().is_ok());
    }
    #[test]
    fn rejects_257() {
        let c = AccessoryConfig { outputs: vec![Accessory::default(); 257] };
        assert!(c.validate().is_err());
    }
    #[test]
    fn active_low_swaps_commands() {
        let a = Accessory { active_high: false, ..Default::default() };
        assert_eq!(a.activation_command().unwrap(), "M9");
        assert_eq!(a.deactivation_command().unwrap(), "M7");
    }
    #[test]
    fn pwm_polarity() {
        let a = Accessory { mode: AccessoryMode::Pwm, pwm_value: 64, active_high: false, ..Default::default() };
        assert_eq!(a.activation_command().unwrap(), "M7 S191");
        assert_eq!(a.deactivation_command().unwrap(), "M7 S255");
    }
    #[test]
    fn invalid_pwm() {
        let a = Accessory { mode: AccessoryMode::Pwm, pwm_value: 300, ..Default::default() };
        assert!(a.activation_command().is_err());
    }
}

/// Independent outputs never get embedded in laser G-code. Manual commands are explicit;
/// job lifecycle integration requires a coordinated controller with confirmed acknowledgements.
pub fn validate_endpoint(a: &Accessory) -> Result<(), String> {
    let e = &a.endpoint;
    if e.address.trim().is_empty() || e.address.contains(['\n', '\r']) {
        return Err("Independent accessory endpoint is required".into());
    }
    match e.transport {
        AccessoryTransport::Gcode => Ok(()),
        AccessoryTransport::Serial => {
            if e.baud == 0 {
                Err("Serial baud must be positive".into())
            } else {
                Ok(())
            }
        }
        AccessoryTransport::ModbusTcp => {
            if a.mode == AccessoryMode::Pwm {
                Err("Modbus TCP coil supports ON/OFF only; PWM requires a register-capable driver".into())
            } else {
                Ok(())
            }
        }
        AccessoryTransport::Mqtt => {
            if e.mqtt_topic.is_empty() || e.mqtt_topic.contains(['#', '+', '\0']) {
                Err("Valid MQTT publish topic required".into())
            } else {
                Ok(())
            }
        }
    }
}
