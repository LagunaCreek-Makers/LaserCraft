# LaserCraft Phase 12 — Ruida UDP passive diagnostics

This phase introduces `apps/vectorcraft/src/ruida_monitor.rs` and a Ruida UDP diagnostics panel.

**Implemented:** receive-only UDP listener bound to a user-selected local port (default 50200), source address, byte count, first 32 bytes in hex, bounded display log, start/stop, bind errors, and a basic socket unit test.

**Not implemented:** Ruida discovery, authentication, handshake, protocol encoding/decoding, model detection, status queries, job uploads, laser control, machine safety integration. No packets are transmitted. The listener is not proof of Ruida connectivity.

Use only on a trusted isolated machine network. UDP payload prefixes may contain sensitive information; avoid sharing logs without inspection. A local firewall may block inbound packets. Do not expose the listener to the public Internet.

Build and test on a Rust 1.95+ machine with dependencies available:

```sh
cargo test -p vectorcraft ruida_monitor
cargo check -p vectorcraft
cargo run -p vectorcraft
```

This environment did not have Cargo or rustc, so compilation and runtime tests were not performed. The next step is to collect documented controller-model-specific protocol fixtures and implement read-only discovery/status queries with test vectors before any write operations.
