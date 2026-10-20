#!/usr/bin/env python3
"""Offline structural checks; not a replacement for cargo check/test."""
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
src = root / "apps/vectorcraft/src"
main = (src / "main.rs").read_text()
modules = ("macro_routines", "macro_canvas", "macro_debugger", "macro_trace", "macro_source_map")
errors = []
for module in modules:
    if not (src / f"{module}.rs").is_file():
        errors.append(f"Missing {module}.rs")
    if f"mod {module};" not in main:
        errors.append(f"Missing main.rs module declaration: {module}")
panel = (src / "laser_panel.rs").read_text()
if "checked_sub(1).and_then(|i| trace.get(i))" not in panel:
    errors.append("Debugger cursor 0 may incorrectly highlight the first step")
debugger = (src / "macro_debugger.rs").read_text()
if "self.cursor.checked_sub(1)" not in debugger:
    errors.append("Variable watch may show future state at cursor 0")
if "cargo test -p vectorcraft --locked" not in (root / ".github/workflows/lasercraft-macro-check.yml").read_text():
    errors.append("Missing Cargo CI regression command")
if "editor.highlighted_path = None" not in panel:
    errors.append("Debugger highlights must be cleared before each preview")
if "self.debugger.context().and_then(" not in panel:
    errors.append("Trace preview must not silently replace invalid inputs with defaults")
if errors:
    for error in errors: print("FAIL:", error, file=sys.stderr)
    sys.exit(1)
print("PASS: module wiring, debugger reset guards, and CI command (static only)")
