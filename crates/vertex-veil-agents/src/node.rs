//! Single-node agent subcommand wired to the real `tashi-vertex` transport.
//!
//! A `node` process runs ONE agent (one entry from the private-intent
//! bundle), speaks on the `VertexTransport`, and writes its own artifact
//! bundle to `<artifacts>/<node-alias>/`. Narratable stdout tags
//! (`[VERTEX]`, `[COORD]`, `[ABORT]`) are emitted at protocol milestones
//! so the orchestrator can interleave per-node streams for a live-narratable
//! demo.
//!
//! Gated behind the `vertex-transport` cargo feature; not compiled in the
//! default build.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use vertex_veil_core::{
    ArtifactWriter, CompletionReceiptRecord, CoordinationLog, CoordinationRuntime, NodeId,
    RunStatus, RuntimeObserver, Scenario, StandaloneVerifier, TopologyConfig, VerifierReport,
};

use crate::private_intents;
use crate::vertex_transport::{VertexConfig, VertexTransport};

/// High-level error returned by [`run_node`].
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("topology load error: {0}")]
    Topology(String),
    #[error("private-intent load error: {0}")]
    PrivateIntent(String),
    #[error("scenario load error: {0}")]
    Scenario(String),
    #[error("node identity error: {0}")]
    Identity(String),
    #[error("peer spec error: {0}")]
    Peer(String),
    #[error("secret material error: {0}")]
    Secret(String),
    #[error("vertex transport error: {0}")]
    Transport(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("artifact io error: {0}")]
    Io(String),
}

/// Outcome of a `node` run surfaced to the CLI dispatcher.
#[derive(Debug)]
pub struct NodeResult {
    pub node_alias: String,
    pub artifacts_dir: PathBuf,
    pub report: VerifierReport,
    pub finalized: bool,
    pub abort_reason: Option<String>,
}

/// Configured node arguments gathered from the CLI.
pub struct NodeArgs {
    /// Local socket bind (e.g. `127.0.0.1:9000`).
    pub bind: String,
    /// Peer entries as `<pubkey_hex>@<addr>`.
    pub peers: Vec<String>,
    /// Local node id (64-char hex) that this process speaks for.
    pub node_id: String,
    /// Optional human alias (e.g. `n1`). Defaults to `node-<hex-prefix>`.
    pub node_alias: Option<String>,
    /// Topology configuration file (TOML).
    pub topology: PathBuf,
    /// Private-intent fixture. May cover more than one agent; this process
    /// extracts the entry for `--node-id` and drops the rest immediately.
    pub private_intents: PathBuf,
    /// Optional adversarial scenario file (TOML).
    pub scenario: Option<PathBuf>,
    /// Parent directory for this node's artifact bundle. Final path is
    /// `<artifacts>/<node-alias>/`.
    pub artifacts: PathBuf,
    /// Max fallback rounds before aborting. Defaults to 4.
    pub max_rounds: u64,
    /// Run identifier (shared across the cluster for a given run).
    pub run_id: String,
    /// Whether this node is rejoining an existing cluster.
    pub rejoin: bool,
    /// Keep the node alive after each completed session.
    pub persist: bool,
    /// Delay between persistent sessions.
    pub persist_sleep_ms: u64,
    /// Max time to block inside a single transport poll (ms).
    pub poll_timeout_ms: u64,
    /// Name of an environment variable holding the tashi-vertex
    /// `KeySecret` (base58-encoded DER, variable length). Preferred over
    /// `--secret` so the secret never lands in argv / process listings.
    pub secret_env: Option<String>,
    /// Direct base58 `KeySecret` string. Discouraged; exposed in argv.
    pub secret_str: Option<String>,
}

/// Run one node to completion (finalization, coherent abort, or error).
pub fn run_node(args: NodeArgs) -> Result<NodeResult, NodeError> {
    let topology = TopologyConfig::load(&args.topology)
        .map_err(|e| NodeError::Topology(e.to_string()))?;

    let local_node: NodeId = args
        .node_id
        .parse()
        .map_err(|_| NodeError::Identity("invalid --node-id".into()))?;

    // Verify the node id is in the topology.
    let topo_node = topology
        .nodes
        .iter()
        .find(|n| n.id == local_node)
        .ok_or_else(|| NodeError::Identity("--node-id not present in topology".into()))?;
    let _ = topo_node; // reserved for future per-node config

    let node_alias = args
        .node_alias
        .clone()
        .unwrap_or_else(|| format!("node-{}", &local_node.to_hex()[..8]));

    // Load and filter private intents.
    let mut agents = private_intents::load(&args.private_intents, &topology)
        .map_err(|e| NodeError::PrivateIntent(e.to_string()))?;
    let local_agent_state = agents.remove(&local_node).ok_or_else(|| {
        NodeError::PrivateIntent(
            "private-intents file has no entry for local node".into(),
        )
    })?;
    drop(agents); // Drop other nodes' secrets from memory as soon as possible.
    let mut local_agents: BTreeMap<NodeId, vertex_veil_core::AgentState> = BTreeMap::new();
    local_agents.insert(local_node, local_agent_state);

    let scenario = match args.scenario.as_ref() {
        Some(p) => Scenario::load(p).map_err(|e| NodeError::Scenario(e.to_string()))?,
        None => Scenario::empty(),
    };

    // Resolve the Vertex KeySecret (base58-encoded DER). Prefer env so the
    // value does not show up in process listings.
    let secret_str = resolve_secret(&args.secret_env, &args.secret_str)?;

    // Parse peers: "<pubkey_hex>@<addr>".
    let mut peer_pairs: Vec<(String, String)> = Vec::with_capacity(args.peers.len());
    for raw in &args.peers {
        let (pk, addr) = raw
            .split_once('@')
            .ok_or_else(|| NodeError::Peer("peer must be <pubkey>@<addr>".into()))?;
        // Don't inspect the pubkey format here — `tashi_vertex::KeyPublic`
        // uses variable-length base58-encoded DER, and parse errors surface
        // during transport startup with a precise message.
        peer_pairs.push((pk.to_string(), addr.to_string()));
    }

    // Bring up the Vertex transport. Note: `secret_hex` is the field name
    // in `VertexConfig` for historical reasons; the payload is the base58
    // `KeySecret` string that `tashi_vertex::KeySecret::from_str` parses.
    let config = VertexConfig {
        bind: args.bind.clone(),
        secret_hex: secret_str,
        peers: peer_pairs,
        rejoin: args.rejoin,
        poll_timeout: Duration::from_millis(args.poll_timeout_ms),
    };

    println!(
        "[{alias}] [VERTEX] engine start addr={bind} peers={npeers} rejoin={rejoin}",
        alias = node_alias,
        bind = args.bind,
        npeers = args.peers.len(),
        rejoin = args.rejoin,
    );

    let mut transport = VertexTransport::start(config)
        .map_err(|e| NodeError::Transport(e.to_string()))?;

    // Bootstrap: give Vertex a moment to form consensus connections with
    // all peers before we start broadcasting application transactions.
    // Without this, early rounds can race past the first ordering window
    // and every commitment is recorded as `requester_missing` across the
    // cluster. The wait is bounded and runs unconditionally — ~2s on
    // loopback, long enough for HELLO-like exchange to settle.
    bootstrap_wait(&mut transport, Duration::from_secs(2));
    println!("[{}] [VERTEX] bootstrap complete", node_alias);

    let topology_text = fs::read_to_string(&args.topology).map_err(|e| NodeError::Io(e.to_string()))?;
    let scenario_text = match &args.scenario {
        Some(scn_path) => Some(
            fs::read_to_string(scn_path).map_err(|e| NodeError::Io(e.to_string()))?,
        ),
        None => None,
    };
    let bundle_dir = args.artifacts.join(&node_alias);
    let sleep_between_runs = Duration::from_millis(args.persist_sleep_ms);
    let mut session_idx = 0u64;
    let mut latest_result: NodeResult;
    let mut transport = transport;

    loop {
        let session_run_id = if args.persist {
            format!("{}-r{:03}", args.run_id, session_idx)
        } else {
            args.run_id.clone()
        };

        if args.persist {
            println!(
                "[{alias}] [VERTEX] persistent session start run_id={run_id}",
                alias = node_alias,
                run_id = session_run_id,
            );
        }

        let observer = Box::new(StdoutObserver::new(node_alias.clone()));
        let rt = CoordinationRuntime::new(
            topology.clone(),
            transport,
            local_agents.clone(),
            scenario.clone(),
            args.max_rounds,
        )
        .map_err(|e| NodeError::Runtime(e.to_string()))?
        .with_observer(observer);

        let (outcome, next_transport) = rt
            .run_with_transport(session_run_id.clone())
            .map_err(|e| NodeError::Runtime(e.to_string()))?;
        transport = next_transport;

        let report = persist_node_bundle(
            &bundle_dir,
            &topology,
            &topology_text,
            scenario_text.as_deref(),
            &node_alias,
            &outcome,
        )?;

        let result = NodeResult {
            node_alias: node_alias.clone(),
            artifacts_dir: bundle_dir.clone(),
            report,
            finalized: outcome.finalized,
            abort_reason: outcome.log.abort_reason.clone(),
        };

        if args.persist {
            let reason_suffix = match &result.abort_reason {
                Some(reason) => format!(" abort_reason={reason}"),
                None => String::new(),
            };
            println!(
                "[{alias}] [VERTEX] persistent session complete run_id={run_id} final_round={round} finalized={finalized}{reason}",
                alias = node_alias,
                run_id = session_run_id,
                round = result.report.final_round.value(),
                finalized = result.finalized,
                reason = reason_suffix,
            );
        }

        latest_result = result;

        if !args.persist {
            break;
        }

        session_idx = session_idx.saturating_add(1);
        thread::sleep(sleep_between_runs);
    }

    Ok(latest_result)
}

fn persist_node_bundle(
    bundle_dir: &std::path::Path,
    topology: &TopologyConfig,
    topology_text: &str,
    scenario_text: Option<&str>,
    alias: &str,
    outcome: &vertex_veil_core::RuntimeOutcome,
) -> Result<VerifierReport, NodeError> {
    let writer = ArtifactWriter::new(bundle_dir).map_err(|e| NodeError::Io(e.to_string()))?;
    clean_owned_files(writer.dir())?;

    writer
        .write_coordination_log(&outcome.log)
        .map_err(|e| NodeError::Io(e.to_string()))?;

    fs::write(writer.dir().join("topology.toml"), topology_text)
        .map_err(|e| NodeError::Io(e.to_string()))?;
    if let Some(text) = scenario_text {
        fs::write(writer.dir().join("scenario.toml"), text)
            .map_err(|e| NodeError::Io(e.to_string()))?;
    }

    let verifier = StandaloneVerifier::new(topology.clone());
    let report = verifier.verify_log(&outcome.log);
    writer
        .write_verifier_report(&report)
        .map_err(|e| NodeError::Io(e.to_string()))?;

    let receipt = pick_final_receipt(&outcome.log);
    writer
        .write_receipt_copy(receipt)
        .map_err(|e| NodeError::Io(e.to_string()))?;

    let mut bundle_files = list_bundle_files(writer.dir());
    for name in ["run_status.json", "bundle_README.md"] {
        if !bundle_files.iter().any(|f| f == name) {
            bundle_files.push(name.to_string());
        }
    }
    bundle_files.sort();

    let status = RunStatus {
        run_id: outcome.log.run_id.clone(),
        finalized: outcome.finalized,
        final_round: outcome.final_round,
        receipt_present: receipt.is_some(),
        abort_reason: outcome.log.abort_reason.clone(),
        rejection_count: outcome.log.rejections.len(),
        bundle_files,
    };
    writer
        .write_run_status(&status)
        .map_err(|e| NodeError::Io(e.to_string()))?;
    writer
        .write_bundle_readme(&render_bundle_readme(&status, alias))
        .map_err(|e| NodeError::Io(e.to_string()))?;

    Ok(report)
}

const OWNED_FILES: &[&str] = &[
    "coordination_log.json",
    "verifier_report.json",
    "run_status.json",
    "completion_receipt.json",
    "topology.toml",
    "scenario.toml",
    "bundle_README.md",
];

fn clean_owned_files(dir: &std::path::Path) -> Result<(), NodeError> {
    if !dir.exists() {
        return Ok(());
    }
    for name in OWNED_FILES {
        let path = dir.join(name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| NodeError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

fn bootstrap_wait(transport: &mut VertexTransport, window: Duration) {
    use std::time::Instant;
    use vertex_veil_core::CoordinationTransport;
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        // flush() drains any pending Vertex events (SyncPoints, initial
        // HELLO-equivalents) and returns when the engine has no more
        // events ready within its poll timeout.
        transport.flush();
    }
}

fn resolve_secret(
    secret_env: &Option<String>,
    secret_str: &Option<String>,
) -> Result<String, NodeError> {
    // Vertex secrets are base58-encoded DER — variable length, not raw hex.
    // We let `tashi_vertex::KeySecret::from_str` validate the payload on
    // parse; here we just route it from one of the three sources the orch
    // or manual recipes may use.
    if let Some(var) = secret_env {
        let val = std::env::var(var)
            .map_err(|_| NodeError::Secret(format!("env var {var} unset or non-utf8")))?;
        return Ok(val);
    }
    if let Some(s) = secret_str {
        return Ok(s.clone());
    }
    Err(NodeError::Secret(
        "provide --secret-env <VAR> or --secret <tashi-vertex KeySecret>".into(),
    ))
}

fn pick_final_receipt(log: &CoordinationLog) -> Option<&CompletionReceiptRecord> {
    if !log.finalized {
        return None;
    }
    log.receipts
        .iter()
        .rev()
        .find(|r| r.round == log.final_round)
}

fn list_bundle_files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn render_bundle_readme(status: &RunStatus, alias: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Vertex Veil Per-Node Bundle ({alias})\n\n"));
    s.push_str(
        "This directory holds one participant's public view of a multi-process\n",
    );
    s.push_str("Vertex-Veil coordination run. Every file here is derived from public\n");
    s.push_str("inputs only; no private witness material is present.\n\n");
    s.push_str("## Outcome\n\n");
    s.push_str(&format!("- run_id: `{}`\n", status.run_id));
    s.push_str(&format!(
        "- finalized: `{}`\n",
        if status.finalized { "true" } else { "false" }
    ));
    s.push_str(&format!(
        "- final_round: `{}`\n",
        status.final_round.value()
    ));
    if let Some(r) = &status.abort_reason {
        s.push_str(&format!("- abort_reason: `{r}`\n"));
    }
    s.push_str(&format!("- rejections_logged: `{}`\n\n", status.rejection_count));
    s.push_str("## Re-verify\n\n```bash\n");
    s.push_str("cargo run -p vertex-veil-agents -- verify --artifacts <path-to-this-dir>\n");
    s.push_str("```\n");
    s
}

// Stdout observer ------------------------------------------------------------

/// Emits narratable protocol events as `[N-alias] [TAG] …` lines. Wraps the
/// inner write target in a Mutex so the orchestrator sees complete lines
/// even under tokio-runtime-driven concurrency.
struct StdoutObserver {
    alias: String,
    lock: Mutex<()>,
}

impl StdoutObserver {
    fn new(alias: String) -> Self {
        StdoutObserver {
            alias,
            lock: Mutex::new(()),
        }
    }

    fn emit(&self, tag: &str, detail: &str) {
        let _g = self.lock.lock().ok();
        println!("[{}] {} {}", self.alias, tag, detail);
    }
}

impl RuntimeObserver for StdoutObserver {
    fn on_round_committed(
        &self,
        round: vertex_veil_core::RoundId,
        finalized: bool,
    ) {
        self.emit(
            "[VERTEX]",
            &format!(
                "round {} committed (finalized={})",
                round.value(),
                finalized
            ),
        );
    }
    fn on_commitment(&self, node: NodeId, round: vertex_veil_core::RoundId) {
        self.emit(
            "[COORD]",
            &format!(
                "commitment from {} round={}",
                short_id(&node),
                round.value()
            ),
        );
    }
    fn on_proposal(
        &self,
        proposer: NodeId,
        round: vertex_veil_core::RoundId,
        matched_capability: &str,
    ) {
        self.emit(
            "[COORD]",
            &format!(
                "proposal by {} ({}) round={}",
                short_id(&proposer),
                matched_capability,
                round.value()
            ),
        );
    }
    fn on_proof_verified(&self, node: NodeId, round: vertex_veil_core::RoundId) {
        self.emit(
            "[COORD]",
            &format!(
                "proof verified for {} round={}",
                short_id(&node),
                round.value()
            ),
        );
    }
    fn on_receipt(&self, provider: NodeId, round: vertex_veil_core::RoundId) {
        self.emit(
            "[COORD]",
            &format!(
                "receipt signed by {} round={}",
                short_id(&provider),
                round.value()
            ),
        );
    }
    fn on_abort(&self, reason: &str, round: vertex_veil_core::RoundId) {
        self.emit(
            "[ABORT]",
            &format!("{} at round={}", reason, round.value()),
        );
    }
}

fn short_id(n: &NodeId) -> String {
    let hex = n.to_hex();
    format!("{}…", &hex[..8])
}
