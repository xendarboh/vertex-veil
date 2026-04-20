//! `vertex-veil-agents` binary entry point.
//!
//! Phase 0 only validates topology configuration on the `demo` path and
//! confirms the `verify` path receives a directory. Subsequent phases wire
//! the Vertex runtime, proof generation, and verifier logic.

use std::process::ExitCode;

use clap::Parser;

use vertex_veil_agents::{Cli, Command};
use vertex_veil_core::{ArtifactWriter, TopologyConfig};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("vertex-veil-agents error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Demo {
            topology,
            scenario,
            artifacts,
        } => {
            let _cfg = TopologyConfig::load(&topology)?;
            let _writer = ArtifactWriter::new(&artifacts)?;
            eprintln!(
                "vertex-veil-agents: Phase 0 bootstrap — topology {} loaded, artifacts dir {} ready{}",
                topology.display(),
                artifacts.display(),
                match &scenario {
                    Some(s) => format!(", scenario {}", s.display()),
                    None => String::new(),
                }
            );
            Ok(())
        }
        Command::Verify { artifacts } => {
            if !artifacts.is_dir() {
                return Err(format!(
                    "verify: artifacts path is not a directory: {}",
                    artifacts.display()
                )
                .into());
            }
            eprintln!(
                "vertex-veil-agents: Phase 0 bootstrap — verify stub for {}",
                artifacts.display()
            );
            Ok(())
        }
    }
}
