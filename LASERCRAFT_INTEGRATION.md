# LaserCraft integration — phase 1

This is the **user-supplied VectorCraft source tree**, augmented in-place with:

- `crates/lasercraft-core`: validated controller-independent operations and offline GRBL G-code exporter.
- `apps/lasercraft-cli`: demonstration command-line application exercising the core from the same Cargo workspace.

The root `Cargo.toml` already includes `crates/*` and `apps/*`, so the new packages are automatically workspace members. Existing VectorCraft code is otherwise unchanged. This is not yet a renamed desktop editor or hardware controller.

## Example

```bash
cargo test -p lasercraft-core
cargo run -p lasercraft-cli -- rectangle 30 20 1200 25 300 200 > rectangle.gcode
```

**Do not run the generated G-code on a real laser without reviewing it and validating controller settings, coordinate systems, limits, interlocks and power calibration.** The exporter assumes GRBL laser mode `$32=1` and `S` range 0–1000, which must be confirmed on the actual device. Ruida is not implemented.

## Engineering follow-up

1. Expose document geometry through an adapter to `lasercraft-core`, including curves and transforms.
2. Add egui laser operation panel to `vectorcraft-ui-egui` and editor command routing.
3. Implement GRBL serial streaming with response handling, stop/hold/resume, disconnect recovery and safety gates.
4. Implement separately tested Ruida protocol with controller-specific profiles and transport.
5. Add raster engraving, image processing, rotary transforms, camera calibration, simulation and material presets.

The original VectorCraft MIT/Apache license and NOTICE files remain included. Upstream dependencies may require network access and Rust >=1.95 for compilation.
