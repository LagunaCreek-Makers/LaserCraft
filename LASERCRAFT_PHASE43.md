# Phase 43 — Offline routine execution trace

Adds `apps/vectorcraft/src/macro_trace.rs` with a deterministic, non-actuating timeline for the existing bounded routine interpreter. The Laser Panel displays the first routine's ordered output actions, cumulative explicit WAIT time, and total output-action count. Timeline output is capped to 300 visible lines, while the existing interpreter enforces a maximum expanded preview length. Includes unit tests for timestamps, action counts and missing routines.

## Limitations

No Rust compiler is available in the authoring environment. Compilation and runtime tests have not been run. The trace is an **offline preview**, not a hardware debugger; it does not account for serial/network latency, actual output state, or device acknowledgement. Live macro execution remains disabled. No changes to safety-critical controls.

## Suggested validation

Run `cargo test -p vectorcraft --bin vectorcraft macro_trace` (verify the package/binary names in Cargo.toml first), then `cargo check -p vectorcraft`. Run GUI interaction tests before relying on the editor.
