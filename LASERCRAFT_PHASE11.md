# Phase 11 — Offline GRBL simulator and streaming safeguards

Adds `grbl_simulator.rs` with deterministic tests for accepted lines, injected controller errors, dropped acknowledgements, hold and resume. Enhances `grbl_stream.rs` with an in-flight acknowledgement timeout (15 seconds in serial transport). Blocks firmware queries and jogging while a job is active.

**Important limitations:** This is a protocol-level simulator, not a physical motion simulator. Timeout may occur during long operations or on slow firmware; no automatic retry is attempted because that could duplicate a laser command. Existing `CancelJob` requests feed hold only, not a guaranteed laser-off emergency stop. Live control is unverified and must not be used on a powered laser until reviewed and tested on a disconnected/laser-disabled controller. A full motion-completion state machine and interlock feedback are still required.

Build/test: `cargo test -p vectorcraft` and `cargo check -p vectorcraft`.
