# LaserCraft Phase 50 — Debugger reset correctness and static preflight

- Fix cursor-zero variable watch: before the first simulated operation, no future variable value is displayed.
- Fix cursor-zero canvas highlight: no block is highlighted before stepping.
- Add `tools/lasercraft_static_check.py` to check module wiring and reset guards without Rust.
- Add static preflight to GitHub Actions before Cargo compilation and tests.

## Verification limits

Static checks are **not** Rust compilation, type checking, UI testing or hardware verification.
Run `python3 tools/lasercraft_static_check.py`, then `cargo test -p vectorcraft --locked` on a machine with Rust 1.95+ and dependencies available.
