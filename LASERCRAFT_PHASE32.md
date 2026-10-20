# Phase 32 — Independent accessory job lifecycle (experimental)

A non-blocking coordinator starts enabled independent serial/Modbus TCP/MQTT accessories in order before queueing GRBL G-code. It attempts rollback on startup failure. On cancel, disconnect, reported job failure, alarm, or job acknowledged AND GRBL Idle, it attempts reverse-order shutdown. Delays are per accessory. A status line and fault acknowledgement appear in the streaming panel.

**Limitations:** Commands and MQTT QoS 1 acknowledgements do not prove physical output states. Network drop, UI crash, power loss, or stalled firmware can leave outputs energized. No per-layer switching or persistent reconciliation. If a cancellation races with startup, the startup thread can still energize devices; do not use for safety-critical loads. Never connect fire suppression, E-stop, interlocks or other life-safety systems to this software. Confirm hardware OFF manually after faults. Hardware fail-safe defaults are essential.

**Verification:** Source only; Rust compilation and physical tests have not been performed. Build with `cargo test -p vectorcraft` and `cargo check -p vectorcraft` after installing toolchain and system dependencies.
