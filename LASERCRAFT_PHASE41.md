# Phase 41 — Routine canvas editing history and clipboard

- Per-routine Undo and Redo with a maximum of 100 undo snapshots.
- Copy a block, then Paste at end of routine; nested blocks copy recursively.
- Existing drag/drop, duplication, deletion, nested IF/ELSE and REPEAT preserved.
- History is transient editor state; the routine database remains the persisted source of truth.
- Added unit tests for undo/redo branch invalidation and nested clipboard copying.

## Limitations
- Search box is present as a draft input, but filtering/highlighting is not yet wired up.
- Copy/paste currently pastes to the end of the routine only, not arbitrary nested insertion.
- This is an offline editor. No live routine execution or safety-control functions.
- Rust compilation and tests are not verified in this environment.
