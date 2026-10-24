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

1. ~~Expose document geometry through an adapter to `lasercraft-core`, including curves and transforms.~~ Done in M1 (see below).
2. Add egui laser operation panel to `vectorcraft-ui-egui` and editor command routing.
3. Implement GRBL serial streaming with response handling, stop/hold/resume, disconnect recovery and safety gates.
4. Implement separately tested Ruida protocol with controller-specific profiles and transport.
5. Add raster engraving, image processing, rotary transforms, camera calibration, simulation and material presets.

The original VectorCraft MIT/Apache license and NOTICE files remain included. Upstream dependencies may require network access and Rust >=1.95 for compilation.

## M1: laser jobs from the document

`crates/laser-cam` (`lasercraft-cam`) turns a VectorCraft document into a `lasercraft_core::Job`:
live effects baked, visible printable layers walked bottom first (hidden, template and
non-printing layers, guides and hidden objects left out), curves flattened to a tolerance in mm,
points converted to millimetres and placed from the machine's origin corner (bottom-left,
top-left, top-right or bottom-right of the artboard; each axis runs from that corner into the bed). Holes are cut before the outlines around them, then nearest first. Images, type
(outline it first), gradients and clipping masks are reported as warnings.

Per-layer settings (mode Line, speed mm/min, power %, passes, enabled) live in
`Document.unknown["laserLayers"]`, keyed by layer id, so they save in `.vectorcraft` files and old
files open unchanged.

Machine profiles (the laser panel's `lasercraft/profiles.json`) give the bed size, maximum S and
the origin corner. `origin` is a new, optional profile field (`bottomLeft`, `topLeft`, `topRight`,
`bottomRight`), chosen in the panel's Machine profile section. Profiles saved before it get their
controller's usual corner: top right for Ruida, bottom left for GRBL and the rest.

Engine commands: `laser.exportGcode {bedWidth, bedHeight, origin?, maxS?, path?, selectionOnly?,
tolerance?, artboard?}` (the bed is required: there is no default size), `laser.setLayer {layer,
speed?, power?, passes?, enabled?, mode?, clear?}` and `laser.layers`. Passes repeat the whole
layer. From the command line, `--machine` reads a profile:

```bash
vectorcraft-cli run --in part.vectorcraft --machine "GRBL 300 x 200" --cmd laser.exportGcode --params '{"path": "part.gcode"}'
```

### The laser panel and the document

The laser panel's **Document → laser job** section makes the job from the open document through
`laser.exportGcode`, for the loaded machine profile's bed, maximum S and origin. Until a profile is
loaded (Saved machines → Load machine) it shows the command's "no machine bed size" error instead
of guessing a size. The machine loaded last is loaded again at start (`active_machine` in
`profiles.json`). **Generate from document** sends the job to the dry run and **Export G-code…**
saves it to a file; both carry the machine's air-assist and accessory G-code, which the panel
passes as `laser.exportGcode`'s `prologue` and `epilogue` (the engine never reads profiles and
takes only comments, M-codes other than M3/M4 and G4 dwells there). `vectorcraft-cli run
--machine` passes the profile's the same way. Export lists what was left out (images, type, gradients, clipping) or why
it was refused (art off the bed). **Document layers** lists the document's layers
(`laser.layers`) and sets their speed, power, passes and on/off with `laser.setLayer` (one undoable
step each). The dry run draws the bed with the machine's origin corner. The pasted-SVG section is
kept as legacy: it ignores the origin.

### Fill mode

A layer in **Fill** mode (`laser.setLayer {mode: fill, interval, angle, bidirectional, overscan,
crosshatch}`, or the panel's layer table) engraves the inside of its closed shapes with parallel
scan lines instead of cutting their outlines. Each shape's fill rule (nonzero or even-odd) decides
what is inside, so holes stay empty, and overlapping shapes are burnt once per line. `interval` is
the line spacing (0.01–10 mm), `angle` turns the lines (0° horizontal, 90° vertical),
`bidirectional` runs every other line back, `crosshatch` fills again at right angles, and
`overscan` (0–50 mm) runs the head on past each end of a line at power 0 (S0) so it burns at
speed; overscan is shortened where it would leave the bed, with a warning. The dry run shows the
overscan as travel.
