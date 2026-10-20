# Phase 39: Drag-and-drop macro canvas

Adds `apps/vectorcraft/src/macro_canvas.rs` with an egui drag handle, draggable palette, insertion lanes, nested IF THEN/ELSE and REPEAT drop targets, and block reordering. The routine database, JSON text editor, event bindings, and offline preview remain unchanged.

To use: expand the Phase 39 routine library, create/open a routine, drag a palette label into a horizontal insertion lane or drag an existing block by its handle. Blue lines mark targets. Edit the parameters inline. Use the existing Save routines to machine action to persist changes.

Limitations: native Rust build and UI tests are unverified. This is a mouse/pointer drag interface, not Blockly; there is no pan/zoom, touch support guarantee, or execution against live hardware. Safety-critical actuators must not be driven by these routines.
