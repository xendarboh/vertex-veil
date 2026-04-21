//! Demo and verify entry points for the CLI.
//!
//! `demo` runs a coordination round against the configured topology, applies
//! an optional adversarial scenario, and writes a public artifact bundle
//! (coordination log, topology copy, verifier report, scenario copy if
//! supplied) into the artifacts directory.
//!
//! `verify` reads the artifact bundle and re-runs the standalone verifier
//! against the persisted log, writing an updated verifier report.

use std::fs;
use std::path::{Path, PathBuf};

use vertex_veil_core::{
    read_coordination_log, ArtifactWriter, CoordinationRuntime, OrderedBus, Scenario,
    StandaloneVerifier, TopologyConfig, VerifierReport,
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
}

/// Configured demo arguments gathered from the CLI.
pub struct DemoArgs {
    pub topology: PathBuf,
    pub private_intents: Option<PathBuf>,
    pub scenario: Option<PathBuf>,
    pub artifacts: PathBuf,
    pub max_rounds: u64,
    pub run_id: String,
}

/// Run the demo end-to-end. Returns the written verifier report.
pub fn demo(args: DemoArgs) -> Result<VerifierReport, RunError> {
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
    let outcome = rt.run(args.run_id.clone()).map_err(|e| RunError::Runtime(e.to_string()))?;

    // Persist artifacts: coordination log, topology copy, scenario copy,
    // verifier report. Topology and scenario copies let the `verify`
    // subcommand re-run without access to the original fixtures.
    let writer = ArtifactWriter::new(&args.artifacts)
        .map_err(|e| RunError::Io(e.to_string()))?;
    writer
        .write_coordination_log(&outcome.log)
        .map_err(|e| RunError::Io(e.to_string()))?;

    persist_text_copy(
        &args.artifacts,
        "topology.toml",
        &fs::read_to_string(&args.topology).map_err(|e| RunError::Io(e.to_string()))?,
    )?;
    if let Some(scn_path) = &args.scenario {
        let text = fs::read_to_string(scn_path).map_err(|e| RunError::Io(e.to_string()))?;
        persist_text_copy(&args.artifacts, "scenario.toml", &text)?;
    }

    let verifier = StandaloneVerifier::new(topology);
    let report = verifier.verify_log(&outcome.log);
    writer
        .write_verifier_report(&report)
        .map_err(|e| RunError::Io(e.to_string()))?;

    Ok(report)
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
    // latest check.
    let writer = ArtifactWriter::new(artifacts).map_err(|e| RunError::Io(e.to_string()))?;
    writer
        .write_verifier_report(&report)
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
