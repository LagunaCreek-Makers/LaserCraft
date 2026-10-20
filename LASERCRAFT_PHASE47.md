# LaserCraft Phase 47 — Offline source highlighting and variable-change breakpoints

- The macro canvas highlights the source block of the currently selected traced operation when the trace path maps directly to an editor block.
- The debugger can advance to the next *simulated* variable change in a traced routine.
- No physical device commands are executed by these debugger features.
- Rust compilation and UI interaction have not been verified in this environment.
- Known limitation: trace paths with iteration/call metadata cannot always be mapped to a visual block; highlights can lag by one UI frame because the canvas appears earlier in the panel.
