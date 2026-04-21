//! `vertex-veil-agents` binary entry point.
//!
//! Phase 3 wires the `demo` subcommand to the coordination runtime and the
//! `verify` subcommand to the standalone verifier.

use std::process::ExitCode;

use clap::Parser;

use vertex_veil_agents::{
    run::{demo, verify, DemoArgs},
    Cli, Command,
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("vertex-veil-agents error: {err}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Demo {
            topology,
            private_intents,
            scenario,
            artifacts,
            max_rounds,
            run_id,
        } => {
            let report = demo(DemoArgs {
                topology,
                private_intents,
                scenario,
                artifacts: artifacts.clone(),
                max_rounds,
                run_id: run_id.clone(),
            })?;
            eprintln!(
                "vertex-veil-agents: demo run_id={} final_round={} valid={}{}",
                run_id,
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
                    "verifier rejected the demo log: {:?}",
                    report.reasons
                )
                .into());
            }
            Ok(())
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
            Ok(())
        }
    }
}
