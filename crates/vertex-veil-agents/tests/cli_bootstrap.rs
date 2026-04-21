//! CLI bootstrap tests for `vertex-veil-agents`.
//!
//! Phase 0 only needs to confirm the CLI parses its subcommand shape and
//! rejects obviously invalid inputs. Later phases will add runtime-behavior
//! tests.

use std::path::PathBuf;

use vertex_veil_agents::{Cli, Command};

#[test]
fn cli_bootstrap_parses_demo_subcommand() {
    let cli = Cli::try_parse_args([
        "vertex-veil-agents",
        "demo",
        "--topology",
        "fixtures/topology-4node.toml",
        "--artifacts",
        "artifacts/phase0",
    ])
    .expect("demo parses");
    match cli.command {
        Command::Demo {
            topology,
            scenario,
            artifacts,
            ..
        } => {
            assert_eq!(topology, PathBuf::from("fixtures/topology-4node.toml"));
            assert_eq!(artifacts, PathBuf::from("artifacts/phase0"));
            assert!(scenario.is_none());
        }
        other => panic!("expected Demo, got {other:?}"),
    }
}

#[test]
fn cli_bootstrap_parses_demo_with_scenario() {
    let cli = Cli::try_parse_args([
        "vertex-veil-agents",
        "demo",
        "--topology",
        "fixtures/topology-4node.toml",
        "--scenario",
        "fixtures/replay-doublecommit-drop.toml",
        "--artifacts",
        "artifacts/phase0",
    ])
    .expect("demo with scenario parses");
    match cli.command {
        Command::Demo { scenario, .. } => {
            assert_eq!(
                scenario,
                Some(PathBuf::from("fixtures/replay-doublecommit-drop.toml"))
            );
        }
        other => panic!("expected Demo, got {other:?}"),
    }
}

#[test]
fn cli_bootstrap_parses_verify_subcommand() {
    let cli = Cli::try_parse_args([
        "vertex-veil-agents",
        "verify",
        "--artifacts",
        "artifacts/phase3",
    ])
    .expect("verify parses");
    match cli.command {
        Command::Verify { artifacts } => {
            assert_eq!(artifacts, PathBuf::from("artifacts/phase3"));
        }
        other => panic!("expected Verify, got {other:?}"),
    }
}

#[test]
fn cli_bootstrap_rejects_missing_required_args() {
    // `demo` without `--topology` must fail.
    assert!(
        Cli::try_parse_args([
            "vertex-veil-agents",
            "demo",
            "--artifacts",
            "artifacts/phase0",
        ])
        .is_err()
    );
    // Unknown subcommand must fail.
    assert!(
        Cli::try_parse_args(["vertex-veil-agents", "nonsense"]).is_err()
    );
}
