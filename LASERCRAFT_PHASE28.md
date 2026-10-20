# Phase 28: Air-assist G-code control

Machine profile: `air_assist` object with `enabled`, `on_command`, `off_command`, `default_on`, `preflow_ms`, `postflow_ms`. Material profile: `air_assist` as `Inherit`, `On`, or `Off`; same setting on each layer. Defaults are disabled (no G-code changes). GRBL default commands M8/M9 are **not universally mapped to a physical output**. User must verify firmware and hardware.

For offline test patterns, SVG conversion and fill toolpaths, the postprocessor injects an ON command and optional preflow delay before the job, and OFF before M2/M30 or at EOF. OFF-mode inserts an OFF command before the job. It refuses files with matching explicit commands to avoid conflicting controls. Delays use G4 P seconds, which may differ on other firmware.

No active Ruida, Modbus, GPIO, MQTT, HTTP or network control is implemented. No guarantee of air shutdown on USB disconnect, E-stop, alarm or process crash. Use normally-closed fail-safe hardware, independent interlocks, and validate OFF behavior. Multicolor SVG jobs currently use the selected layer's air setting for the whole job, not per-color switching. No compile or hardware test performed.
