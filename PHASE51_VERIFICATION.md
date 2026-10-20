# Phase 51: debugger consistency and verification

This phase fixes two offline debugger correctness issues:

- Source block highlights are cleared before each trace update, so Reset, a changed routine, or an invalid preview cannot leave a stale highlighted block.
- Traced preview now uses the validated input context rather than silently falling back to default values on invalid JSON.
- Variable-change searches at the end of a trace explicitly return `None`.

## Checks performed in this environment

- `python3 tools/lasercraft_static_check.py`: PASS
- `python3 -m unittest discover -s tools/tests -v`: 4 tests PASS
- ZIP integrity: PASS

## Not verified

`cargo` and `rustc` are not installed in this environment, so compilation, Rust unit tests, and UI runtime checks have not been performed. The Python structural checks do not prove Rust correctness. Hardware macro execution remains disabled.

## Required CI verification

Run `cargo test -p vectorcraft --locked` in a Rust-capable environment, inspect failures, and fix them before declaring a release candidate.
