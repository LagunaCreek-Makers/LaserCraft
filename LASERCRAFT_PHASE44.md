# Phase 44: offline macro debugger

Adds a selectable-routine debugger with simulated integer inputs, step/continue/reset, action-index breakpoints, elapsed WAIT time, and simulated accessory output state inspection. Uses the existing bounded routine interpreter. Does not execute real accessory commands. Conditional and variable evaluation happens in the existing preview interpreter; individual variable mutations are not yet exposed in debugger snapshots. No native compilation has been performed.
