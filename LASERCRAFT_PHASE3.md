# LaserCraft Phase 3: offline SVG CAM integration

Added `lasercraft_core::svg_cam::svg_to_job` and a native Laser Operations panel section for SVG input, conversion, boundary validation and G-code copying.

## Supported
- SVG `<rect>`, `<line>`, `<polyline>`, `<polygon>`, and straight `M/L/H/V/Z` path segments.
- Multiple subpaths and pass repetition.
- GRBL offline G-code generation with bed boundary validation.
- Errors on unsupported curve segments, transforms, circles and ellipses rather than silently generating incomplete paths.

## Not yet supported
- Direct live VectorCraft document extraction: use VectorCraft's SVG export and paste SVG markup into the panel.
- Proper SVG unit/viewBox mapping, nested transformations, Bézier flattening, stroke-to-path conversion, fills, raster, Ruida, device connections.
- Production-ready controller safety and machine calibration.

## Test
`cargo test -p lasercraft-core`
`cargo check -p vectorcraft`
`cargo run -p vectorcraft`

This environment did not have Cargo installed. Source has not been compiled or hardware-tested. Never send generated output to a connected machine without independent review and physical safety controls.
