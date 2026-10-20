# LaserCraft Phase 5 — offline geometry and fill foundation

Adds cubic and quadratic Bézier polyline sampling, circle/ellipse approximation, rectangular bidirectional scanline fill generation, and simple job statistics. The native Laser Operations panel exposes a diagnostic fill job.

## Limitations
- No native hardware streaming, Ruida driver, or raster engraving.
- SVG arcs, smooth path commands, transforms, compound paths, viewBox scaling, units other than implicit mm, and per-object color mapping are not handled.
- Cubic and quadratic subdivision is based on approximate control-polygon length (not a certified maximum chord-error tolerance).
- Fill works for a rectangle diagnostic only, not arbitrary polygons or holes.
- Statistics exclude acceleration, dwell, and machine-specific behavior.
- Review G-code and verify machine setup; no unattended operation.

## Verify
`cargo test -p lasercraft-core && cargo check -p vectorcraft`

Compilation was not available in the authoring environment. Changes are source-only and unverified.
