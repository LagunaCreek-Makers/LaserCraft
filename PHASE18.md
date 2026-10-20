# Phase 18: raster scanline generator and improved offline preview

- Added `raster_to_gcode` for grayscale and thresholded 8-bit pixels, bidirectional scanlines, and laser-off travel between rows. The raster generator is a library API, not yet wired to image import.
- Added stop-at-each-segment acceleration-aware **estimate**. Not a GRBL planner or junction look-ahead model.
- Added adjustable exposure resolution (40–200 columns) and grayscale/amber overlay to the native panel.
- Corrected a pre-existing syntax typo in G-code modal parser (`3=>motion=3,,20`).
- Added three unit tests.

## Verify
`cargo test -p vectorcraft phase18_tests`
`cargo check -p vectorcraft`

This archive was not compiled; hardware use is NOT validated.
