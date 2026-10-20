# LaserCraft Phase 6 — Offline preview and SVG stroke-layer assignment

New `lasercraft-core::layer_cam::colored_svg_to_job` associates exact SVG `stroke="#rrggbb"` colors with enabled operation layers, applying per-layer speed, power and passes. The Laser Operations panel has a checkbox for this mode and an offline preview of travel and cut moves. Unmatched colors are skipped. CSS styles/classes, SVG transforms, fills, and nested SVG complexities are **not supported**; unsupported styles fail closed. This is not a complete SVG rendering engine. Layer color controls remain fixed defaults and are not yet persisted.

The preview is an approximate 2D drawing, not an acceleration-aware time simulation or hardware validation. No GRBL/Ruida communication has been added. The original VectorCraft document is not automatically re-imported in this phase: the panel still accepts pasted SVG.

Validation commands (not executed here because Cargo is unavailable):

```bash
cargo test -p lasercraft-core
cargo check -p vectorcraft
cargo run -p vectorcraft
```

Do not send generated G-code to an unattended machine. Verify coordinate conventions, laser enable mode, limits, and interlocks before physical testing.
