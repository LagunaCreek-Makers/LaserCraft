# Phase 38: Event-bound routine templates (offline)

- Adds reusable running and completed status templates to the nested visual routine editor.
- Adds editable event-to-routine bindings, enabled flags, and preview of matching events.
- Preserves structured JSON text editor and machine-profile persistence.
- Validation rejects unsupported events and references to missing routines.
- Removing a routine also removes its event bindings.
- Preview executes no hardware actions. No drag-and-drop canvas or live serialized execution queue is implemented.
- Tests added but not run: no Cargo/Rust toolchain available.
- Never connect this automation to CO2 flooding, fire suppression, emergency stop, or other life-safety equipment.
