//! Proptest invariants for keel types.
//!
//! READ-ONLY: the edit-agent must not modify this file.

use keel::types::{LedgerEntry, TierHealth, TierStatus};
use proptest::prelude::*;

fn arb_tier_status() -> impl Strategy<Value = TierStatus> {
    prop_oneof![
        Just(TierStatus::Reachable),
        any::<String>().prop_map(|r| TierStatus::Unreachable { reason: r }),
        Just(TierStatus::Keyless),
        Just(TierStatus::Exhausted),
        Just(TierStatus::Unconfigured),
        Just(TierStatus::Skipped),
    ]
}

proptest! {
    #[test]
    fn tier_health_serde_roundtrip(
        tier in "[a-z][a-z0-9-]{0,14}",
        checked_at in any::<i64>(),
        consecutive_failures in any::<u32>(),
        status in arb_tier_status(),
    ) {
        let h = TierHealth { tier, status, checked_at, consecutive_failures };
        let json = serde_json::to_string(&h).unwrap();
        let back: TierHealth = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(h, back);
    }

    #[test]
    fn ledger_entry_serde_roundtrip(
        tier in "[a-z][a-z0-9-]{0,14}",
        tokens_in in any::<u64>(),
        tokens_out in any::<u64>(),
        ts in any::<i64>(),
    ) {
        // est_cost_usd: avoid NaN/Inf which JSON can't encode
        let e = LedgerEntry {
            tier,
            tokens_in,
            tokens_out,
            est_cost_usd: 0.0,
            ts,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(e, back);
    }
}
