# LaserCraft Phase 2: Native Laser Operations panel

Added `apps/vectorcraft/src/laser_panel.rs` and integrated it into the desktop eframe app. The floating egui window is displayed by default and can be closed with its X (reopening currently requires restarting the app). This is a source-level integration, not yet integrated into VectorCraft's dock registry.

## Features
- Multiple operation layers with enabled toggles, labels and color indicators
- Line, Fill, Fill + Line, Image operation selectors (settings only; CAM for fill/image not implemented)
- Per-layer speed, power, passes, line interval
- Machine bed size and GRBL S-max settings
- Offline GRBL diagnostic square generation using `lasercraft-core`, with bounds checking and copy-to-clipboard

## Limitations and safety
- No live controller communication, no Ruida encoder, no artwork-to-toolpath conversion, no actual fill/raster toolpaths.
- Diagnostic G-code is **not** generated from document artwork. It includes laser power commands; do not send to hardware without review and appropriate safety precautions.
- Rust toolchain is unavailable in this environment, so source is uncompiled and untested. Run `cargo check -p vectorcraft` and `cargo test -p lasercraft-core` locally.
- Future: register native panel in `crates/ui-egui/src/state.rs` and `dock.rs`, persist layer data, link SVG/document shapes and color mapping to operations.
