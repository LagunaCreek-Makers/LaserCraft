# Phase 35: dual macro editors

The visual block editor and JSON text editor share the same MacroConfig. Load blocks into text explicitly; validate and apply text atomically. Saving the selected machine profile persists applied changes. Invalid JSON or invalid device mappings do not mutate live rules. Text may be stale after block edits; reload deliberately. This is a JSON structured editor, not a free-form scripting engine. Do not use macros for safety-critical control. Rust build not verified.
