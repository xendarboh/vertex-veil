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
pub mod node;
#[cfg(feature = "vertex-transport")]
pub mod orchestrate;
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
        /// Emit narratable `[COORD]` / `[VERTEX]` / `[ABORT]` stdout
        /// tags so the single-command run reads like a live-narratable
        /// video while still writing the full artifact bundle.
        #[arg(long, default_value_t = false)]
        narrate: bool,
    },
    /// Run the standalone verifier against a saved artifact directory.
    Verify {
        /// Directory containing a coordination log and its topology.
        #[arg(long)]
        artifacts: PathBuf,
    },
    /// Run a single coordinated node attached to a real `tashi-vertex`
    /// consensus transport. Spawn four of these with matching peer lists to
    /// run the Phase 5 BFT baseline. Each node writes its own bundle to
    /// `<artifacts>/<node-alias>/`.
    ///
    /// Feature-gated behind `vertex-transport` so the default build stays
    /// network-free.
    #[cfg(feature = "vertex-transport")]
    Node {
        /// Local socket bind (e.g. `127.0.0.1:9000`).
        #[arg(long)]
        bind: String,
        /// Peer entries as `<pubkey_hex>@<addr>`. Repeat for each peer.
        #[arg(long = "peer")]
        peers: Vec<String>,
        /// The 64-char hex node id this process speaks for. MUST be present
        /// in the topology.
        #[arg(long)]
        node_id: String,
        /// Optional human-readable alias used as the bundle subdirectory
        /// name and the stdout log prefix (e.g. `n1`).
        #[arg(long)]
        node_alias: Option<String>,
        /// Topology configuration file (TOML), shared across the cluster.
        #[arg(long)]
        topology: PathBuf,
        /// Private-intent fixture (TOML). May cover multiple agents; this
        /// process keeps only its own entry in memory.
        #[arg(long)]
        private_intents: PathBuf,
        /// Optional adversarial scenario file (TOML).
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Parent directory for this node's artifact bundle. Writes to
        /// `<artifacts>/<node-alias>/`.
        #[arg(long)]
        artifacts: PathBuf,
        /// Max fallback rounds before aborting. Defaults to 4.
        #[arg(long, default_value_t = 4)]
        max_rounds: u64,
        /// Run identifier (shared across the cluster for one run).
        #[arg(long, default_value = "veil-bft")]
        run_id: String,
        /// Rejoin an existing cluster instead of bootstrapping fresh.
        #[arg(long, default_value_t = false)]
        rejoin: bool,
        /// Max milliseconds per transport poll.
        #[arg(long, default_value_t = 500)]
        poll_timeout_ms: u64,
        /// Env var holding the tashi-vertex `KeySecret` (base58-encoded
        /// DER). Preferred over `--secret` so the value does not land in
        /// argv.
        #[arg(long)]
        secret_env: Option<String>,
        /// Direct tashi-vertex `KeySecret` as a base58-encoded string.
        /// Discouraged; the value shows up in process listings. Use
        /// `--secret-env` unless debugging.
        #[arg(long = "secret")]
        secret_str: Option<String>,
    },
    /// Orchestrate a multi-process BFT baseline: spawn one `node` child per
    /// topology entry, optionally kill one mid-run and restart it with
    /// `--rejoin`, aggregate per-node bundles. Requires the
    /// `vertex-transport` feature.
    #[cfg(feature = "vertex-transport")]
    DemoBft {
        /// Topology configuration file (TOML).
        #[arg(long, default_value = "fixtures/topology-4node.toml")]
        topology: PathBuf,
        /// Private-intent fixture (TOML). Same file passed to every child;
        /// each child keeps only its own slice in memory.
        #[arg(long, default_value = "fixtures/topology-4node.private.toml")]
        private_intents: PathBuf,
        /// Optional adversarial scenario file (TOML).
        #[arg(long)]
        scenario: Option<PathBuf>,
        /// Parent directory for per-node bundles. Each child writes to
        /// `<artifacts>/<alias>/`.
        #[arg(long, default_value = "artifacts/bft")]
        artifacts: PathBuf,
        /// First UDP port. Children bind to `base_port..base_port+N-1`.
        #[arg(long, default_value_t = 9000)]
        base_port: u16,
        /// If set, the orchestrator kills one node when it observes this
        /// round committed on its stdout.
        #[arg(long)]
        fail_at_round: Option<u64>,
        /// When combined with `--fail-at-round`, delay this many ms then
        /// respawn the killed node with `--rejoin`.
        #[arg(long)]
        rejoin_after_ms: Option<u64>,
        /// Alias of the node to kill. Defaults to the last node in stable
        /// key order (e.g. `n4` for a 4-node topology).
        #[arg(long)]
        fail_target: Option<String>,
        /// Max fallback rounds before a child aborts. Defaults to 4.
        #[arg(long, default_value_t = 4)]
        max_rounds: u64,
        /// Run identifier shared across all children.
        #[arg(long, default_value = "veil-bft")]
        run_id: String,
        /// Max ms per child transport poll.
        #[arg(long, default_value_t = 500)]
        poll_timeout_ms: u64,
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
