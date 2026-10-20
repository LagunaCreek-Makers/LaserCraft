# Phase 13 — passive Ruida network inventory

This phase extends the UDP listener with an IP-based peer inventory, timestamped packet records, bounded capture (500 packets), packet-length/prefix fingerprints, CSV clipboard export, and loopback tests. It intentionally does not transmit discovery probes or interpret proprietary bytes as machine state. A UDP sender is not proof of a Ruida controller. The listener does not promiscuously capture packets addressed to other hosts: to observe traffic, direct permitted test traffic to the listener or use an authorized network capture setup.

## Verification

`cargo test -p vectorcraft ruida_monitor`
`cargo check -p vectorcraft`

Do not enable laser output or motion based on these diagnostics. Active discovery, controller identification, status decoding, and job transfer require model-specific documented or independently verified protocol fixtures and hardware testing.
