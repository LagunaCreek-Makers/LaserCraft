//! Accessory outputs: the configuration and its G-code live in `lasercraft_cam::hooks::accessories`
//! (shared with laser.exportGcode's callers); sending to independent transports (serial, Modbus
//! TCP, MQTT) is here, in the app.
pub use lasercraft_cam::hooks::accessories::*;

/// Sends a manual state change on a transport independent of the laser controller.
/// Not intended for emergency stops, extinguishers, or other life-safety equipment.
pub fn send_independent(a: &Accessory, activate: bool) -> Result<(), String> {
    validate_endpoint(a)?;
    let e = &a.endpoint;
    match e.transport {
        AccessoryTransport::Gcode => Err("G-code accessories must use the laser job sender".into()),
        AccessoryTransport::Serial => {
            use std::io::Write;
            let mut port =
                serialport::new(&e.address, e.baud).timeout(std::time::Duration::from_secs(2)).open().map_err(|x| format!("Serial open: {x}"))?;
            let cmd = if activate { a.activation_command()? } else { a.deactivation_command()? };
            port.write_all(format!("{cmd}\n").as_bytes()).map_err(|x| format!("Serial write: {x}"))
        }
        AccessoryTransport::ModbusTcp => {
            use std::io::{Read, Write};
            use std::net::{TcpStream, ToSocketAddrs};
            let addr = if e.address.contains(':') { e.address.clone() } else { format!("{}:502", e.address) };
            let socket = addr.to_socket_addrs().map_err(|x| x.to_string())?.next().ok_or("No Modbus address")?;
            let mut stream = TcpStream::connect_timeout(&socket, std::time::Duration::from_secs(2)).map_err(|x| x.to_string())?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).map_err(|x| x.to_string())?;
            let high = if activate { a.active_high } else { !a.active_high };
            let val = if high { 0xFF00u16 } else { 0u16 };
            let mut packet = [0u8; 12];
            packet[4..6].copy_from_slice(&6u16.to_be_bytes());
            packet[6] = e.unit_id;
            packet[7] = 5;
            packet[8..10].copy_from_slice(&e.channel.to_be_bytes());
            packet[10..12].copy_from_slice(&val.to_be_bytes());
            stream.write_all(&packet).map_err(|x| x.to_string())?;
            let mut response = [0u8; 12];
            stream.read_exact(&mut response).map_err(|x| x.to_string())?;
            if response != packet {
                return Err("Modbus coil acknowledgement mismatch".into());
            }
            Ok(())
        }
        AccessoryTransport::Mqtt => {
            // Explicitly use an installed MQTT client rather than implementing insecure partial MQTT.
            // No shell is invoked, so topic and payload cannot inject commands.
            let payload = if a.mode == AccessoryMode::Pwm {
                if a.pwm_max == 0 || a.pwm_value > a.pwm_max {
                    return Err("Invalid PWM range".into());
                }
                let v = if activate {
                    if a.active_high { a.pwm_value } else { a.pwm_max - a.pwm_value }
                } else if a.active_high {
                    0
                } else {
                    a.pwm_max
                };
                e.mqtt_pwm_template.replace("{value}", &v.to_string())
            } else if activate {
                e.mqtt_on.clone()
            } else {
                e.mqtt_off.clone()
            };
            let (host, port) = e.address.rsplit_once(':').unwrap_or((&e.address, "1883"));
            let status = std::process::Command::new("mosquitto_pub")
                .args(["-h", host, "-p", port, "-t", &e.mqtt_topic, "-m", &payload, "-q", "1"])
                .status()
                .map_err(|x| format!("mosquitto_pub unavailable: {x}"))?;
            if status.success() { Ok(()) } else { Err(format!("MQTT publish failed: {status}")) }
        }
    }
}

#[cfg(test)]
mod phase31_tests {
    use super::*;
    #[test]
    fn independent_does_not_enter_gcode() {
        let mut a = Accessory { enabled: true, ..Default::default() };
        a.endpoint.transport = AccessoryTransport::Mqtt;
        a.endpoint.address = "localhost:1883".into();
        a.endpoint.mqtt_topic = "laser/fan".into();
        let c = AccessoryConfig { outputs: vec![a] };
        assert_eq!(c.apply("M2").unwrap(), "M2");
    }
    #[test]
    fn modbus_pwm_rejected() {
        let mut a = Accessory { enabled: true, ..Default::default() };
        a.mode = AccessoryMode::Pwm;
        a.endpoint.transport = AccessoryTransport::ModbusTcp;
        a.endpoint.address = "localhost:502".into();
        assert!(AccessoryConfig { outputs: vec![a] }.validate().is_err());
    }
}
