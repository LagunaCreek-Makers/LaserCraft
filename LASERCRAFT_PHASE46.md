# Phase 46: Offline source provenance and variable watches

Added `RoutineDatabase::traced_preview` to expand IF/ELSE, REPEAT, CALL, and variable assignments while retaining source block paths, call stacks, and variable snapshots for every emitted action. The debugger displays the selected source block and a variable watch. Two Rust unit tests were added.

Limitations: offline only; variable watches show state at the most recent emitted action, not at standalone variable assignments; source paths are textual, not yet a graphical highlight overlay. Native Rust compilation and tests have not been run in this environment. No accessory outputs are triggered.
