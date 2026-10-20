# LaserCraft Phase 25 — Build Verification Infrastructure

This phase adds a GitHub Actions build/test workflow and dependency-free Python CLI regression tests for the checkerboard calibration helper. It does **not** add automatic ChArUco detection.

## Local verification in this environment
- `python3 -m unittest discover -s tools/tests -v`: PASS (4 tests)
- `python3 -m compileall -q tools/lasercraft_calibrate.py`: PASS
- `cargo check`, `cargo test`, native executable build: NOT RUN (Rust toolchain unavailable)

## CI verification
Push this source to GitHub, open Actions > LaserCraft build and tests > Run workflow. The workflow attempts Rust 1.95.0 compilation and tests, then uploads a Linux desktop binary **only on success**. A CI failure is evidence of integration issues to fix, not a completed build.

## Remaining gaps
- Verify native desktop compilation, resolve compiler errors, and test with actual webcam and laser controllers.
- Automatic ChArUco detection, multiple-camera calibration, and hardware safety validation are not implemented.
