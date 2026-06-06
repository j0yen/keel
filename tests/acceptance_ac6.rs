//! AC6: keel pulse exits non-zero when top configured tier is not Reachable,
//! zero otherwise; two integration cases.

use keel::probe::{FakeProbe, FakeProbeEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};
use keel::types::TierStatus;

#[test]
fn exit_zero_when_top_tier_reachable() {
    let probe = FakeProbe::new(); // all → Reachable by default
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        skip: vec![],
        max: None,
    };

    let results = run_pulse(&probe, &env);
    let code = print_pulse(&results, OutputFormat::Table).expect("should not fail");
    assert_eq!(code, 0, "exit code should be 0 when top tier is Reachable");
}

#[test]
fn exit_nonzero_when_top_tier_unreachable() {
    let mut probe = FakeProbe::new();
    // Make "opus" (the top tier) unreachable
    probe.responses.insert(
        "opus".to_string(),
        TierStatus::Unreachable {
            reason: "test forced".to_string(),
        },
    );
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        skip: vec![],
        max: None,
    };

    let results = run_pulse(&probe, &env);
    let code = print_pulse(&results, OutputFormat::Table).expect("should not fail");
    assert_ne!(code, 0, "exit code should be non-zero when top tier is Unreachable");
}
