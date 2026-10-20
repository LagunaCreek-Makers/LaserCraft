# LaserCraft Phase 7 — Persistent Profiles (source only)

Adds a machine/material configuration section to the existing Laser Operations panel.

- Machine profiles: controller family, bed dimensions, GRBL S maximum, connection endpoint and baud.
- Material presets: speed, power, passes and line interval; apply to current layer.
- Saved in a versioned JSON file in the user configuration directory.
- Uses a temporary file and rename for saving.
- GRBL/Ruida hardware connections are **not implemented**. Never send exported G-code to hardware without independent review.

Build: `cargo check -p vectorcraft`; `cargo test -p lasercraft-core`; `cargo run -p vectorcraft`.

**Not compiled or run in this environment (Cargo unavailable).**

Next: install Rust CI and fix build errors, validate controller profiles, then implement read-only GRBL discovery/status before any motion/firing support.
