//! Demo and verify entry points for the CLI.
//!
//! `demo` runs a coordination round against the configured topology, applies
//! an optional adversarial scenario, and writes a public artifact bundle
//! into the artifacts directory. The bundle layout is:
//!
//! ```text
//! artifacts/<run>/
//!   coordination_log.json   # ordered public record of the run
//!   verifier_report.json    # standalone verifier's decision
//!   run_status.json         # judge-facing summary
//!   completion_receipt.json # present when the run finalized
//!   topology.toml           # copy of the topology the run used
//!   scenario.toml           # copy of the adversarial scenario, if any
//!   bundle_README.md        # how to re-verify the bundle
//! ```
//!
//! `verify` reads that bundle and re-runs the standalone verifier against
//! the persisted log, writing an updated verifier report and run status.
//!
//! By default `demo` rotates any previous bundle at the target path to
//! `<artifacts>.prev-<N>` instead of clobbering or deleting it. Unrelated
//! files inside the prior directory are preserved intact inside the rotated
//! sibling. Pass `--force` to bypass rotation and overwrite in place.

use std::fs;
use std::path::{Path, PathBuf};

use vertex_veil_core::{
    read_coordination_log, ArtifactWriter, CompletionReceiptRecord, CoordinationLog,
    CoordinationRuntime, OrderedBus, RunStatus, Scenario, StandaloneVerifier, TopologyConfig,
    VerifierReport,
};

use crate::private_intents;

/// High-level error returned by [`demo`] / [`verify`]. Keeps private witness
/// values out of surfaced error messages.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("topology load error: {0}")]
    Topology(String),
    #[error("scenario load error: {0}")]
    Scenario(String),
    #[error("private-intent load error: {0}")]
    PrivateIntent(String),
    #[error("artifact io error: {0}")]
    Io(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("verifier reports log invalid: {0}")]
    VerifierInvalid(String),
    #[error("demo run aborted without finalizing: {0}")]
    Aborted(String),
}

/// Outcome of a `demo` run surfaced to the CLI dispatcher.
#[derive(Debug)]
pub struct DemoResult {
    pub report: VerifierReport,
    pub finalized: bool,
    pub abort_reason: Option<String>,
    pub rotated_prev: Option<PathBuf>,
}

/// Configured demo arguments gathered from the CLI.
pub struct DemoArgs {
    pub topology: PathBuf,
    pub private_intents: Option<PathBuf>,
    pub scenario: Option<PathBuf>,
    pub artifacts: PathBuf,
    pub max_rounds: u64,
    pub run_id: String,
    /// Skip rotation — write directly into the target directory, overwriting
    /// any files this writer owns. Files from this writer's manifest only;
    /// unrelated files are always preserved.
    pub force: bool,
}

/// Run the demo end-to-end. Returns the full result (verifier report +
/// outcome metadata).
pub fn demo(args: DemoArgs) -> Result<DemoResult, RunError> {
    let topology = TopologyConfig::load(&args.topology)
        .map_err(|e| RunError::Topology(e.to_string()))?;

    // If no private-intent file is provided, look for a sibling
    // `<topology-stem>.private.toml` (convention) in the same directory.
    let private_path = match args.private_intents.clone() {
        Some(p) => p,
        None => default_private_path(&args.topology),
    };

    let agents = private_intents::load(&private_path, &topology)
        .map_err(|e| RunError::PrivateIntent(format!("{e}")))?;

    let scenario = match args.scenario.as_ref() {
        Some(p) => Scenario::load(p).map_err(|e| RunError::Scenario(e.to_string()))?,
        None => Scenario::empty(),
    };

    let rt = CoordinationRuntime::new(
        topology.clone(),
        OrderedBus::new(),
        agents,
        scenario.clone(),
        args.max_rounds,
    )
    .map_err(|e| RunError::Runtime(e.to_string()))?;
    let outcome = rt
        .run(args.run_id.clone())
        .map_err(|e| RunError::Runtime(e.to_string()))?;

    // Rotate any existing bundle (unless --force).
    let (writer, rotated_prev) = if args.force {
        // Clean up only the files this writer owns so unrelated files stay.
        let w = ArtifactWriter::new(&args.artifacts)
            .map_err(|e| RunError::Io(e.to_string()))?;
        force_clean_owned_files(&args.artifacts).map_err(RunError::Io)?;
        (w, None)
    } else {
        ArtifactWriter::open_versioned(&args.artifacts)
            .map_err(|e| RunError::Io(e.to_string()))?
    };

    writer
        .write_coordination_log(&outcome.log)
        .map_err(|e| RunError::Io(e.to_string()))?;

    persist_text_copy(
        writer.dir(),
        "topology.toml",
        &fs::read_to_string(&args.topology).map_err(|e| RunError::Io(e.to_string()))?,
    )?;
    if let Some(scn_path) = &args.scenario {
        let text = fs::read_to_string(scn_path).map_err(|e| RunError::Io(e.to_string()))?;
        persist_text_copy(writer.dir(), "scenario.toml", &text)?;
    }

    // Even on abort we write the log and a verifier report so the bundle
    // stays forensically useful; an aborted log is expected to verify as
    // `valid = true` (coherent abort) but carry an `abort_reason`.
    let verifier = StandaloneVerifier::new(topology);
    let report = verifier.verify_log(&outcome.log);
    writer
        .write_verifier_report(&report)
        .map_err(|e| RunError::Io(e.to_string()))?;

    // Extract the final completion receipt if present.
    let receipt = pick_final_receipt(&outcome.log);
    writer
        .write_receipt_copy(receipt)
        .map_err(|e| RunError::Io(e.to_string()))?;

    // Two-pass manifest: snapshot the directory after all owned files are
    // already on disk EXCEPT the manifest files themselves (run_status.json
    // and bundle_README.md), then include those names by construction so
    // the manifest is complete and self-referential.
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
        .map_err(|e| RunError::Io(e.to_string()))?;
    writer
        .write_bundle_readme(&render_bundle_readme(&status))
        .map_err(|e| RunError::Io(e.to_string()))?;

    Ok(DemoResult {
        report,
        finalized: outcome.finalized,
        abort_reason: outcome.log.abort_reason,
        rotated_prev,
    })
}

/// Read artifacts from a directory and re-verify them.
pub fn verify(artifacts: &Path) -> Result<VerifierReport, RunError> {
    if !artifacts.is_dir() {
        return Err(RunError::Io(format!(
            "artifacts path is not a directory: {}",
            artifacts.display()
        )));
    }
    let topology_path = artifacts.join("topology.toml");
    let topology = TopologyConfig::load(&topology_path)
        .map_err(|e| RunError::Topology(e.to_string()))?;
    let log = read_coordination_log(artifacts).map_err(|e| RunError::Io(e.to_string()))?;
    let verifier = StandaloneVerifier::new(topology);
    let report = verifier.verify_log(&log);

    // Overwrite the persisted verifier report so it always reflects the
    // latest check; also refresh run_status.json so bundle inspection stays
    // consistent.
    let writer = ArtifactWriter::new(artifacts).map_err(|e| RunError::Io(e.to_string()))?;
    writer
        .write_verifier_report(&report)
        .map_err(|e| RunError::Io(e.to_string()))?;

    let receipt = pick_final_receipt(&log);
    let mut bundle_files = list_bundle_files(writer.dir());
    for name in ["run_status.json", "bundle_README.md"] {
        if !bundle_files.iter().any(|f| f == name) {
            bundle_files.push(name.to_string());
        }
    }
    bundle_files.sort();
    let status = RunStatus {
        run_id: log.run_id.clone(),
        finalized: log.finalized,
        final_round: log.final_round,
        receipt_present: receipt.is_some(),
        abort_reason: log.abort_reason.clone(),
        rejection_count: log.rejections.len(),
        bundle_files,
    };
    writer
        .write_run_status(&status)
        .map_err(|e| RunError::Io(e.to_string()))?;
    Ok(report)
}

fn default_private_path(topology: &Path) -> PathBuf {
    let parent = topology.parent().unwrap_or_else(|| Path::new("."));
    let stem = topology
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("topology");
    parent.join(format!("{stem}.private.toml"))
}

fn persist_text_copy(dir: &Path, name: &str, text: &str) -> Result<(), RunError> {
    let path = dir.join(name);
    fs::write(&path, text).map_err(|e| RunError::Io(e.to_string()))?;
    Ok(())
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

fn list_bundle_files(dir: &Path) -> Vec<String> {
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

/// Filenames owned by this writer. [`demo --force`] overwrites these in
/// place and leaves unrelated files alone so judges can drop auxiliary
/// notes into the artifact directory without fearing data loss.
const OWNED_FILES: &[&str] = &[
    "coordination_log.json",
    "verifier_report.json",
    "run_status.json",
    "completion_receipt.json",
    "topology.toml",
    "scenario.toml",
    "bundle_README.md",
];

fn force_clean_owned_files(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for name in OWNED_FILES {
        let p = dir.join(name);
        if p.exists() {
            fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn render_bundle_readme(status: &RunStatus) -> String {
    let mut s = String::new();
    s.push_str("# Vertex Veil Run Bundle\n\n");
    s.push_str(
        "This directory contains the public coordination record for one Vertex Veil run.\n",
    );
    s.push_str("Every file here is derived from public inputs alone; no secret material\n");
    s.push_str("is required — or present — at any point.\n\n");

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
    s.push_str(&format!(
        "- receipt_present: `{}`\n",
        if status.receipt_present {
            "true"
        } else {
            "false"
        }
    ));
    if let Some(reason) = &status.abort_reason {
        s.push_str(&format!("- abort_reason: `{reason}`\n"));
    }
    s.push_str(&format!(
        "- rejections_logged: `{}`\n\n",
        status.rejection_count
    ));

    s.push_str("## Files\n\n");
    for name in &status.bundle_files {
        s.push_str(&format!("- `{name}`\n"));
    }
    s.push('\n');

    s.push_str("## Re-verify the bundle\n\n");
    s.push_str("From a freshly-cloned checkout of the `vertex-veil` workspace, point the\n");
    s.push_str("standalone verifier at this directory. The verifier reads the public\n");
    s.push_str("coordination log only; do not attach secret fixture files to the\n");
    s.push_str("bundle, and the verifier will never ask for them:\n\n");
    s.push_str("```bash\n");
    s.push_str("cargo run -p vertex-veil-agents -- verify --artifacts <path-to-this-dir>\n");
    s.push_str("```\n\n");

    s.push_str("A third party can also re-run the verifier programmatically via the\n");
    s.push_str("`vertex-veil-core::StandaloneVerifier` API, feeding it the\n");
    s.push_str("`coordination_log.json` and `topology.toml` from this bundle.\n");
    s
}
