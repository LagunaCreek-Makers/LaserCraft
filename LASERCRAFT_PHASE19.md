# Phase 19: USB camera workpiece placement preview

- Native egui Camera Placement window launched from the Camera Tools button.
- Import PNG/JPEG/WebP camera stills; optional single-frame capture via installed `ffmpeg` from USB webcam.
- Four-click mapping: bed bottom-left, bottom-right, top-right, top-left. Bed coordinates follow the active LaserCraft machine profile.
- Offline G-code overlay: laser-on (red) and travel (blue) on the camera photograph, with adjustable opacity.
- No hardware motion or laser firing is performed by this feature.

## Limits

The four-corner mapping uses bilinear interpolation, **not** lens calibration or a full projective homography. Camera perspective, barrel distortion, lens height, material thickness and camera movement can introduce alignment errors. No automatic fiducial detection, live video, multi-camera stitching, or accurate geometric correction is included. USB capture requires an independently installed ffmpeg executable and device permissions. All source changes are uncompiled and require Rust validation.

## Test

`cargo check -p vectorcraft && cargo test -p vectorcraft`

Verify camera images using an offline G-code square before any physical framing. Test with laser disabled; validate machine interlocks and alignment separately.
