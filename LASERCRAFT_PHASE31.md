# Phase 31: Independent accessory endpoints

Each of up to 256 configured accessories may now select Laser G-code, separate serial/COM, Modbus TCP (FC05 coil), or MQTT (via installed `mosquitto_pub`). Profile settings persist in the existing machine profile JSON. The accessory editor has explicit manual ON/OFF controls for independent endpoints. **Independent commands are not embedded into laser G-code and are not automatically dispatched at job boundaries.** This avoids presenting an unimplemented cross-controller synchronization feature as reliable.

Serial endpoints send a configured single-line M-code over a separate port; their firmware must understand that command. Modbus TCP writes a zero-based coil and checks its acknowledgement; PWM via Modbus is not implemented. MQTT supports ON/OFF payloads and PWM payload templates, requires an installed `mosquitto_pub`, and currently supports plain MQTT without credentials/TLS. Manual send success means only that the command was transmitted/acknowledged by the protocol, not that physical hardware state was independently verified.

**Safety:** Not for CO2 extinguishers, fire suppression, E-stop, or interlocks. No guaranteed shutdown on process crash, network loss, or machine alarm. Use independent fail-safe hardware for those functions.

Build and hardware tests have not been executed.
