# LaserCraft Phase 22: experimental lens correction

Adds `apps/vectorcraft/src/camera_lens.rs` and extends the camera placement panel with manually entered Brown–Conrady radial/tangential distortion parameters. The original four-corner mapping remains the default. When enabled, clicked image corners are undistorted before solving the planar homography; generated toolpaths are projected back through the lens model for display. Settings are saved in the existing camera calibration JSON.

## Limitations
- No checkerboard or ArUco detection, no automatic intrinsic calibration, no multi-camera stitching, and no native continuous video stream.
- Values must be obtained from real calibration measurements. Default coefficients perform no correction.
- Calibration applies to one camera resolution, mounting position, focal configuration, and workpiece plane. Verify accuracy at interior reference points before using the overlay.
- Offline visual placement only; this module does not issue machine motion or laser commands.
- The Rust workspace was not compiled or hardware-tested in this environment.

## Suggested checks
`cargo test -p vectorcraft camera_lens`
`cargo check -p vectorcraft`
`cargo run -p vectorcraft`
