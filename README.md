# keel

Brain tier-ladder health probe and shared types for the wintermute keel fleet.

## What this does

`keel pulse` prints a one-glance health table for the configured brain tier ladder,
answering "can the brain even reach this tier?" without dispatching a completion request
or incurring billing. It probes reachability and auth using non-generating endpoints
(`/v1/models`) only.

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
[
  { "tier": "local-3b", "status": { "status": "reachable" }, ... },
  ...
]
```

Exit code: **0** if the top configured tier is `Reachable`, **non-zero** otherwise.
This lets hooks and self-review scripts gate on it directly.

## Type surface contract

These types are declared in the foundational `keel` crate. Sibling crates
(`keel-ledger`, `keel-cordon`, `keel-beacon`) extend them without redefining.

### `TierStatus`

```rust
pub enum TierStatus {
    Reachable,
    Unreachable { reason: String },
    Keyless,
    Exhausted,
    Unconfigured,
    Skipped,
}
```

### `TierHealth`

```rust
pub struct TierHealth {
    pub tier: String,
    pub status: TierStatus,
    pub checked_at: i64,          // Unix seconds
    pub consecutive_failures: u32,
}
```

### `LedgerEntry`

Declared here (schema owner); written and read by `keel-ledger`. `keel pulse`
does not persist or query ledger entries.

```rust
pub struct LedgerEntry {
    pub tier: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub est_cost_usd: f64,
    pub ts: i64,
}
```

### `Ladder`

The resolved list of configured tiers after applying `WM_BRAIN_SKIP_TIERS`
and `WM_BRAIN_MAX_TIER`. All keel subcommands agree on the rungs via this type.

```rust
pub struct Ladder {
    pub tiers: Vec<TierConfig>,
}
```

### `TierConfig`

```rust
pub struct TierConfig {
    pub name: String,
    pub kind: TierKind,    // Local | Cloud
    pub endpoint: String,
}
```

## Traits

### `TierProbe`

```rust
pub trait TierProbe: Send + Sync {
    fn probe(&self, tier: &TierConfig, env: &dyn ProbeEnv) -> TierHealth;
}
```

Implementations: `HttpProbe` (real, uses `ureq`), `FakeProbe` (test double — no sockets).

The probe **never** calls `/v1/chat/completions`. Local tiers use `GET /v1/models`.
Cloud tiers short-circuit to `Keyless` when `WM_ANTHROPIC_KEY` is absent.

### `ProbeEnv`

```rust
pub trait ProbeEnv: Send + Sync {
    fn anthropic_key(&self) -> Option<String>;
    fn skip_tiers(&self) -> Vec<String>;
    fn max_tier(&self) -> Option<String>;
}
```

Implementations: `SystemEnv` (reads real env vars), `FakeProbeEnv` (test fixture).

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `WM_ANTHROPIC_KEY` | (unset) | Anthropic API key; empty → cloud tiers report `Keyless` |
| `WM_BRAIN_SKIP_TIERS` | (none) | Comma-separated tier names to exclude from the ladder |
| `WM_BRAIN_MAX_TIER` | (none) | Highest tier to include; tiers above this are `Unconfigured` |

## Building

```
cargo build --release
```

Requires Rust ≥ 1.85. No network access needed for tests (`cargo test` is offline-safe).

## License

MIT OR Apache-2.0
