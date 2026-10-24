//! Phase 13: passive Ruida network inventory and bounded packet diagnostics.
//! No discovery probes, status requests, job data, or motion commands are sent.
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct Datagram {
    pub source: SocketAddr,
    pub received_unix_ms: u128,
    pub bytes: usize,
    pub prefix_hex: String,
    #[allow(dead_code, reason = "captured for export; only the prefix is shown so far")]
    pub payload_hex: String,
    pub fingerprint: String,
}
impl Datagram {
    fn from_bytes(source: SocketAddr, bytes: &[u8]) -> Self {
        let hex = |data: &[u8]| data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
        // Diagnostic fingerprint only; no undocumented bytes are interpreted as controller state.
        let fingerprint = format!("len={} head={}", bytes.len(), hex(&bytes[..bytes.len().min(4)]));
        Self {
            source,
            received_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            bytes: bytes.len(),
            prefix_hex: hex(&bytes[..bytes.len().min(32)]),
            payload_hex: hex(&bytes[..bytes.len().min(256)]),
            fingerprint,
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Peer {
    pub packets: u64,
    pub bytes: u64,
    pub last_seen_unix_ms: u128,
    pub last_fingerprint: String,
}
#[derive(Debug, Default)]
pub struct Inventory {
    pub peers: BTreeMap<IpAddr, Peer>,
    pub captured: Vec<Datagram>,
    pub dropped: u64,
}
impl Inventory {
    pub fn record(&mut self, packet: Datagram) {
        let peer = self.peers.entry(packet.source.ip()).or_default();
        peer.packets += 1;
        peer.bytes += packet.bytes as u64;
        peer.last_seen_unix_ms = packet.received_unix_ms;
        peer.last_fingerprint = packet.fingerprint.clone();
        if self.captured.len() >= 500 {
            self.captured.remove(0);
            self.dropped += 1;
        }
        self.captured.push(packet);
    }
    pub fn csv(&self) -> String {
        let mut s = String::from("unix_ms,source,bytes,fingerprint,prefix_hex\n");
        for p in &self.captured {
            s.push_str(&format!("{},{},{},\"{}\",\"{}\"\n", p.received_unix_ms, p.source, p.bytes, p.fingerprint, p.prefix_hex));
        }
        s
    }
}
#[derive(Debug)]
pub enum Event {
    Listening(u16),
    Packet(Datagram),
    Error(String),
    Stopped,
}
pub struct Listener {
    pub events: Receiver<Event>,
    stop: Sender<()>,
}
impl Listener {
    pub fn start(port: u16) -> Self {
        let (tx, events) = mpsc::channel();
        let (stop, rx) = mpsc::channel();
        thread::spawn(move || {
            let socket = match UdpSocket::bind(("0.0.0.0", port)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("UDP bind failed: {e}")));
                    return;
                }
            };
            if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(250))) {
                let _ = tx.send(Event::Error(e.to_string()));
                return;
            }
            let actual_port = socket.local_addr().map(|a| a.port()).unwrap_or(port);
            let _ = tx.send(Event::Listening(actual_port));
            let mut buf = [0u8; 65535];
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                match socket.recv_from(&mut buf) {
                    Ok((n, source)) => {
                        let _ = tx.send(Event::Packet(Datagram::from_bytes(source, &buf[..n])));
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                    Err(e) => {
                        let _ = tx.send(Event::Error(e.to_string()));
                        break;
                    }
                }
            }
            let _ = tx.send(Event::Stopped);
        });
        Self { events, stop }
    }
    pub fn stop(&self) {
        let _ = self.stop.send(());
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_receives_local_datagram() {
        let listener = Listener::start(0);
        let port = match listener.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Listening(p) => p,
            other => panic!("unexpected {other:?}"),
        };
        assert_ne!(port, 0);
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(&[0xAA, 0x01, 0x02], ("127.0.0.1", port)).unwrap();
        match listener.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Packet(p) => {
                assert_eq!(p.bytes, 3);
                assert_eq!(p.prefix_hex, "AA 01 02");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    #[test]
    fn inventory_bounded_and_csv() {
        let addr = "127.0.0.1:50200".parse().unwrap();
        let mut inv = Inventory::default();
        for _ in 0..503 {
            inv.record(Datagram::from_bytes(addr, &[1, 2]));
        }
        assert_eq!(inv.captured.len(), 500);
        assert_eq!(inv.dropped, 3);
        assert_eq!(inv.peers.len(), 1);
        assert_eq!(inv.peers[&addr.ip()].packets, 503);
        assert!(inv.csv().contains("127.0.0.1:50200"));
    }
}
