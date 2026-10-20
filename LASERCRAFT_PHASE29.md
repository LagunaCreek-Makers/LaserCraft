# Phase 29: configurable auxiliary M-code outputs

Adds a machine-profile accessory list and offline G-code wrappers for up to eight non-safety-critical outputs. Each enabled output has a name, ON/OFF single-line M-code, optional integer S argument, and pre/post delays. Settings persist in `profiles.json` under each machine's `accessories` field. The generator wraps SVG, fill and calibration G-code with accessory commands and turns outputs off before M2/M30.

## Important limits
- **Fire suppression must be independent of G-code** and operated by dedicated certified equipment and sensors. Do not connect CO2 extinguishers to M8 or any laser job code; a frozen/disconnected controller can fail to fire.
- M3/M4/M5 are reserved for laser/spindle power and are rejected as accessories.
- M7/M8/M9 are controller/firmware-dependent. Standard GRBL does **not** provide generic PWM fan control via `M7 S...`; only configure such syntax for firmware known to implement it. Some controllers share M9 for multiple outputs.
- The accessory list is applied to the whole job, not per layer. No Modbus, GPIO, HTTP, Ruida or direct control implementation.
- No physical laser test or successful Rust build was performed.
