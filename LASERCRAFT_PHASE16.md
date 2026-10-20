# LaserCraft Phase 16: animated offline G-code dry run

Adds time-based playback, selectable G-code, nominal tool-head interpolation, zoom, pan, playback speed, and visible completed-vs-pending segments to the existing egui panel. All operations are offline and do not send controller commands.

## Testing

```sh
cargo test -p vectorcraft laser_sim
cargo check -p vectorcraft
cargo run -p vectorcraft
```

## Limitations

The existing Phase 15 parser supports G0/G1, not arcs or machine-specific extensions. Playback timing uses nominal feed rate and ignores acceleration, buffering, dwell, rotary kinematics, and laser power ramping. Preview is not a machine safety check. This package has not been compiled or tested on hardware.
