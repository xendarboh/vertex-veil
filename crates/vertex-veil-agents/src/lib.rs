//! CLI bootstrap for the Vertex Veil agent binary.
//!
//! Phase 0 scope is limited to command-line argument parsing and the
//! subcommand shape that later phases will fill in. No Vertex network
//! behavior, no proof generation, and no verifier logic live here yet.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Top-level CLI for `vertex-veil-agents`.
#[derive(Debug, Parser)]
#[command(
    name = "vertex-veil-agents",
    about = "Vertex Veil CLI agents and standalone verifier",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands exposed by the CLI.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a demo coordination flow against a topology fixture. Phase 0
    /// registers the subcommand shape; subsequent phases implement behavior.
    Demo {
        /// Topology configuration file (TOML).
        #[arg(long)]
        topology: PathBuf,
        /// Optional adversarial scenario file (TOML).
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Directory to write public coordination artifacts into.
        #[arg(long)]
        artifacts: PathBuf,
    },
    /// Run the standalone verifier against a saved artifact directory.
    Verify {
        /// Directory containing a coordination log and proofs.
        #[arg(long)]
        artifacts: PathBuf,
    },
}

impl Cli {
    /// Alias for [`Parser::try_parse_from`] so test helpers don't have to
    /// import the `clap` prelude.
    pub fn try_parse_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(args)
    }
}
