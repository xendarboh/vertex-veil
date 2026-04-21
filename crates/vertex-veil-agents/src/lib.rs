//! CLI surface for the Vertex Veil agent binary.
//!
//! Phase 3 fills this in: the `demo` subcommand spins up a Vertex-ordered
//! coordination runtime and persists a public artifact bundle, and the
//! `verify` subcommand reads that bundle with the standalone verifier.
//!
//! # Scope
//!
//! This module exposes [`Cli`] / [`Command`] and helpers used to drive the
//! runtime from a single-process binary. The core protocol logic lives in
//! `vertex-veil-core::runtime`; nothing in here depends on a specific
//! transport implementation.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub mod private_intents;
pub mod run;

#[cfg(feature = "vertex-transport")]
pub mod vertex_transport;

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
    /// Run a demo coordination flow against a topology fixture. Writes a
    /// full public artifact bundle (coordination log, verifier report, run
    /// status, completion receipt, README) to the artifact directory.
    Demo {
        /// Topology configuration file (TOML).
        #[arg(long)]
        topology: PathBuf,
        /// Private-intent fixture (TOML) matched to the topology.
        #[arg(long)]
        private_intents: Option<PathBuf>,
        /// Optional adversarial scenario file (TOML).
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Directory to write public coordination artifacts into.
        #[arg(long)]
        artifacts: PathBuf,
        /// Max fallback rounds before aborting. Defaults to 4.
        #[arg(long, default_value_t = 4)]
        max_rounds: u64,
        /// Optional run identifier (default: `veil-demo`).
        #[arg(long, default_value = "veil-demo")]
        run_id: String,
        /// Overwrite the bundle in place instead of rotating the prior
        /// bundle into `<artifacts>.prev-<N>`. Unrelated files are always
        /// preserved; only files owned by this writer are replaced.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Run the standalone verifier against a saved artifact directory.
    Verify {
        /// Directory containing a coordination log and its topology.
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
