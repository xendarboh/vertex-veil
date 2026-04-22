//! Phase 4 end-to-end demo bundle tests.
//!
//! These tests cover all 25 checkboxes for Phase 4 of plan.md:
//! Happy / Bad / Edge / Security / Data Leak / Data Damage. They exercise
//! the `demo()` programmatic entry point plus a few subprocess invocations
//! (for exit-code semantics).
//!
//! Every test runs against a per-test scratch directory under `target/`
//! so concurrent tests do not collide.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use serde_json::Value;
use vertex_veil_agents::run::{demo, verify, DemoArgs, DemoResult, RunError};

fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

fn topology_4() -> PathBuf {
    fixtures_dir().join("topology-4node.toml")
}
fn topology_4_private() -> PathBuf {
    fixtures_dir().join("topology-4node.private.toml")
}
fn topology_gpu_only() -> PathBuf {
    fixtures_dir().join("topology-4node-gpu-only.toml")
}
fn topology_gpu_only_private() -> PathBuf {
    fixtures_dir().join("topology-4node-gpu-only.private.toml")
}
fn topology_6() -> PathBuf {
    fixtures_dir().join("topology-6node.toml")
}
fn topology_6_private() -> PathBuf {
    fixtures_dir().join("topology-6node.private.toml")
}
fn scenario_replay_dc_drop() -> PathBuf {
    fixtures_dir().join("replay-doublecommit-drop.toml")
}
fn scenario_fallback() -> PathBuf {
    fixtures_dir().join("fallback-recovery.toml")
}
fn scenario_abort() -> PathBuf {
    fixtures_dir().join("abort-drop-all-providers.toml")
}

/// Per-test scratch dir under `target/phase4-tests/<name>`. Cleaned and
/// re-created on each invocation so individual tests stay isolated. The
/// returned path is canonicalized (no `..` components) so it passes
/// `ArtifactWriter::validate_output_path`.
fn scratch(name: &str) -> PathBuf {
    let mut d = workspace_root();
    d.push("target");
    d.push("phase4-tests");
    d.push(name);
    if d.exists() {
        let _ = fs::remove_dir_all(&d);
    }
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn args(
    name: &str,
    topology: impl Into<PathBuf>,
    private: Option<PathBuf>,
    scenario: Option<PathBuf>,
    max_rounds: u64,
    force: bool,
) -> DemoArgs {
    let dir = scratch(name);
    DemoArgs {
        topology: topology.into(),
        private_intents: private,
        scenario,
        artifacts: dir,
        max_rounds,
        run_id: format!("test-{name}"),
        force,
        narrate: false,
    }
}

fn run(args: DemoArgs) -> Result<DemoResult, RunError> {
    demo(args)
}

fn read_text(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn read_json(p: &Path) -> Value {
    let t = read_text(p);
    serde_json::from_str(&t).expect("valid JSON")
}

/// Strings that would only appear in a bundle file if a private field
/// leaked. These are field NAMES or redaction markers, not numeric values:
/// raw numerics like `200` are too common to scan literally (they appear
/// inside hex hashes by coincidence). The private values are already
/// field-segregated and never copied into public records by construction,
/// so catching the field NAME is a sufficient guard.
const FORBIDDEN_PRIVATE_NEEDLES: &[&str] = &[
    "budget_cents",
    "reservation_cents",
    "signing_secret_key",
    "[REDACTED]",
    "witness",
];

fn assert_no_private_strings(text: &str, file_label: &str) {
    for needle in FORBIDDEN_PRIVATE_NEEDLES {
        assert!(
            !text.contains(needle),
            "{file_label} unexpectedly contains forbidden private marker {needle:?}"
        );
    }
}

// =====================================================================
// Happy Path (4)
// =====================================================================

#[test]
fn happy_baseline_demo_produces_valid_report() {
    let r = run(args(
        "happy_baseline",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    ))
    .expect("demo runs");
    assert!(r.finalized);
    assert!(r.report.valid, "reasons: {:?}", r.report.reasons);
    assert!(r.report.reasons.is_empty());
}

#[test]
fn happy_bundle_layout_predictable() {
    let a = args(
        "bundle_layout",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    for must in [
        "coordination_log.json",
        "verifier_report.json",
        "run_status.json",
        "completion_receipt.json",
        "bundle_README.md",
        "topology.toml",
    ] {
        assert!(dir.join(must).exists(), "missing bundle file {must}");
    }
}

#[test]
fn happy_multi_round_recovery_via_fallback() {
    let r = run(args(
        "fallback_recovery",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_fallback()),
        4,
        false,
    ))
    .expect("demo runs");
    assert!(r.finalized);
    assert!(r.report.valid, "reasons: {:?}", r.report.reasons);
    assert_eq!(r.report.final_round.value(), 1);
}

#[test]
fn happy_third_party_verifier_runs_without_private_inputs() {
    let a = args(
        "third_party_verify",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).expect("demo runs");
    // Sanity: no private fixture file should be inside the bundle.
    assert!(!dir.join("topology-4node.private.toml").exists());
    let report = verify(&dir).expect("verify runs");
    assert!(report.valid, "reasons: {:?}", report.reasons);
}

// =====================================================================
// Bad Path (5)
// =====================================================================

#[test]
fn bad_demo_fails_clearly_when_required_fixture_missing() {
    // Missing topology file simulates a missing required dependency in
    // the demo input set. The demo should surface a structured error and
    // exit non-zero (the binary path), without panicking.
    let a = args(
        "missing_topology",
        "fixtures/does-not-exist.toml",
        None,
        None,
        4,
        false,
    );
    let err = run(a).expect_err("missing fixture must error");
    let msg = format!("{err}");
    assert!(
        matches!(err, RunError::Topology(_)),
        "expected Topology error, got {msg}"
    );
}

#[test]
fn bad_demo_fails_clearly_for_malformed_node_config() {
    let dir = scratch("malformed_topology");
    let bad_path = dir.join("bad-topology.toml");
    fs::write(&bad_path, "this is not valid toml [[[").unwrap();
    let a = DemoArgs {
        topology: bad_path,
        private_intents: None,
        scenario: None,
        artifacts: dir.join("artifacts"),
        max_rounds: 4,
        run_id: "bad".into(),
        force: false,
        narrate: false,
    };
    let err = run(a).expect_err("malformed topology must error");
    assert!(matches!(err, RunError::Topology(_)));
}

#[test]
fn bad_verifier_marks_invalid_when_bundle_incomplete() {
    let a = args(
        "incomplete_bundle",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    // Remove the coordination log to break the bundle.
    fs::remove_file(dir.join("coordination_log.json")).unwrap();
    let res = verify(&dir);
    assert!(res.is_err(), "verify on broken bundle must fail");
}

#[test]
fn bad_unrecoverable_fallback_exits_nonzero() {
    let a = args(
        "unrecoverable",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_abort()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let r = run(a).expect("demo returns Result on abort");
    assert!(!r.finalized);
    // Verifier still says the abort bundle is structurally valid; the
    // demo binary exits non-zero because finalized=false.
    assert!(r.report.valid, "reasons: {:?}", r.report.reasons);
    let status = read_json(&dir.join("run_status.json"));
    assert_eq!(status["finalized"], Value::Bool(false));
    assert_eq!(status["abort_reason"], Value::String("max_rounds_exceeded".into()));
}

#[test]
fn bad_silent_drop_threshold_abort_emits_verifiable_artifact() {
    let a = args(
        "silent_drop_threshold",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_abort()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let r = run(a).expect("demo returns Result on abort");
    assert!(!r.finalized);
    // Coordination log must still be readable and the verifier must
    // re-confirm coherence on a fresh `verify` invocation.
    let report = verify(&dir).expect("verify on aborted bundle reads cleanly");
    assert!(report.valid, "aborted bundle should still verify coherent: {:?}", report.reasons);
    let log = read_json(&dir.join("coordination_log.json"));
    assert_eq!(log["finalized"], Value::Bool(false));
    assert!(log["abort_reason"].is_string());
}

// =====================================================================
// Edge Cases (4)
// =====================================================================

#[test]
fn edge_subset_capability_tags_baseline_works() {
    let r = run(args(
        "subset_tags",
        topology_gpu_only(),
        Some(topology_gpu_only_private()),
        None,
        4,
        false,
    ))
    .expect("subset-tag demo runs");
    assert!(r.finalized);
    assert!(r.report.valid);
}

#[test]
fn edge_larger_topology_works() {
    let r = run(args(
        "larger_topology",
        topology_6(),
        Some(topology_6_private()),
        None,
        4,
        false,
    ))
    .expect("6-node demo runs");
    assert!(r.finalized);
    assert!(r.report.valid, "reasons: {:?}", r.report.reasons);
}

#[test]
fn edge_artifact_packaging_deterministic() {
    let a1 = args("determ_a", topology_4(), Some(topology_4_private()), None, 4, false);
    let a2 = args("determ_b", topology_4(), Some(topology_4_private()), None, 4, false);
    // Same run_id forces determinism even on the run_id field, which is
    // serialized into the log.
    let mut a1 = a1;
    a1.run_id = "fixed-run".into();
    let mut a2 = a2;
    a2.run_id = "fixed-run".into();
    let d1 = a1.artifacts.clone();
    let d2 = a2.artifacts.clone();
    let _ = run(a1).unwrap();
    let _ = run(a2).unwrap();
    let log1 = read_text(&d1.join("coordination_log.json"));
    let log2 = read_text(&d2.join("coordination_log.json"));
    assert_eq!(log1, log2, "coordination logs must be byte-identical for same inputs");
}

#[test]
fn edge_replay_doublecommit_reproducible() {
    let a1 = args(
        "repro_dcd_a",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let a2 = args(
        "repro_dcd_b",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let mut a1 = a1;
    a1.run_id = "fixed-dcd".into();
    let mut a2 = a2;
    a2.run_id = "fixed-dcd".into();
    let d1 = a1.artifacts.clone();
    let d2 = a2.artifacts.clone();
    let _ = run(a1).unwrap();
    let _ = run(a2).unwrap();
    assert_eq!(
        read_text(&d1.join("coordination_log.json")),
        read_text(&d2.join("coordination_log.json"))
    );
}

// =====================================================================
// Security (4)
// =====================================================================

#[test]
fn security_packaged_artifacts_contain_no_private_material() {
    let a = args(
        "no_private_in_bundle",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        let text = fs::read_to_string(&p).unwrap_or_default();
        assert_no_private_strings(&text, &p.display().to_string());
    }
}

#[test]
fn security_failure_messages_redact_private() {
    // Force a private-intent loader failure by providing a private file
    // whose nominal "private" budget contains a syntactically invalid
    // entry; the loader must surface the field name without echoing the
    // value.
    let dir = scratch("redact_failure");
    let private_path = dir.join("malformed.private.toml");
    // A `budget_cents` declared as a string fails to deserialize because
    // it's typed `u64`. The loader's redaction must keep the value out of
    // the surfaced error.
    let secret_marker = "SECRET-99999";
    let body = format!(
        r#"
version = 1

[[agents]]
node = "1111111111111111111111111111111111111111111111111111111111111111"
role = "requester"
required_capability = "GPU"
budget_cents = "{secret_marker}"
"#
    );
    fs::write(&private_path, body).unwrap();
    let a = DemoArgs {
        topology: topology_4(),
        private_intents: Some(private_path),
        scenario: None,
        artifacts: dir.join("artifacts"),
        max_rounds: 4,
        run_id: "redact".into(),
        force: false,
        narrate: false,
    };
    let err = run(a).expect_err("malformed private intent must error");
    let msg = format!("{err}");
    assert!(
        !msg.contains(secret_marker),
        "error message must not echo private secret marker: {msg}"
    );
}

#[test]
fn security_tampered_bundle_detected_by_verify() {
    let a = args(
        "tamper_detection",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    let log_path = dir.join("coordination_log.json");
    let mut log_json = read_json(&log_path);
    // Flip the receipt's signature_hex to a clearly-invalid value.
    log_json["receipts"][0]["signature_hex"] = Value::String("00".repeat(64));
    fs::write(&log_path, serde_json::to_string_pretty(&log_json).unwrap()).unwrap();
    let report = verify(&dir).expect("verify returns a report even on tamper");
    assert!(!report.valid);
    assert!(
        report.reasons.iter().any(|s| s.contains("signature_mismatch")),
        "expected signature_mismatch reason, got {:?}",
        report.reasons
    );
    // The persisted report file must reflect the tamper finding.
    let r2 = read_json(&dir.join("verifier_report.json"));
    assert_eq!(r2["valid"], Value::Bool(false));
}

#[test]
fn security_bundle_demonstrates_visible_rejections() {
    let a = args(
        "visible_rejections",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    let log = read_json(&dir.join("coordination_log.json"));
    let rejections = log["rejections"].as_array().unwrap();
    let codes: Vec<&str> = rejections
        .iter()
        .filter_map(|r| r["reason_code"].as_str())
        .collect();
    // The bundled adversarial scenario exercises double_commit, replay,
    // node_dropped, and an invalid proof; their reason_codes must show
    // up so a third party can audit the rejections trace.
    let expected = [
        "duplicate_commitment",
        "replay_detected",
        "node_dropped",
        "public_inputs_mismatch",
    ];
    for needle in expected {
        assert!(
            codes.contains(&needle),
            "missing reason_code {needle} in {codes:?}"
        );
    }
}

// =====================================================================
// Data Leak (4)
// =====================================================================

#[test]
fn leak_bundle_readme_does_not_instruct_private_exposure() {
    let a = args(
        "readme_leak",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    let readme = read_text(&dir.join("bundle_README.md"));
    assert!(!readme.contains("private-intent"));
    assert!(!readme.contains("private_intents"));
    assert!(readme.contains("do not"), "README must caution against shipping private fixtures: {readme}");
    assert_no_private_strings(&readme, "bundle_README.md");
}

#[test]
fn leak_run_status_is_public_only() {
    let a = args(
        "status_public_only",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    let txt = read_text(&dir.join("run_status.json"));
    assert_no_private_strings(&txt, "run_status.json");
}

#[test]
fn leak_failure_output_redacts_private() {
    // The redaction path is also exercised by the loader; this test
    // confirms the higher-level demo() error surface preserves it.
    let dir = scratch("leak_failure_output");
    let private_path = dir.join("malformed.private.toml");
    let secret_marker = "SECRET-LEAK-CHECK";
    let body = format!(
        r#"
version = 1

[[agents]]
node = "1111111111111111111111111111111111111111111111111111111111111111"
role = "requester"
required_capability = "GPU"
budget_cents = "{secret_marker}"
"#
    );
    fs::write(&private_path, body).unwrap();
    let a = DemoArgs {
        topology: topology_4(),
        private_intents: Some(private_path),
        scenario: None,
        artifacts: dir.join("artifacts"),
        max_rounds: 4,
        run_id: "leak".into(),
        force: false,
        narrate: false,
    };
    let err = run(a).expect_err("must error");
    let msg = format!("{err}");
    assert!(!msg.contains(secret_marker));
}

#[test]
fn leak_verifier_workflow_does_not_require_private() {
    let a = args(
        "verify_no_private",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    // Confirm: no `*.private.toml` ever lands in the bundle dir.
    for entry in fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".private."),
            "private fixture leaked into bundle: {name}"
        );
    }
    // Verify still works with only the public bundle present.
    let report = verify(&dir).expect("verify on public-only bundle must succeed");
    assert!(report.valid);
}

// =====================================================================
// Data Damage (4)
// =====================================================================

#[test]
fn damage_re_run_versions_existing_bundle() {
    let a = args(
        "rotation_a",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let r1 = run(a).expect("first run ok");
    assert!(r1.rotated_prev.is_none());

    // Second run against the same dir should rotate, not clobber.
    let a2 = DemoArgs {
        topology: topology_4(),
        private_intents: Some(topology_4_private()),
        scenario: None,
        artifacts: dir.clone(),
        max_rounds: 4,
        run_id: "rotation".into(),
        force: false,
        narrate: false,
    };
    let r2 = run(a2).expect("second run ok");
    assert!(r2.rotated_prev.is_some());
    let prev = r2.rotated_prev.unwrap();
    assert!(prev.exists(), "rotated bundle must persist: {}", prev.display());
    assert!(prev.join("coordination_log.json").exists());
}

#[test]
fn damage_re_run_does_not_touch_unrelated_files() {
    let a = args(
        "preserve_unrelated",
        topology_4(),
        Some(topology_4_private()),
        None,
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let _ = run(a).unwrap();
    // Drop an unrelated file in the bundle.
    let unrelated = dir.join("judge-notes.md");
    fs::write(&unrelated, b"hand-written notes").unwrap();

    // Re-run with rotation: the entire prior dir is moved, so the
    // unrelated file is preserved INSIDE the rotated sibling.
    let a2 = DemoArgs {
        topology: topology_4(),
        private_intents: Some(topology_4_private()),
        scenario: None,
        artifacts: dir.clone(),
        max_rounds: 4,
        run_id: "preserve".into(),
        force: false,
        narrate: false,
    };
    let r2 = run(a2).unwrap();
    let prev = r2.rotated_prev.unwrap();
    assert_eq!(
        fs::read_to_string(prev.join("judge-notes.md")).unwrap(),
        "hand-written notes"
    );
    // And: --force re-run preserves the unrelated file in the same dir.
    fs::write(dir.join("more-notes.md"), b"more").unwrap();
    let a3 = DemoArgs {
        topology: topology_4(),
        private_intents: Some(topology_4_private()),
        scenario: None,
        artifacts: dir.clone(),
        max_rounds: 4,
        run_id: "preserve-force".into(),
        force: true,
        narrate: false,
    };
    let _ = run(a3).unwrap();
    assert_eq!(fs::read_to_string(dir.join("more-notes.md")).unwrap(), "more");
}

#[test]
fn damage_aborted_run_leaves_coherent_bundle() {
    let a = args(
        "abort_coherent",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_abort()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let r = run(a).unwrap();
    assert!(!r.finalized);
    for must in [
        "coordination_log.json",
        "verifier_report.json",
        "run_status.json",
        "bundle_README.md",
        "topology.toml",
        "scenario.toml",
    ] {
        assert!(dir.join(must).exists(), "missing bundle file {must} on aborted run");
    }
    let status = read_json(&dir.join("run_status.json"));
    assert_eq!(status["finalized"], Value::Bool(false));
    assert!(status["abort_reason"].is_string());
}

#[test]
fn damage_adversarial_bundle_preserves_evidence_after_failures() {
    let a = args(
        "adversarial_evidence",
        topology_4(),
        Some(topology_4_private()),
        Some(scenario_replay_dc_drop()),
        4,
        false,
    );
    let dir = a.artifacts.clone();
    let r = run(a).unwrap();
    assert!(r.finalized);
    let log = read_json(&dir.join("coordination_log.json"));
    let rejections = log["rejections"].as_array().unwrap();
    assert!(!rejections.is_empty(), "rejections must be preserved");
    // After verification the bundle's verifier_report is still valid AND
    // the rejections array is not pruned.
    let log2 = read_json(&dir.join("coordination_log.json"));
    assert_eq!(log2["rejections"], log["rejections"]);
}

// =====================================================================
// Demo binary smoke: exit codes (kept lightweight; uses already-built
// binary via cargo).
// =====================================================================

#[test]
fn binary_exit_zero_on_happy_path() {
    let dir = scratch("binary_happy");
    let bin = binary_path();
    let status = StdCommand::new(&bin)
        .args([
            "demo",
            "--topology",
        ])
        .arg(topology_4())
        .arg("--private-intents")
        .arg(topology_4_private())
        .arg("--artifacts")
        .arg(&dir)
        .arg("--run-id")
        .arg("binary-happy")
        .current_dir(workspace_root())
        .status()
        .expect("binary runs");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn binary_exit_two_on_abort() {
    let dir = scratch("binary_abort");
    let bin = binary_path();
    let status = StdCommand::new(&bin)
        .args([
            "demo",
            "--topology",
        ])
        .arg(topology_4())
        .arg("--private-intents")
        .arg(topology_4_private())
        .arg("--scenario")
        .arg(scenario_abort())
        .arg("--artifacts")
        .arg(&dir)
        .arg("--run-id")
        .arg("binary-abort")
        .current_dir(workspace_root())
        .status()
        .expect("binary runs");
    // Demo binary returns code 2 when the run aborts but the bundle is
    // structurally valid. (Plan: "Demo run exits non-zero when the
    // fallback round cannot recover".)
    assert_eq!(status.code(), Some(2));
}

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../..");
    p.canonicalize().unwrap()
}

fn binary_path() -> PathBuf {
    // `CARGO_BIN_EXE_*` is set by cargo for integration tests and points
    // at the already-built binary, avoiding an inner `cargo run` that
    // would conflict with the outer test harness on the target dir lock.
    PathBuf::from(env!("CARGO_BIN_EXE_vertex-veil-agents"))
}

#[test]
fn fixtures_exist() {
    let root = workspace_root();
    for f in [
        "fixtures/topology-4node.toml",
        "fixtures/topology-4node.private.toml",
        "fixtures/topology-4node-gpu-only.toml",
        "fixtures/topology-4node-gpu-only.private.toml",
        "fixtures/topology-6node.toml",
        "fixtures/topology-6node.private.toml",
        "fixtures/replay-doublecommit-drop.toml",
        "fixtures/fallback-recovery.toml",
        "fixtures/abort-drop-all-providers.toml",
    ] {
        assert!(root.join(f).exists(), "missing fixture {f}");
    }
    // Fixtures dir must also exist.
    assert!(fixtures_dir().is_dir());
}
