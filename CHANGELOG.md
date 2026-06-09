# Changelog

All notable changes to this project will be documented in this file.

## v0.1.0 (2026-06-08)

### keel-beacon (PRD-keel-beacon)

Makes the brain tier ceiling legible in the moment: `keel status` one-liner
answers "what tier is the brain standing on, and for how long?"; agorabus
`wm.keel.*` events are emitted when the effective tier ceiling changes
(drop → `wm.keel.degraded`, rise → `wm.keel.refloat`, no-change → nothing),
so peon-ping, a future self-heal loop, or the operator can react live instead
of rediscovering the same floored-tier fact at the next daily self-review.

- `keel status` — one-line floored/nominal report with duration; `--format json`
- `keel beacon` — ceiling-change detector; edge-triggered, not a heartbeat
- `RecordingBeacon` test double — zero live-bus connections in `cargo test`
- SIGPIPE reset so `keel status | head` never panics

### keel-pulse (PRD-keel-pulse)

Core types, `TierProbe`/`ProbeEnv` traits, `pulse` subcommand: the floored-tier
line that keel-beacon reads.
