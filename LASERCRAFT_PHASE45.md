# LaserCraft Phase 45 — Offline conditional macro debugger

Adds conditional breakpoints on simulated PWM/device activation, elapsed simulated time and WAIT actions; a device-state watch; and resets action-index breakpoints when the expanded plan changes. All actions remain simulated. No hardware execution is enabled.

Tests are provided in `macro_debugger.rs` but have not been run. Rust/Cargo were not available in the build environment. Before using this software on real equipment, compile, run tests, inspect editor integration, and separately validate controller behavior.

Note: Breakpoints are indexed by the expanded plan, not source-block identifiers; variable and conditional expression source-level stepping remains future work.
