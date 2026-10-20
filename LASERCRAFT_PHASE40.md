# LaserCraft Phase 40 — Canvas editing reliability

- Add Duplicate and Delete controls to every block, including nested blocks.
- Make drag/drop edits transactional: invalid paths or full lanes leave original AST unchanged.
- Reject unknown palette indices.
- Add unit tests for nested duplicate/delete and failed-move preservation.

No live hardware automation added. Rust compilation and tests have not been run in this environment.
