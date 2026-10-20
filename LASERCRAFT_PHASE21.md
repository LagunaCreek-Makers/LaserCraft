# LaserCraft Phase 21: live camera snapshots and interactive placement

## Implemented
- Asynchronous FFmpeg USB camera frame acquisition, with configurable 250–3000 ms interval. Each frame is an independent capture; it is not low-latency streaming video.
- Camera image refresh without blocking the egui UI thread while waiting for FFmpeg.
- Calibration invalidation if incoming camera resolution changes.
- Dragging over the calibrated image updates artwork X/Y placement using the inverse planar homography.
- Unit test for inverse homography round-trip.
- Retains four-corner projective calibration, saved JSON profiles, and G-code path overlay.

## Deliberately not implemented
- Automatic checkerboard/ArUco detection, optical distortion correction, multi-camera stitching, hardware firing or motion from camera clicks, and verified alignment precision.
- Device enumeration and low-latency native webcam streaming. Requires an installed FFmpeg executable.

## Build and verify
```
cargo test -p vectorcraft camera_calibration
cargo check -p vectorcraft
cargo run -p vectorcraft
```
These commands were not executed in the build environment (Cargo unavailable). The implementation is uncompiled and unverified. Test with a stationary camera and an unpowered laser first.

## Calibration safety
The camera-to-bed transform is valid only for a fixed camera, bed, image resolution, and workpiece plane. Distortion, parallax, and workpiece thickness are not modeled. Independently measure errors at multiple bed locations before using any alignment for machining. A physical emergency stop and interlocks remain necessary.
