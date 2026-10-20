# LaserCraft Phase 33 — Experimental event macros

- Adds serialized per-machine event macro rules (up to 256 rules, 32 actions per rule).
- Rules select independently connected accessory outputs (serial, MQTT, Modbus TCP), ON/OFF or PWM percent.
- UI allows editing rules and saving them to a machine profile.
- JobStarted/JobCompleted/JobCancelled/JobFault event hooks are wired to coordinator transitions.
- LayerStarted and InputHigh/InputLow are schema/editor placeholders, **not live triggers**: hardware input polling and layer boundaries are not yet integrated.
- Macro dispatch runs in a background thread, but acknowledgement and physical feedback are **not** tracked; no guaranteed ordering relative to shutdown coordinator. Not suitable for unattended operation.
- CO2 work-area flooding, fire suppression, oxygen displacement, safety interlocks and emergency stops must not be automated by these macros. Use independently certified and interlocked safety equipment. CO2 flooding in occupied/enclosed areas can cause fatal asphyxiation.
- No Rust toolchain is installed in this environment; source was not compiled or executed.
