# Phase 17: arc interpolation and relative exposure overlay

* G2/G3 arcs with I/J relative center offsets are tessellated to at most 0.25 mm chord-length sampling (maximum 20,000 segments). R-format arcs and unsupported coordinate planes are not handled.
* The dry-run panel overlays an accumulated, relative laser-on path exposure heatmap, computed to the current playback time. This is **not** a physical simulation of material darkening, kerf, temperature, or delivered optical energy.
* The simulation remains offline: no machine communication.
* Nominal time ignores acceleration, jerk, planner buffers, laser power scaling, and controller-specific semantics.
* Tests added for arcs and exposure. Cargo was not available in the authoring environment; compilation remains unverified.

Build: `cargo test -p vectorcraft laser_sim && cargo check -p vectorcraft`.
