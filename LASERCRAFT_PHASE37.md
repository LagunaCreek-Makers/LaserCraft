# Phase 37: Nested visual routine editor

The visual routine editor now edits the full existing bounded AST rather than showing a flat read-only debug listing. It supports nested IF/ELSE, AND/OR/NOT conditions, named input and variable equality comparisons, REPEAT loops, WAIT durations, device ON/OFF/PWM, variables and named routine calls. Blocks can be moved up/down or deleted. Both editors operate on the same `RoutineDatabase` structure; the JSON editor uses explicit import/export to avoid overwriting uncommitted text.

## Boundaries

This is a form-based recursive block editor, **not drag-and-drop Blockly**. The routine interpreter is offline preview only. Live input subscriptions, hardware dispatch, wall-clock scheduling, cancellation and fault recovery remain unimplemented. The WAIT block is a planned delay and does not schedule an actual timer. User-programmable macros must not control fire suppression, CO2 flooding, or other life-safety systems.

## Verification

No Cargo/Rust compiler was available here. The new tests have not been run. Run `cargo test -p vectorcraft` and `cargo check -p vectorcraft` in a configured Rust build environment and resolve any compiler or integration issues before use.
