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

### Image mode

A layer in **Image** mode (`laser.setLayer {mode: image, interval, dither, power, minPower,
negative, bidirectional, overscan}`) engraves the images placed on it line by line; paths on it are
left out (with a warning), as images on Line and Fill layers are. Each image is resampled at
`interval` along its own rows and columns (a turned image is engraved along its turned rows), each
sample the area average of the pixels under it; transparent pixels never burn. `dither` turns grey
levels into power: `floydSteinberg` (default) or `jarvis` error diffusion, `ordered` (4 × 4 Bayer)
or `threshold` burn at full power or not at all; `grayscale` sets each sample's power between
`minPower` and `power` by its darkness. `negative` burns the light parts. Overscan runs at S0 and
stops at the bed's edges. Images are capped at 16 million samples (40 000 along a side) at the
chosen interval: a larger one asks for a larger interval instead of allocating it.

## M2: machine control (GRBL)

### The machine crate

`crates/laser-machine` (`lasercraft-machine`, no workspace dependencies, no I/O of its own) holds
the GRBL 1.1 protocol and job streaming, driven through a `Transport` the caller supplies:

- `grbl`: what a job may hold (`job_lines`: no `$` or real-time commands, lines of at most 70
  ASCII characters), the real-time bytes, replies (`ok`, `error:N`, `ALARM:N`, status reports with
  positions, work offset, feed/spindle, buffer, pins and overrides, `$N=` settings, feedback), the
  meaning of each error and alarm number (in our own words, from GRBL's public interface
  documentation; no GRBL source, which is GPL), the settings LaserCraft checks (`$30`, `$32`,
  `$110/111`, `$120/121`, `$130/131`, `$22`), the jog line, and a streamer that sends line by
  line or by character counting (keeping the 128-byte receive buffer full, never over it).
- `Controller`: one `poll(now)` at a time (no threads, no clock): status every 500 ms, replies
  parsed, job lines sent, errors and alarms stop the job with their meaning, an answer missing for
  15 s or a controller restart fails the job and the link is treated as lost.
- `sim::SimulatedGrbl`: a scripted controller for tests (receive-buffer overflow detection,
  rejected, silent and alarming lines, settings, hold/resume/reset).

The desktop app's serial thread runs the controller over the serial port (line by line, as
before); its old streamer and simulator moved into the crate with their tests.

### Safety gates and the job lifecycle

A job starts only when the controller's last status (no older than 2 s) says Idle, with no alarm,
no open door (state or door input), no feed hold, not asleep or in check mode. When the settings
are known (the desktop app reads `$$` on connecting), laser mode must be on (`$32=1`), the job
must fit the machine's travel (`$130`/`$131`), and a full-power value (`$30`) other than the
profile's maxS is a warning. The job must also stay on the bed the profile gives. **Stop** sends a
feed hold, then a soft reset once the machine has halted (or after 1.5 s): the laser goes off and
the buffers are cleared. **Resume** works only while the machine is held. **Unlock** (`$X`) and
**Home** (`$H`) are refused during a job; the panel enables them only with motion enabled. None
of this replaces a hardware emergency stop or interlocks.

### Job time estimate

`lasercraft_machine::grbl::estimate` plans a job's moves the way a motion controller does: each
move with a trapezoidal speed profile, corner speeds from the junction deviation, and a look-ahead
over the whole job (moves start and end at rest only where they must: reversals, dwells, the
start and end). Rapids run at the maximum rate; overscan, passes and air-assist dwells are in the
G-code, so they count. The limits come from GRBL's `$110/$111`, `$120/$121` and `$11` when read,
else the machine profile's `max_rate` and `acceleration` (set in the panel's Machine profile
section), else 6000 mm/min and 1000 mm/s². `laser.exportGcode` returns
`estimate: {seconds, burnMm, travelMm, moves}` (params `maxRate`, `acceleration`; `vectorcraft-cli
run --machine` passes the profile's), the panel shows it after Generate/Export and in the dry run.
It is an estimate: the controller's buffer and the serial link make the real time somewhat longer.

### Automation and real machines

The control channel's `laser.machine.*` methods reach the laser panel's machine
(`docs/control-protocol.md`); MCP has no machine tools. The rule (from the project owner): **machine motion is panel-only.** On a real machine (a
serial port), automation may only `connect`, `disconnect`, read `status`, `readSettings` (`$$`,
read-only), `hold` and `stop`. `resume`, `jog`, `home`, `unlock` and `start` are refused with
"machine motion is panel-only", and nothing (no param, no setting) turns them on: the desktop app
decides from how the link was opened (the serial port or the built-in simulator), never from the
request. On the simulator (`connect {simulator: true}`) automation may drive everything, so
tests and CI cover the whole job lifecycle.
