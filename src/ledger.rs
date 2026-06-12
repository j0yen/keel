//! Append-only spend ledger for cloud tier usage.
//!
//! Every cloud call records its tokens and estimated cost as one NDJSON line.
//! The file is **append-only** — never truncated or rewritten — mirroring the
//! recall/gossip append discipline.
//!
//! # Price table
//!
//! Prices from Anthropic public pricing page, 2026-06-06.
//! Input/output prices in USD per million tokens.
//!
//! | Tier      | Input $/Mtok | Output $/Mtok |
//! |-----------|-------------|--------------|
//! | haiku     | 0.80        | 4.00         |
//! | sonnet    | 3.00        | 15.00        |
//! | opus      | 15.00       | 75.00        |
//! | local-*   | 0.00        | 0.00         |
//!
//! Unknown tiers: `est_cost_usd` is stored as `null` (never panics).

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Price table (2026-06-06)
// ---------------------------------------------------------------------------

/// Per-tier pricing entry, in USD per **million** tokens.
#[derive(Debug, Clone, Copy)]
pub struct TierPrice {
    /// Input price per million tokens (USD).
    pub input_per_mtok: f64,
    /// Output price per million tokens (USD).
    pub output_per_mtok: f64,
}

/// Static price table. Tiers not listed here → cost = `None`.
///
/// Source: Anthropic public pricing page, 2026-06-06.
pub const PRICE_TABLE: &[(&str, TierPrice)] = &[
    (
        "haiku",
        TierPrice {
            input_per_mtok: 0.80,
            output_per_mtok: 4.00,
        },
    ),
    (
        "sonnet",
        TierPrice {
            input_per_mtok: 3.00,
            output_per_mtok: 15.00,
        },
    ),
    (
        "opus",
        TierPrice {
            input_per_mtok: 15.00,
            output_per_mtok: 75.00,
        },
    ),
];

/// Compute the estimated cost (USD) for a known tier.
///
/// Returns `None` for tiers not in `PRICE_TABLE` or for local tiers (prefix
/// `"local-"`). Never panics.
#[must_use]
pub fn compute_cost(tier: &str, tokens_in: u64, tokens_out: u64) -> Option<f64> {
    // Local tiers are free; cost is Some(0.0) rather than None.
    if tier.starts_with("local-") || tier == "local" {
        return Some(0.0);
    }

    PRICE_TABLE
        .iter()
        .find(|(name, _)| *name == tier)
        .map(|(_, price)| {
            #[allow(
                clippy::float_arithmetic,
                clippy::cast_precision_loss,
                clippy::as_conversions
            )]
            {
                let cost_in = (tokens_in as f64) * price.input_per_mtok / 1_000_000.0;
                let cost_out = (tokens_out as f64) * price.output_per_mtok / 1_000_000.0;
                cost_in + cost_out
            }
        })
}

// ---------------------------------------------------------------------------
// NDJSON record (what is actually stored on disk)
// ---------------------------------------------------------------------------

/// One NDJSON line in the ledger file.
///
/// Distinct from `types::LedgerEntry` (which is the shared type) so that
/// `est_cost_usd` can be `null` for unknown tiers without changing the
/// shared type's `f64` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRecord {
    /// The tier that generated this usage (e.g. `"haiku"`, `"local-3b"`).
    pub tier: String,
    /// Input tokens consumed.
    pub tokens_in: u64,
    /// Output tokens generated.
    pub tokens_out: u64,
    /// Estimated cost in USD; `None` for unknown/unpriced tiers.
    pub est_cost_usd: Option<f64>,
    /// Unix timestamp (seconds) of the turn, injected by the caller.
    pub ts: i64,
}

// ---------------------------------------------------------------------------
// LedgerStore trait
// ---------------------------------------------------------------------------

/// Abstraction over the ledger file path so tests can inject a fixture dir.
pub trait LedgerStore: Send + Sync {
    /// Path to the NDJSON spend file.
    fn spend_path(&self) -> PathBuf;
    /// Path to the JSON status-override file.
    fn status_path(&self) -> PathBuf;
}

/// Default store that writes to `~/.local/state/keel/`.
#[derive(Debug, Default)]
pub struct DefaultStore;

impl LedgerStore for DefaultStore {
    fn spend_path(&self) -> PathBuf {
        dirs_home()
            .join(".local")
            .join("state")
            .join("keel")
            .join("spend.ndjson")
    }

    fn status_path(&self) -> PathBuf {
        dirs_home()
            .join(".local")
            .join("state")
            .join("keel")
            .join("status.json")
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from)
}

/// A store backed by a caller-supplied directory (for tests).
#[derive(Debug)]
pub struct FixtureStore {
    /// Directory under which `spend.ndjson` and `status.json` are created.
    pub dir: PathBuf,
}

impl FixtureStore {
    /// Create a fixture store rooted at `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl LedgerStore for FixtureStore {
    fn spend_path(&self) -> PathBuf {
        self.dir.join("spend.ndjson")
    }

    fn status_path(&self) -> PathBuf {
        self.dir.join("status.json")
    }
}

// ---------------------------------------------------------------------------
// record() — append one entry
// ---------------------------------------------------------------------------

/// Append one token-usage record to the ledger.
///
/// - Creates the parent directory if it does not exist.
/// - Appends one NDJSON line; never truncates.
/// - Cost is computed from the price table; unknown tier → `None`.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or the entry cannot be
/// serialized or written.
pub fn record(
    store: &dyn LedgerStore,
    tier: &str,
    tokens_in: u64,
    tokens_out: u64,
    now: i64,
) -> Result<(), String> {
    let path = store.spend_path();
    ensure_parent(&path)?;

    let entry = LedgerRecord {
        tier: tier.to_owned(),
        tokens_in,
        tokens_out,
        est_cost_usd: compute_cost(tier, tokens_in, tokens_out),
        ts: now,
    };

    let line = serde_json::to_string(&entry).map_err(|e| format!("serialize ledger entry: {e}"))?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open ledger {}: {e}", path.display()))?;

    writeln!(file, "{line}").map_err(|e| format!("write ledger {}: {e}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// read_records() — load all entries
// ---------------------------------------------------------------------------

/// Load all records from the ledger file.
///
/// Lines that fail to parse are skipped (with the error ignored) so a
/// corrupted line does not abort a spend aggregation.
///
/// # Errors
///
/// Returns an error if the file cannot be opened (missing file → empty vec,
/// not an error).
pub fn read_records(store: &dyn LedgerStore) -> Result<Vec<LedgerRecord>, String> {
    let path = store.spend_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file =
        std::fs::File::open(&path).map_err(|e| format!("open ledger {}: {e}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("read ledger line: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(r) = serde_json::from_str::<LedgerRecord>(&line) {
            records.push(r);
        }
    }
    Ok(records)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create ledger dir {}: {e}", parent.display()))?;
    }
    Ok(())
}
