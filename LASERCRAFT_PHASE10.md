# Phase 10 — GRBL job streaming

Experimental one-line-per-acknowledgement sender, progress, error stop, cancellation via feed hold, and explicit UI arming. No hardware validation or Rust compilation performed. `ok` acknowledges receipt, not physical motion completion. Disconnect does not guarantee a stopped laser. Controller resets and safety interlocks must be physical. Do not use unattended.

Run `cargo test -p vectorcraft` and `cargo check -p vectorcraft` before hardware tests.
