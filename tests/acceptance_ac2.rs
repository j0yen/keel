//! AC2: TierStatus, TierHealth, LedgerEntry, Ladder, TierConfig are public
//! and serde-(de)serializable; a round-trip test covers each.

use keel::types::{Ladder, LedgerEntry, TierConfig, TierHealth, TierKind, TierStatus};

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    v: &T,
) {
    let json = serde_json::to_string(v).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*v, back, "round-trip failed for {json}");
}

#[test]
fn tier_status_roundtrip() {
    roundtrip(&TierStatus::Reachable);
    roundtrip(&TierStatus::Unreachable {
        reason: "connection refused".to_string(),
    });
    roundtrip(&TierStatus::Keyless);
    roundtrip(&TierStatus::Exhausted);
    roundtrip(&TierStatus::Unconfigured);
    roundtrip(&TierStatus::Skipped);
}

#[test]
fn tier_health_roundtrip() {
    let h = TierHealth {
        tier: "haiku".to_string(),
        status: TierStatus::Reachable,
        checked_at: 1_700_000_000,
        consecutive_failures: 3,
    };
    roundtrip(&h);
}

#[test]
fn ledger_entry_roundtrip() {
    let e = LedgerEntry {
        tier: "opus".to_string(),
        tokens_in: 100,
        tokens_out: 200,
        est_cost_usd: 0.003,
        ts: 1_700_000_001,
    };
    roundtrip(&e);
}

#[test]
fn tier_config_roundtrip() {
    let c = TierConfig {
        name: "local-3b".to_string(),
        kind: TierKind::Local,
        endpoint: "http://localhost:11434".to_string(),
    };
    roundtrip(&c);
}

#[test]
fn ladder_roundtrip() {
    let ladder = Ladder {
        tiers: vec![
            TierConfig {
                name: "local-3b".to_string(),
                kind: TierKind::Local,
                endpoint: "http://localhost:11434".to_string(),
            },
            TierConfig {
                name: "haiku".to_string(),
                kind: TierKind::Cloud,
                endpoint: "https://api.anthropic.com".to_string(),
            },
        ],
    };
    roundtrip(&ladder);
}
