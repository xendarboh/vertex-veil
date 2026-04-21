//! `vertex-veil-agents` binary entry point.
//!
//! Phase 3 wired the `demo` subcommand to the coordination runtime and the
//! `verify` subcommand to the standalone verifier. Phase 4 hardens the
//! bundle layout, persists aborts as a coherent artifact set, and adds
//! directory versioning so the single-command judge flow does not destroy
//! prior runs on replay.

use std::process::ExitCode;

use clap::Parser;

use vertex_veil_agents::{
    run::{demo, verify, DemoArgs},
    Cli, Command,
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("vertex-veil-agents error: {err}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::Demo {
            topology,
            private_intents,
            scenario,
            artifacts,
            max_rounds,
            run_id,
            force,
        } => {
            let result = demo(DemoArgs {
                topology,
                private_intents,
                scenario,
                artifacts: artifacts.clone(),
                max_rounds,
                run_id: run_id.clone(),
                force,
            })?;
            if let Some(prev) = &result.rotated_prev {
                eprintln!(
                    "vertex-veil-agents: rotated prior bundle to {}",
                    prev.display()
                );
            }
            eprintln!(
                "vertex-veil-agents: demo run_id={} final_round={} finalized={} valid={}{}{}",
                run_id,
                result.report.final_round.value(),
                result.finalized,
                result.report.valid,
                match &result.abort_reason {
                    Some(r) => format!(" abort_reason={r}"),
                    None => String::new(),
                },
                if result.report.reasons.is_empty() {
                    String::new()
                } else {
                    format!(" reasons={:?}", result.report.reasons)
                }
            );
            if !result.report.valid {
                return Err(format!(
                    "verifier rejected the demo log: {:?}",
                    result.report.reasons
                )
                .into());
            }
            if !result.finalized {
                // Aborted runs exit non-zero so CI / demo scripts can detect
                // the threshold-exceeded path without parsing artifacts.
                return Ok(ExitCode::from(2));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Verify { artifacts } => {
            let report = verify(&artifacts)?;
            eprintln!(
                "vertex-veil-agents: verify run_id={} final_round={} valid={}{}",
                report.run_id,
                report.final_round.value(),
                report.valid,
                if report.reasons.is_empty() {
                    String::new()
                } else {
                    format!(" reasons={:?}", report.reasons)
                }
            );
            if !report.valid {
                return Err(format!(
                    "verifier rejected the persisted log: {:?}",
                    report.reasons
                )
                .into());
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
