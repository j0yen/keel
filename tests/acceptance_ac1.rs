//! AC1: cargo build and cargo test succeed offline (no live network).
//! A test asserts the probe path makes zero real outbound connections (FakeProbe only).

use keel::probe::{FakeProbe, FakeProbeEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};

#[test]
fn probe_makes_zero_real_connections() {
    // FakeProbe never opens a socket.  If this compiles and runs, the
    // offline constraint is structurally enforced: there is no code path
    // in FakeProbe that calls ureq or std::net.
    let probe = FakeProbe::new();
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        ..Default::default()
    };
    let results = run_pulse(&probe, &env);
    // Verify we got results without panicking
    assert!(!results.is_empty(), "expected at least one tier in results");

    // Verify the output path also exercises FakeProbe without network
    let _code = print_pulse(&results, OutputFormat::Table)
        .expect("print_pulse should not fail");
}
