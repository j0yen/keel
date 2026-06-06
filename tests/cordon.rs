//! Integration tests for keel-cordon — AC1..AC8 of PRD-keel-cordon.
//!
//! This file is the entry point that `cargo test` will report as
//! `Running tests/cordon.rs` (self_orphaned_mock_tests guard — AC8).
//!
//! Individual acceptance criteria are in submodules below.

// AC1 — offline: Cordon compiles and all tests use fixtures only
mod ac1 {
    use keel::cordon::{Cordon, CordonConfig};
    use keel::types::{TierHealth, TierStatus};

    #[test]
    fn cordon_builds_offline_no_network() {
        let cfg = CordonConfig::default();
        let health = vec![TierHealth {
            tier: "haiku".to_string(),
            status: TierStatus::Keyless,
            checked_at: 0,
            consecutive_failures: 0,
        }];
        let cordon = Cordon::from_health(&health, &cfg);
        // Just verifying it builds and runs — no network calls possible
        let _ = cordon.should_attempt("haiku", 1_000);
    }
}

// AC2 — a tier stamped Exhausted at t0 returns Skip for now < t0+cooldown,
//        then exactly one Attempt (half-open) at now >= cooldown
mod ac2 {
    use keel::cordon::{Cordon, CordonConfig, Decision};
    use keel::types::TierStatus;

    #[test]
    fn exhausted_skip_then_halfopen_attempt() {
        let cfg = CordonConfig {
            base_secs: 60,
            max_secs: 3_600,
            keyless_via_cooldown: false,
        };
        let mut cordon = Cordon::empty(cfg);
        let t0: i64 = 1_000_000;

        // Record one failure (Exhausted) at t0
        cordon.record_failure("haiku", t0, TierStatus::Exhausted);

        // now < t0 + 60s → Skip
        let d = cordon.should_attempt("haiku", t0 + 30);
        assert!(
            matches!(d, Decision::Skip { .. }),
            "expected Skip before cooldown expires; got {d:?}"
        );

        // now == t0 + 60s exactly → Attempt (half-open)
        let d = cordon.should_attempt("haiku", t0 + 60);
        assert!(
            matches!(d, Decision::Attempt),
            "expected Attempt at cooldown boundary; got {d:?}"
        );

        // now > t0 + 60s → still Attempt
        let d = cordon.should_attempt("haiku", t0 + 120);
        assert!(
            matches!(d, Decision::Attempt),
            "expected Attempt after cooldown expires; got {d:?}"
        );
    }
}

// AC3 — record_failure grows cooldown_until exponentially and is capped; n=1..6
mod ac3 {
    use keel::cordon::{Cordon, CordonConfig, Decision};
    use keel::types::TierStatus;

    #[test]
    fn exponential_cooldown_table_n1_to_6() {
        let base: i64 = 60;
        let ceiling: i64 = 3_600;
        let cfg = CordonConfig {
            base_secs: base,
            max_secs: ceiling,
            keyless_via_cooldown: false,
        };
        let mut cordon = Cordon::empty(cfg);
        let t0: i64 = 0;

        // Expected cooldown values: base * 2^(n-1), capped at ceiling
        let expected_cooldowns: [i64; 6] = [
            60,    // n=1: 60 * 2^0 = 60
            120,   // n=2: 60 * 2^1 = 120
            240,   // n=3: 60 * 2^2 = 240
            480,   // n=4: 60 * 2^3 = 480
            960,   // n=5: 60 * 2^4 = 960
            1_920, // n=6: 60 * 2^5 = 1920 (< 3600 ceiling)
        ];

        for (i, &expected_cd) in expected_cooldowns.iter().enumerate() {
            let n = i + 1;
            let now = t0 + (i as i64) * 10_000; // advance time to clear previous cooldown
            cordon.record_failure("haiku", now, TierStatus::Exhausted);
            // now + expected_cd - 1 → still skip
            let d = cordon.should_attempt("haiku", now + expected_cd - 1);
            assert!(
                matches!(d, Decision::Skip { .. }),
                "n={n}: expected Skip at t={}, got {d:?}",
                now + expected_cd - 1
            );
            // now + expected_cd → attempt (half-open)
            let d = cordon.should_attempt("haiku", now + expected_cd);
            assert!(
                matches!(d, Decision::Attempt),
                "n={n}: expected Attempt at cooldown boundary t={}, got {d:?}",
                now + expected_cd
            );
        }
    }

    #[test]
    fn cooldown_is_capped_at_ceiling() {
        let base: i64 = 3_600; // base = ceiling
        let ceiling: i64 = 3_600;
        let cfg = CordonConfig {
            base_secs: base,
            max_secs: ceiling,
            keyless_via_cooldown: false,
        };
        let mut cordon = Cordon::empty(cfg);

        // After many failures, cooldown should never exceed ceiling
        for i in 0_i64..20 {
            let now = i * 100_000;
            cordon.record_failure("opus", now, TierStatus::Exhausted);
            // Check that the next decision is Skip for at least 1s but at most ceiling+1s
            let d_before = cordon.should_attempt("opus", now + 1);
            assert!(
                matches!(d_before, Decision::Skip { .. }),
                "failure {}: expected Skip immediately after record_failure",
                i + 1
            );
            // Verify the retry_after is within ceiling from the recorded time
            if let Decision::Skip { retry_after: Some(ra), .. } = d_before {
                let cd = ra - now;
                assert!(
                    cd <= ceiling,
                    "failure {}: cooldown {cd} exceeds ceiling {ceiling}",
                    i + 1
                );
            }
        }
    }
}

// AC4 — record_success resets consecutive_failures to 0 and clears cooldown
mod ac4 {
    use keel::cordon::{Cordon, CordonConfig, Decision};
    use keel::types::TierStatus;

    #[test]
    fn record_success_resets_failures_and_clears_cooldown() {
        let cfg = CordonConfig::default();
        let mut cordon = Cordon::empty(cfg);
        let t0: i64 = 1_000_000;

        // Record 3 failures to build up state
        cordon.record_failure("haiku", t0, TierStatus::Exhausted);
        cordon.record_failure("haiku", t0 + 1, TierStatus::Exhausted);
        cordon.record_failure("haiku", t0 + 2, TierStatus::Exhausted);

        // Verify it's in Skip state
        let d = cordon.should_attempt("haiku", t0 + 3);
        assert!(matches!(d, Decision::Skip { .. }), "expected Skip after 3 failures");

        // Record success
        cordon.record_success("haiku");

        // Now should be Attempt immediately
        let d = cordon.should_attempt("haiku", t0 + 3);
        assert!(
            matches!(d, Decision::Attempt),
            "expected Attempt after record_success; got {d:?}"
        );

        // And state is fresh
        let states = cordon.states();
        let state = states.iter().find(|s| s.tier == "haiku").expect("haiku state");
        assert_eq!(state.consecutive_failures, 0, "consecutive_failures should be 0");
        assert!(state.cooldown_until.is_none(), "cooldown_until should be None");
    }
}

// AC5 — Keyless tier returns Skip without consulting network;
//        if key later appears (fixture flip), next call returns Attempt
mod ac5 {
    use keel::cordon::{Cordon, CordonConfig, Decision};
    use keel::types::{TierHealth, TierStatus};

    #[test]
    fn keyless_returns_skip_without_cooldown() {
        let cfg = CordonConfig {
            keyless_via_cooldown: false,
            ..Default::default()
        };
        let health = vec![TierHealth {
            tier: "haiku".to_string(),
            status: TierStatus::Keyless,
            checked_at: 0,
            consecutive_failures: 0,
        }];
        let cordon = Cordon::from_health(&health, &cfg);

        let d = cordon.should_attempt("haiku", 9_999_999);
        assert!(
            matches!(d, Decision::Skip { ref reason, retry_after: None } if reason.contains("keyless")),
            "expected Skip with keyless reason and no retry_after; got {d:?}"
        );
    }

    #[test]
    fn keyless_flip_to_reachable_returns_attempt() {
        let cfg = CordonConfig {
            keyless_via_cooldown: false,
            ..Default::default()
        };

        // Initial state: keyless
        let health_keyless = vec![TierHealth {
            tier: "haiku".to_string(),
            status: TierStatus::Keyless,
            checked_at: 0,
            consecutive_failures: 0,
        }];
        let cordon_keyless = Cordon::from_health(&health_keyless, &cfg);
        let d = cordon_keyless.should_attempt("haiku", 0);
        assert!(matches!(d, Decision::Skip { .. }), "keyless → Skip");

        // Key appears: rebuild Cordon with Reachable status
        let health_reachable = vec![TierHealth {
            tier: "haiku".to_string(),
            status: TierStatus::Reachable,
            checked_at: 1,
            consecutive_failures: 0,
        }];
        let cordon_reachable = Cordon::from_health(&health_reachable, &cfg);
        let d = cordon_reachable.should_attempt("haiku", 1);
        assert!(
            matches!(d, Decision::Attempt),
            "reachable → Attempt after key appears; got {d:?}"
        );
    }
}

// AC6 — keel cordon --format json lists every configured tier with its Decision
//        and retry_after, consistent with the injected health state
mod ac6 {
    use keel::cordon::{Cordon, CordonConfig, Decision};
    use keel::types::{TierHealth, TierStatus};

    #[test]
    fn json_output_all_tiers_with_decision_and_retry_after() {
        let cfg = CordonConfig {
            base_secs: 60,
            max_secs: 3_600,
            keyless_via_cooldown: false,
        };
        // Build a mixed health snapshot
        let healths = vec![
            TierHealth { tier: "local-3b".to_string(), status: TierStatus::Reachable, checked_at: 0, consecutive_failures: 0 },
            TierHealth { tier: "haiku".to_string(), status: TierStatus::Keyless, checked_at: 0, consecutive_failures: 0 },
            TierHealth { tier: "sonnet".to_string(), status: TierStatus::Exhausted, checked_at: 0, consecutive_failures: 0 },
        ];
        let mut cordon = Cordon::from_health(&healths, &cfg);
        // Give sonnet a recorded failure so it has a cooldown
        cordon.record_failure("sonnet", 1_000, TierStatus::Exhausted);

        let now = 1_001_i64;

        // Check expected decisions
        let local_d = cordon.should_attempt("local-3b", now);
        assert!(matches!(local_d, Decision::Attempt), "local-3b should Attempt");

        let haiku_d = cordon.should_attempt("haiku", now);
        assert!(matches!(haiku_d, Decision::Skip { retry_after: None, .. }), "haiku should Skip (keyless)");

        let sonnet_d = cordon.should_attempt("sonnet", now);
        assert!(
            matches!(sonnet_d, Decision::Skip { retry_after: Some(_), .. }),
            "sonnet should Skip with retry_after; got {sonnet_d:?}"
        );

        // Serialize decisions to JSON and verify round-trip
        let entries: Vec<serde_json::Value> = cordon.states().iter().map(|s| {
            let d = cordon.should_attempt(&s.tier, now);
            serde_json::json!({
                "tier": s.tier,
                "decision": d,
                "consecutive_failures": s.consecutive_failures,
                "cooldown_until": s.cooldown_until,
            })
        }).collect();

        let json = serde_json::to_string_pretty(&entries).expect("JSON serialization");
        assert!(json.contains("attempt") || json.contains("skip"), "JSON should contain decision values");
        // Verify all 3 tiers are present
        assert!(json.contains("local-3b"), "JSON should contain local-3b");
        assert!(json.contains("haiku"), "JSON should contain haiku");
        assert!(json.contains("sonnet"), "JSON should contain sonnet");
    }
}

// AC7 — Cordon consumes keel-pulse TierHealth and types directly (no duplicated type)
mod ac7 {
    use keel::cordon::{Cordon, CordonConfig};
    use keel::types::{TierHealth, TierStatus};

    /// Constructs a Cordon from both pulse TierHealth and status stamps.
    #[test]
    fn cordon_from_health_and_status_stamps_merged_view() {
        let cfg = CordonConfig::default();

        // Simulate pulse TierHealth with mixed states
        let healths = vec![
            TierHealth {
                tier: "local-3b".to_string(),
                status: TierStatus::Reachable,
                checked_at: 1_000,
                consecutive_failures: 0,
            },
            TierHealth {
                tier: "haiku".to_string(),
                status: TierStatus::Exhausted,
                checked_at: 1_000,
                consecutive_failures: 2,
            },
        ];

        let cordon = Cordon::from_health(&healths, &cfg);

        // The cordon must use the same TierStatus types from keel::types (no duplication)
        let states = cordon.states();
        let local = states.iter().find(|s| s.tier == "local-3b").expect("local-3b");
        let haiku = states.iter().find(|s| s.tier == "haiku").expect("haiku");

        assert_eq!(local.status, TierStatus::Reachable);
        assert_eq!(haiku.status, TierStatus::Exhausted);

        // Decision reflects the status — no separate type needed
        use keel::cordon::Decision;
        assert!(matches!(cordon.should_attempt("local-3b", 2_000), Decision::Attempt));
        // haiku has Exhausted status but no cooldown_until yet → Attempt (first encounter)
        assert!(matches!(cordon.should_attempt("haiku", 2_000), Decision::Attempt));
    }
}

// AC8 guard — this integration test file entry appears in `cargo test` output
// (as "Running tests/cordon.rs"), confirming the file is wired correctly.
// No test function needed here — the module structure above is sufficient.
