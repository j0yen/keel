//! `keel` — brain tier-ladder health probe and shared types.
//!
//! This crate is the foundational corpus for the `keel` fleet.
//! Sibling crates (`keel-ledger`, `keel-cordon`, `keel-beacon`) extend
//! the types and traits declared here.
//!
//! # Quick start
//!
//! ```no_run
//! use keel::probe::{FakeProbe, FakeProbeEnv};
//! use keel::pulse::{OutputFormat, run_pulse, print_pulse};
//!
//! let probe = FakeProbe::new();
//! let env = FakeProbeEnv::default();
//! let results = run_pulse(&probe, &env);
//! let _exit = print_pulse(&results, OutputFormat::Table).unwrap();
//! ```

pub mod beacon;
pub mod ladder;
pub mod ledger;
pub mod mark;
pub mod probe;
pub mod pulse;
pub mod spend;
pub mod status;
pub mod types;
