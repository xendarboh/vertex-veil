//! Integration tests for the public coordination artifact schema and writer.
//!
//! Covers:
//!
//! - roundtrip of a populated coordination log
//! - structural absence of private witness markers in serialized artifacts
//! - duplicate-commitment rejection per (node, round)
//! - output-path validation (empty, traversal, file-not-dir)
//! - no-clobber guarantee for existing coordination logs

use std::fs;
use std::path::PathBuf;

use vertex_veil_core::{
    ArtifactError, ArtifactWriter, CapabilityTag, CommitmentRecord, CompletionReceiptRecord,
    CoordinationLog, NodeId, ProofArtifactRecord, ProposalRecord, PublicIntent, RoundId,
    VerifierReport,
};

fn node(byte: u8) -> NodeId {
    NodeId::from_bytes([byte; 32])
}

fn sample_intent() -> PublicIntent {
    PublicIntent::Requester {
        node_id: node(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    }
}

fn populated_log() -> CoordinationLog {
    let mut log = CoordinationLog::new("run-0");
    log.append_commitment(CommitmentRecord {
        node_id: node(0x11),
        round: RoundId::new(0),
        commitment_hex: "ab".repeat(16),
        public_intent: sample_intent(),
    })
    .unwrap();
    log.append_proposal(ProposalRecord {
        proposer: node(0x22),
        round: RoundId::new(0),
        candidate_requester: node(0x11),
        candidate_provider: node(0x22),
        matched_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    });
    log.append_proof(ProofArtifactRecord {
        node_id: node(0x22),
        round: RoundId::new(0),
        public_inputs_hex: "cd".repeat(4),
        proof_hex: "ef".repeat(8),
    });
    log.append_receipt(CompletionReceiptRecord {
        provider: node(0x22),
        round: RoundId::new(0),
        signature_hex: "f0".repeat(16),
    });
    log
}

#[test]
fn artifacts_coordination_log_roundtrip_is_deterministic() {
    let log = populated_log();
    let a = serde_json::to_string(&log).unwrap();
    let b = serde_json::to_string(&log).unwrap();
    assert_eq!(a, b);

    let mut back: CoordinationLog = serde_json::from_str(&a).unwrap();
    back.reindex().unwrap();
    assert_eq!(back, log);
}

#[test]
fn artifacts_serialized_log_contains_no_private_markers() {
    let log = populated_log();
    let json = serde_json::to_string(&log).unwrap();
    for forbidden in [
        "budget_cents",
        "reservation_cents",
        "[REDACTED]",
        "witness",
    ] {
        assert!(
            !json.contains(forbidden),
            "serialized log leaked marker {forbidden:?}"
        );
    }
}

#[test]
fn artifacts_reject_duplicate_commitment() {
    let mut log = CoordinationLog::new("run-0");
    let c = CommitmentRecord {
        node_id: node(0x11),
        round: RoundId::new(0),
        commitment_hex: "ab".repeat(16),
        public_intent: sample_intent(),
    };
    log.append_commitment(c.clone()).unwrap();
    let err = log.append_commitment(c).unwrap_err();
    assert!(matches!(err, ArtifactError::DuplicateCommitment { round } if round == 0));
}

#[test]
fn artifacts_writer_rejects_empty_path() {
    let err = ArtifactWriter::new("").unwrap_err();
    assert!(matches!(err, ArtifactError::InvalidOutputPath(_)));
}

#[test]
fn artifacts_writer_rejects_traversal() {
    let err = ArtifactWriter::new("../escape").unwrap_err();
    assert!(matches!(err, ArtifactError::InvalidOutputPath(_)));
}

#[test]
fn artifacts_writer_rejects_existing_file_path() {
    let dir = tmp_dir("veil-existing-file");
    let file = dir.join("not-a-dir");
    fs::write(&file, b"x").unwrap();
    let err = ArtifactWriter::new(&file).unwrap_err();
    assert!(matches!(err, ArtifactError::InvalidOutputPath(_)));
}

#[test]
fn artifacts_writer_refuses_to_clobber_existing_log() {
    let dir = tmp_dir("veil-no-clobber");
    let writer = ArtifactWriter::new(&dir).unwrap();
    writer.write_coordination_log(&populated_log()).unwrap();
    let err = writer.write_coordination_log(&populated_log()).unwrap_err();
    assert!(matches!(err, ArtifactError::DirectoryNotEmpty(_)));
}

#[test]
fn artifacts_writer_writes_report_alongside_log() {
    let dir = tmp_dir("veil-writes-report");
    let writer = ArtifactWriter::new(&dir).unwrap();
    let log_path = writer.write_coordination_log(&populated_log()).unwrap();
    let report = VerifierReport {
        run_id: "run-0".into(),
        valid: true,
        final_round: RoundId::new(0),
        reasons: vec!["placeholder verification".into()],
    };
    let report_path = writer.write_verifier_report(&report).unwrap();
    assert!(log_path.exists());
    assert!(report_path.exists());
}

#[test]
fn artifacts_optional_metadata_absent_is_valid() {
    let log = CoordinationLog::new("run-empty");
    let json = serde_json::to_string(&log).unwrap();
    // Empty collections serialize as `[]`.
    assert!(json.contains("\"commitments\":[]"));
    let back: CoordinationLog = serde_json::from_str(&json).unwrap();
    assert_eq!(back.commitments.len(), 0);
}

fn tmp_dir(label: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    dir.push(format!("vertex-veil-{label}-{pid}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
