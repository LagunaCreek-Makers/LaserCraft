# LaserCraft Phase 8 — GRBL read-only serial monitor

Added `apps/vectorcraft/src/grbl_monitor.rs`, with an asynchronous serial worker, port enumeration, GRBL `?` status polling, status parsing, bounded UI log, alarm display and disconnect. The Laser Operations panel includes port selection and monitor controls.

**Safety scope:** This code does not stream jobs, jog axes, home, reset controllers, or command laser emission. Connecting may still interact with a live controller; test on an isolated machine with laser power physically disabled. Some GRBL-compatible firmware and USB serial adapters behave differently.

**Build:** `cargo test -p vectorcraft --no-default-features` and `cargo run -p vectorcraft`. `serialport` 4.6 is a new dependency with default features disabled to avoid Linux libudev requirements. Rust compilation and hardware testing were not possible in the authoring environment. Lockfile may need updating during first Cargo build.

**Next:** GRBL firmware identification, connection state machine, supervised command queue, and hardware-in-the-loop tests before any movement or job streaming. Ruida remains unimplemented.
