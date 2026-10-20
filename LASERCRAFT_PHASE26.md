# Phase 26: Laser calibration test patterns

Offline G-code test generator: power/speed, interval, passes, grayscale power ramp, minimum-power mark, and kerf squares. Calibration records are associated with the selected machine and material and saved to the existing local profiles.json. The selected layer settings are recorded as preferred settings; saving a result does not automatically change machine or material profiles.

IMPORTANT: Source not compiled or hardware tested. M4 assumes compatible GRBL dynamic laser mode; check firmware configuration and coordinate system before use. Tests are not valid measurements until physical results are recorded. Grayscale test uses stepped cell power, not photographic grayscale. Kerf test produces squares but does not calculate kerf from measurements. Test patterns intentionally contain no automatic machine commands.

Run `cargo test -p vectorcraft laser_tests` and `cargo check -p vectorcraft`.
