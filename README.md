# keel

A health probe for the wintermute brain's tier ladder — it tells you which model tier the brain can actually reach right now, without spending a token to find out.

## Why it exists

The wintermute brain runs on a ladder of model tiers, local-3b at the bottom up through opus at the top, and falls down a rung when a tier is unreachable or out of budget. The hard question at any moment is "what rung are we standing on?" The naive way to answer it — send a completion and see what happens — costs money and confuses billing with reachability. `keel` answers it for free: it probes the non-generating endpoints (`GET /v1/models`) and infers the ceiling from reachability and auth alone. A completion request is never sent.

## Install

```
cargo install --path .
```

Requires Rust ≥ 1.85 (edition 2024). Tests run offline — `cargo test` touches no sockets.

## Commands

Three subcommands read the same ladder; they differ in what they report.

### `keel pulse` — the full table

```
$ keel pulse
TIER            STATUS          CHECKED      FAILS
-------------------------------------------------------
local-3b        reachable       1717..       0
local-8b        skipped         0            0
haiku           keyless         1717..       0
sonnet          keyless         1717..       0
opus            keyless         1717..       0

$ keel pulse --format json
[ { "tier": "local-3b", "status": { "status": "reachable" }, ... }, ... ]
```

Exit code is **0** when the top configured tier is `Reachable`, non-zero otherwise — so a hook or self-review script can gate on it directly.

### `keel status` — the one-liner

Answers "what tier is the brain on, and for how long?"

```
$ keel status
floored: local-3b for 4d 6h (cloud keyless since 2026-05-30)

$ keel status        # when the full ladder is up
nominal: opus reachable
```

With `--state-file <path>` (or `WM_KEEL_STATE_FILE`), `status` reads the persisted ceiling to report how long the current rung has held. Exit code is 0 when nominal, non-zero when floored. JSON via `--format json`.

### `keel beacon` — emit a change event

Diffs the current effective ceiling against the last one written to the state file and, when it moves, publishes `wm.keel.degraded` or `wm.keel.refloat` to agorabus over its Unix socket (default `~/.cache/agorabus/sock`). Run it on a timer to get notified when the brain drops a rung or recovers one. Requires `--state-file` or `WM_KEEL_STATE_FILE`.

## The ladder

The default ladder, lowest to highest: `local-3b`, `local-8b` (both Ollama on `localhost:11434`), then `haiku`, `sonnet`, `opus` (Anthropic API). Local tiers probe `GET /v1/models`; cloud tiers short-circuit to `Keyless` when `WM_ANTHROPIC_KEY` is absent, since an unkeyed cloud tier is unreachable in practice.

| Variable | Default | Effect |
|---|---|---|
| `WM_ANTHROPIC_KEY` | unset | Empty → cloud tiers report `Keyless` |
| `WM_BRAIN_SKIP_TIERS` | none | Comma-separated tier names to drop from the ladder |
| `WM_BRAIN_MAX_TIER` | none | Highest tier to include; everything above is excluded |
| `WM_KEEL_STATE_FILE` | `~/.local/share/keel/last-ceiling.json` | Where `status`/`beacon` persist the last ceiling |

## Type surface

`keel` is also the foundational crate of its fleet: it owns the shared types, and sibling crates (`keel-ledger`, `keel-cordon`, `keel-beacon`) extend them rather than redefine them. The names below are the contract those siblings depend on.

- **`TierStatus`** — `Reachable | Unreachable { reason } | Keyless | Exhausted | Unconfigured | Skipped`.
- **`TierHealth`** — `{ tier, status, checked_at: i64, consecutive_failures: u32 }`.
- **`LedgerEntry`** — `{ tier, tokens_in, tokens_out, est_cost_usd, ts }`. Schema owned here; written and read by `keel-ledger`. `keel pulse` neither persists nor queries it.
- **`Ladder` / `TierConfig`** — the resolved rungs after skip/max filtering. Every subcommand agrees on the ladder through this type.

Two traits make the probe testable without sockets:

- **`TierProbe`** — `HttpProbe` (real, `ureq`) and `FakeProbe` (test double). The probe never calls `/v1/chat/completions`.
- **`ProbeEnv`** — `SystemEnv` (reads real env vars) and `FakeProbeEnv` (test fixture).

## Where it fits

`keel` is the health-and-types floor for the keel fleet of wintermute brain crates. `keel-ledger` records token spend against the `LedgerEntry` schema declared here; `keel-beacon`/`keel-cordon` consume the same tier types.

## License

MIT OR Apache-2.0
