# LaserCraft Phase 14 — Offline Ruida protocol inspection

Adds `apps/vectorcraft/src/ruida_protocol.rs`, a deliberately **non-authoritative** packet inspector and tests, and a new collapsible inspector in the native Laser Operations panel.

## Scope
- Parse manually pasted hexadecimal datagrams (no network transmission).
- Display packet length, first bytes, trailing bytes, additive byte sum and XOR **as descriptive statistics**, not as claimed Ruida checksums.
- Group packet lengths and first-byte frequencies in a reusable in-memory analysis structure.
- Preserve Phase 13 passive UDP inventory unchanged.

## Not implemented
Active Ruida discovery, verified model identification, checksum verification, response decoding, Ethernet connection, file encoding/upload, laser control. No Ruida packet structure is claimed without controller-specific evidence and fixtures.

## Verification
`cargo test -p vectorcraft ruida_protocol` and `cargo check -p vectorcraft` (requires Rust toolchain). Do not connect a powered laser to this unverified development build.
