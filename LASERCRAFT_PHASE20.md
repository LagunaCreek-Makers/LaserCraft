# Phase 20: projective camera alignment

Adds a pure Rust 4-point planar homography mapping bed coordinates to normalized camera image coordinates, plus calibration JSON save/load and artwork X/Y offset controls. No hardware movement is initiated. Calibration profiles are bound to image resolution and machine bed size. Unit tests cover perspective mapping and degenerate geometry.

## Limitations

- This is still a still-image capture interface, not continuous live video.
- Lens distortion, parallax, Z height, camera movement, and focus are **not corrected**. A homography is only valid for one flat plane.
- Camera pixel clicks do not yet move artwork; use numeric X/Y offsets.
- No calibrated accuracy certificate, physical validation, or controller movement integration.
- Build/test with `cargo test -p vectorcraft camera_calibration` and `cargo check -p vectorcraft`.
