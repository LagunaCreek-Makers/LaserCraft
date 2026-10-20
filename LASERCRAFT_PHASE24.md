# Phase 24 — Offline automatic chessboard camera intrinsics

Capture at least 8 sharp images of a printed, flat chessboard at different tilts and positions **without changing focus, zoom, or resolution**. For a board with 10×7 squares, use `--columns 9 --rows 6` (INNER corners). Measure square width in millimeters.

```bash
python3 -m pip install opencv-python numpy
python3 tools/lasercraft_calibrate.py 'calibration/*.jpg' --columns 9 --rows 6 --square-mm 20 --output lens.json
```

The helper detects chessboard corners with OpenCV, fits camera intrinsics/distortion, reports per-view reprojection errors, and writes normalized Brown–Conrady parameters. In LaserCraft's Camera Placement panel, load a camera image of the same resolution, then choose **Import automatic checkerboard calibration**. Recalibrate the bed corners after importing lens correction and use independent checkpoints to assess actual placement accuracy. Import is rejected for invalid files, resolution mismatches, RMS > 1 px, or significant unsupported k3 distortion.

Limitations: only chessboard targets (not ChArUco), offline stills, no live automatic detection, no multi-camera stitching, no physical accuracy certification. The helper depends on an external Python/OpenCV installation. No machine motion or laser firing is performed. This workspace has not been compiled or tested against hardware.
