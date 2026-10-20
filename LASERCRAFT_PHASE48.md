# Phase 48 — Nested source mapping and offline build readiness

- Added `macro_source_map.rs` translating trace paths (`then`, `else`, `repeat[n]`) into the block-canvas path convention.
- Checks that the resolved source block exists before highlighting it, including nested loop iterations.
- Traced steps in called routines highlight their own routine canvas rather than the caller.
- Added Rust tests for nested mapping, malformed provenance and nonexistent branches.
- Existing debugger remains offline; no live hardware macro execution is enabled.

## Build prerequisites

Rust 1.95+ (workspace edition 2024), Cargo, Linux desktop dependencies, and network access to the pinned craft-libs Git dependencies.

```sh
cargo fmt --all -- --check
cargo test -p vectorcraft --bin vectorcraft macro_source_map
cargo check -p vectorcraft
```

The above commands have NOT been executed in this environment (Rust/Cargo unavailable). The package is source-only and not a verified executable.
