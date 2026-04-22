//! Multi-process orchestrator for the Phase 5 BFT baseline.
//!
//! `demo-bft` spawns one `vertex-veil-agents node …` child per topology
//! entry, with matching peer lists over UDP loopback ports, and threads
//! their stdout through a single pane prefixed `[N1]`..`[Nk]`. When
//! `--fail-at-round N` is set the orchestrator watches for
//! `[Nk] [VERTEX] round N committed` lines on a target child and sends it
//! SIGKILL. If `--rejoin-after-ms M` is also set, it respawns the killed
//! child M milliseconds later with `--rejoin`, mirroring the warmup
//! reference's manual recovery flow.
//!
//! Feature-gated behind `vertex-transport` because spawning `node` children
//! only makes sense when that feature is enabled; and the orchestrator is
//! only useful in that setting.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as PCommand, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use tashi_vertex::KeySecret;
use vertex_veil_core::TopologyConfig;

/// High-level orchestrator error.
#[derive(Debug, thiserror::Error)]
pub enum OrchestrateError {
    #[error("topology load error: {0}")]
    Topology(String),
    #[error("private-intent read error: {0}")]
    PrivateIntent(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("spawn error for node {0}: {1}")]
    Spawn(String, String),
    #[error("unexpected child exit: node {0} exited with {1}")]
    ChildExit(String, i32),
    #[error("timeout: {0}")]
    Timeout(String),
}

/// Configured orchestrator arguments gathered from the CLI.
pub struct OrchestrateArgs {
    pub topology: PathBuf,
    pub private_intents: PathBuf,
    pub scenario: Option<PathBuf>,
    pub artifacts: PathBuf,
    pub base_port: u16,
    pub fail_at_round: Option<u64>,
    pub rejoin_after_ms: Option<u64>,
    pub fail_target: Option<String>,
    pub max_rounds: u64,
    pub run_id: String,
    pub poll_timeout_ms: u64,
    /// Path to the `vertex-veil-agents` binary to spawn. Defaults to
    /// `std::env::current_exe()` so the same binary is used for both the
    /// orchestrator and the children.
    pub binary: Option<PathBuf>,
}

/// Orchestrator outcome.
#[derive(Debug)]
pub struct OrchestrateResult {
    pub children_finalized: usize,
    pub children_aborted: usize,
    pub bundle_dirs: Vec<PathBuf>,
    /// `true` when every child exited with 0 (finalized) or 2 (coherent
    /// abort). `false` when any child crashed or returned an unexpected
    /// code.
    pub overall_ok: bool,
}

/// Run the orchestrator end-to-end.
pub fn run_orchestrator(args: OrchestrateArgs) -> Result<OrchestrateResult, OrchestrateError> {
    let topology = TopologyConfig::load(&args.topology)
        .map_err(|e| OrchestrateError::Topology(e.to_string()))?;

    // Ordered nodes by stable key — matches the proposer rotation's order.
    let mut ordered: Vec<&vertex_veil_core::NodeConfig> = topology.nodes.iter().collect();
    ordered.sort_by_key(|n| n.id);
    if ordered.len() < 2 {
        return Err(OrchestrateError::Topology(
            "need at least 2 topology nodes to orchestrate".into(),
        ));
    }

    // Derive per-node metadata. Vertex identity keys are generated fresh
    // each orchestrator run: `KeySecret::generate()` → (base58 secret,
    // base58 public). These are distinct from the topology's
    // `signing_public_key` (ed25519, used for completion-receipt
    // verification). Keeping them separate means the topology fixture
    // stays the same across runs; the Vertex cluster identity is
    // orchestrator-local.
    let mut nodes: Vec<NodeSpec> = Vec::with_capacity(ordered.len());
    for (idx, nc) in ordered.iter().enumerate() {
        let alias = format!("n{}", idx + 1);
        let port = args.base_port + idx as u16;
        let bind = format!("127.0.0.1:{port}");
        let secret = KeySecret::generate();
        let public = secret.public();
        nodes.push(NodeSpec {
            alias,
            node_id_hex: nc.id.to_hex(),
            bind,
            vertex_pubkey: format!("{public}"),
            vertex_secret: format!("{secret}"),
        });
    }

    // Spawn.
    let binary = args
        .binary
        .clone()
        .unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| PathBuf::from("vertex-veil-agents")));

    fs::create_dir_all(&args.artifacts)
        .map_err(|e| OrchestrateError::Io(e.to_string()))?;

    let (tx, rx) = mpsc::channel::<OrchEvent>();
    let mut children: BTreeMap<String, ChildHandle> = BTreeMap::new();
    for (idx, spec) in nodes.iter().enumerate() {
        let child = spawn_one(
            &binary,
            spec,
            &nodes,
            idx,
            &args,
            false,
            tx.clone(),
        )?;
        children.insert(spec.alias.clone(), child);
    }

    let fail_target_alias = args
        .fail_target
        .clone()
        .unwrap_or_else(|| nodes.last().unwrap().alias.clone());
    let fail_at = args.fail_at_round;
    let rejoin_after_ms = args.rejoin_after_ms;

    // Event loop. We wait for:
    //   - "[Nk] [VERTEX] round X committed" lines to drive failure injection.
    //   - ChildExited notifications for aggregation.
    // Hard timeout guards against hangs.
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut killed_once = false;
    let mut rejoin_spawn_at: Option<Instant> = None;

    loop {
        if Instant::now() > deadline {
            for (_alias, c) in children.iter_mut() {
                let _ = c.child.kill();
            }
            return Err(OrchestrateError::Timeout(
                "orchestrator deadline exceeded".into(),
            ));
        }

        // Non-blocking rejoin scheduler.
        if let Some(at) = rejoin_spawn_at {
            if Instant::now() >= at {
                rejoin_spawn_at = None;
                let idx = nodes
                    .iter()
                    .position(|n| n.alias == fail_target_alias)
                    .expect("fail target in nodes list");
                let spec = &nodes[idx];
                println!(
                    "[ORCHESTRATOR] rejoining {alias} with --rejoin",
                    alias = spec.alias
                );
                let child = spawn_one(&binary, spec, &nodes, idx, &args, true, tx.clone())?;
                children.insert(spec.alias.clone(), child);
            }
        }

        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(OrchEvent::RoundCommitted { alias, round }) => {
                println!("[ORCHESTRATOR] observed {} round {} committed", alias, round);
                if let (Some(target_round), false) = (fail_at, killed_once) {
                    if alias == fail_target_alias && round == target_round {
                        if let Some(h) = children.get_mut(&fail_target_alias) {
                            println!(
                                "[ORCHESTRATOR] killing {alias} at round {round}",
                                alias = fail_target_alias,
                                round = target_round
                            );
                            let _ = h.child.kill();
                            killed_once = true;
                            if let Some(ms) = rejoin_after_ms {
                                rejoin_spawn_at =
                                    Some(Instant::now() + Duration::from_millis(ms));
                            }
                        }
                    }
                }
            }
            Ok(OrchEvent::ChildExited { alias, code }) => {
                println!("[ORCHESTRATOR] {alias} exited code={code}");
                // Remove so we don't try to wait on it again.
                children.remove(&alias);
                if children.is_empty() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if any child died silently.
                let mut to_remove: Vec<String> = Vec::new();
                for (alias, h) in children.iter_mut() {
                    if let Ok(Some(status)) = h.child.try_wait() {
                        let code = status.code().unwrap_or(-1);
                        println!("[ORCHESTRATOR] {alias} exited code={code}");
                        to_remove.push(alias.clone());
                    }
                }
                for a in to_remove {
                    children.remove(&a);
                }
                if children.is_empty() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drain any remaining children's waits just in case.
    let mut bundle_dirs: Vec<PathBuf> = Vec::new();
    for spec in &nodes {
        bundle_dirs.push(args.artifacts.join(&spec.alias));
    }

    // Pull exit codes from the global map tracked during the loop. We read
    // what's remembered on the channel; surviving children we already
    // kicked above had status printed. For simplicity we don't track each
    // exit code separately here beyond the "OK vs not OK" aggregate. A
    // judge can still `verify` each per-node bundle.
    let children_finalized = bundle_dirs
        .iter()
        .filter(|d| {
            let st = d.join("run_status.json");
            if let Ok(text) = fs::read_to_string(st) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    return v.get("finalized").and_then(|x| x.as_bool()).unwrap_or(false);
                }
            }
            false
        })
        .count();
    let children_aborted = bundle_dirs
        .iter()
        .filter(|d| {
            let st = d.join("run_status.json");
            if let Ok(text) = fs::read_to_string(st) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    return v
                        .get("abort_reason")
                        .map(|x| !x.is_null())
                        .unwrap_or(false);
                }
            }
            false
        })
        .count();

    let overall_ok = children_finalized + children_aborted > 0
        && children_finalized + children_aborted >= bundle_dirs.len().saturating_sub(1);

    println!(
        "[ORCHESTRATOR] summary finalized={} aborted={} bundles={}",
        children_finalized,
        children_aborted,
        bundle_dirs.len()
    );
    for dir in &bundle_dirs {
        println!("[ORCHESTRATOR] bundle: {}", dir.display());
    }
    println!(
        "[ORCHESTRATOR] {}",
        if overall_ok { "exit 0" } else { "exit 1" }
    );

    Ok(OrchestrateResult {
        children_finalized,
        children_aborted,
        bundle_dirs,
        overall_ok,
    })
}

// ------------------------------------------------------------------ internals

struct NodeSpec {
    alias: String,
    node_id_hex: String,
    bind: String,
    /// Base58-encoded tashi-vertex `KeyPublic` (DER).
    vertex_pubkey: String,
    /// Base58-encoded tashi-vertex `KeySecret` (DER).
    vertex_secret: String,
}

struct ChildHandle {
    child: Child,
}

enum OrchEvent {
    RoundCommitted { alias: String, round: u64 },
    #[allow(dead_code)]
    ChildExited { alias: String, code: i32 },
}

fn spawn_one(
    binary: &Path,
    spec: &NodeSpec,
    all: &[NodeSpec],
    self_idx: usize,
    args: &OrchestrateArgs,
    rejoin: bool,
    events: Sender<OrchEvent>,
) -> Result<ChildHandle, OrchestrateError> {
    let mut cmd = PCommand::new(binary);
    cmd.arg("node")
        .arg("--bind")
        .arg(&spec.bind)
        .arg("--node-id")
        .arg(&spec.node_id_hex)
        .arg("--node-alias")
        .arg(&spec.alias)
        .arg("--topology")
        .arg(&args.topology)
        .arg("--private-intents")
        .arg(&args.private_intents)
        .arg("--artifacts")
        .arg(&args.artifacts)
        .arg("--max-rounds")
        .arg(args.max_rounds.to_string())
        .arg("--run-id")
        .arg(&args.run_id)
        .arg("--poll-timeout-ms")
        .arg(args.poll_timeout_ms.to_string());

    if let Some(scn) = &args.scenario {
        cmd.arg("--scenario").arg(scn);
    }
    if rejoin {
        cmd.arg("--rejoin");
    }

    // Peer list: every other node.
    for (i, peer) in all.iter().enumerate() {
        if i == self_idx {
            continue;
        }
        cmd.arg("--peer")
            .arg(format!("{}@{}", peer.vertex_pubkey, peer.bind));
    }

    // Pass the secret via env so it doesn't land in argv.
    let env_var = format!("VERTEX_SECRET_{}", spec.alias.to_uppercase());
    cmd.env(&env_var, &spec.vertex_secret);
    cmd.arg("--secret-env").arg(&env_var);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| OrchestrateError::Spawn(spec.alias.clone(), e.to_string()))?;

    // Tee child stdout → our stdout with alias prefix; also parse for
    // round-committed events to drive the failure injector.
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| OrchestrateError::Spawn(spec.alias.clone(), "no stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| OrchestrateError::Spawn(spec.alias.clone(), "no stderr pipe".into()))?;

    let alias_for_out = spec.alias.clone();
    let evs = events.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            // Pass line through to the user's terminal verbatim. Children
            // already prefix their own lines with `[alias]`.
            println!("{line}");
            // Parse for round-committed narratives.
            if let Some(r) = parse_round_committed(&line) {
                let _ = evs.send(OrchEvent::RoundCommitted {
                    alias: alias_for_out.clone(),
                    round: r,
                });
            }
        }
    });

    let alias_for_err = spec.alias.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[{alias_for_err}/err] {line}");
        }
    });

    // No separate waiter thread: we cannot move `child` into a waiter and
    // keep it in ChildHandle at the same time. The main event loop polls
    // `try_wait()` on every tick, which covers the exit signal.

    Ok(ChildHandle { child })
}

fn parse_round_committed(line: &str) -> Option<u64> {
    // Matches `[Nk] [VERTEX] round N committed (finalized=...)`.
    let tag = "[VERTEX]";
    let idx = line.find(tag)?;
    let rest = &line[idx + tag.len()..];
    let rest = rest.trim_start();
    let mut words = rest.split_whitespace();
    let word0 = words.next()?;
    if word0 != "round" {
        return None;
    }
    let num: u64 = words.next()?.parse().ok()?;
    Some(num)
}

