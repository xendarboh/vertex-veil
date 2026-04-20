//! Public coordination artifact schema.
//!
//! The [`CoordinationLog`] is the canonical ordered record of a Vertex Veil
//! run. It is built exclusively from public types ([`NodeId`],
//! [`CapabilityTag`], [`PublicIntent`], [`RoundId`], and the record structs
//! defined here). Private witness material is absent by construction: no
//! field in this module has the type [`crate::private_intent::Secret`], and
//! the serialization roundtrip tests assert that on the wire as well.
//!
//! [`ArtifactWriter`] is a thin, deterministic filesystem writer that
//! produces JSON files under a run directory. It validates output paths and
//! refuses to clobber an existing coordination log.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityTag;
use crate::error::ArtifactError;
use crate::keys::NodeId;
use crate::shared_types::{PublicIntent, RoundId};

/// Commitment record: a node's opaque commitment to its private intent for a
/// specific round, plus the public intent payload the commitment binds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitmentRecord {
    pub node_id: NodeId,
    pub round: RoundId,
    /// Hex-encoded opaque commitment bytes. The construction is fixed in
    /// Phase 1; Phase 0 only requires a serializable shape.
    pub commitment_hex: String,
    pub public_intent: PublicIntent,
}

/// Proposal record emitted by the round proposer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalRecord {
    pub proposer: NodeId,
    pub round: RoundId,
    pub candidate_requester: NodeId,
    pub candidate_provider: NodeId,
    pub matched_capability: CapabilityTag,
}

/// Proof artifact produced by a Noir prover. Phase 2 will populate the proof
/// bytes and public inputs; Phase 0 only requires the public shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofArtifactRecord {
    pub node_id: NodeId,
    pub round: RoundId,
    /// Hex-encoded public inputs fed to the circuit.
    pub public_inputs_hex: String,
    /// Hex-encoded proof bytes.
    pub proof_hex: String,
}

/// Signed completion receipt published by the matched provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionReceiptRecord {
    pub provider: NodeId,
    pub round: RoundId,
    /// Hex-encoded signature bytes.
    pub signature_hex: String,
}

/// Verifier report. Emitted by a standalone third-party verifier that reads
/// the public coordination log and decides whether the run is valid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierReport {
    pub run_id: String,
    pub valid: bool,
    pub final_round: RoundId,
    pub reasons: Vec<String>,
}

/// Canonical ordered public coordination record for one run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationLog {
    pub run_id: String,
    pub schema_version: u32,
    pub commitments: Vec<CommitmentRecord>,
    pub proposals: Vec<ProposalRecord>,
    pub proofs: Vec<ProofArtifactRecord>,
    pub receipts: Vec<CompletionReceiptRecord>,
    #[serde(skip_serializing, skip_deserializing)]
    commitment_keys: BTreeSet<(NodeId, RoundId)>,
}

impl CoordinationLog {
    pub fn new(run_id: impl Into<String>) -> Self {
        CoordinationLog {
            run_id: run_id.into(),
            schema_version: 1,
            commitments: Vec::new(),
            proposals: Vec::new(),
            proofs: Vec::new(),
            receipts: Vec::new(),
            commitment_keys: BTreeSet::new(),
        }
    }

    /// Append a commitment. Returns [`ArtifactError::DuplicateCommitment`] if
    /// the same `(node_id, round)` pair has already been recorded.
    pub fn append_commitment(&mut self, c: CommitmentRecord) -> Result<(), ArtifactError> {
        let key = (c.node_id, c.round);
        if !self.commitment_keys.insert(key) {
            return Err(ArtifactError::DuplicateCommitment {
                round: c.round.value(),
            });
        }
        self.commitments.push(c);
        Ok(())
    }

    pub fn append_proposal(&mut self, p: ProposalRecord) {
        self.proposals.push(p);
    }

    pub fn append_proof(&mut self, p: ProofArtifactRecord) {
        self.proofs.push(p);
    }

    pub fn append_receipt(&mut self, r: CompletionReceiptRecord) {
        self.receipts.push(r);
    }

    /// Recompute internal commitment-key index from the public vector.
    /// Needed after deserialization because the index is not persisted.
    pub fn reindex(&mut self) -> Result<(), ArtifactError> {
        self.commitment_keys.clear();
        for c in &self.commitments {
            let key = (c.node_id, c.round);
            if !self.commitment_keys.insert(key) {
                return Err(ArtifactError::DuplicateCommitment {
                    round: c.round.value(),
                });
            }
        }
        Ok(())
    }
}

/// Filesystem writer for public coordination artifacts.
///
/// Writes JSON files into a run directory. The writer:
///
/// - rejects parent-directory traversal in output paths
/// - rejects empty or whitespace-only path components
/// - refuses to overwrite an existing `coordination_log.json`
#[derive(Debug)]
pub struct ArtifactWriter {
    dir: PathBuf,
}

impl ArtifactWriter {
    /// Open a writer rooted at `dir`, creating the directory if needed.
    pub fn new(dir: impl AsRef<Path>) -> Result<Self, ArtifactError> {
        let dir = dir.as_ref().to_path_buf();
        validate_output_path(&dir)?;
        if dir.exists() {
            if !dir.is_dir() {
                return Err(ArtifactError::InvalidOutputPath(
                    "artifact path exists and is not a directory".into(),
                ));
            }
        } else {
            fs::create_dir_all(&dir).map_err(ArtifactError::io)?;
        }
        Ok(ArtifactWriter { dir })
    }

    /// Write the coordination log, refusing to clobber an existing run.
    pub fn write_coordination_log(&self, log: &CoordinationLog) -> Result<PathBuf, ArtifactError> {
        let out = self.dir.join("coordination_log.json");
        if out.exists() {
            return Err(ArtifactError::DirectoryNotEmpty(out.display().to_string()));
        }
        let json = serde_json::to_string_pretty(log).map_err(ArtifactError::serialization)?;
        fs::write(&out, json).map_err(ArtifactError::io)?;
        Ok(out)
    }

    /// Write the verifier report alongside the coordination log. Overwriting
    /// an existing report is allowed because the verifier is re-runnable.
    pub fn write_verifier_report(&self, report: &VerifierReport) -> Result<PathBuf, ArtifactError> {
        let out = self.dir.join("verifier_report.json");
        let json = serde_json::to_string_pretty(report).map_err(ArtifactError::serialization)?;
        fs::write(&out, json).map_err(ArtifactError::io)?;
        Ok(out)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn validate_output_path(path: &Path) -> Result<(), ArtifactError> {
    if path.as_os_str().is_empty() {
        return Err(ArtifactError::InvalidOutputPath("empty path".into()));
    }
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(ArtifactError::InvalidOutputPath(
                    "path must not contain '..' segments".into(),
                ));
            }
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.trim().is_empty() {
                    return Err(ArtifactError::InvalidOutputPath(
                        "path contains empty or whitespace-only segment".into(),
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node(byte: u8) -> NodeId {
        NodeId::from_bytes([byte; 32])
    }

    fn sample_intent() -> PublicIntent {
        PublicIntent::Requester {
            node_id: sample_node(0x11),
            round: RoundId::new(0),
            required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        }
    }

    #[test]
    fn rejects_duplicate_commitment_same_node_same_round() {
        let mut log = CoordinationLog::new("run-0");
        let c = CommitmentRecord {
            node_id: sample_node(0x11),
            round: RoundId::new(0),
            commitment_hex: "ab".repeat(16),
            public_intent: sample_intent(),
        };
        log.append_commitment(c.clone()).unwrap();
        let err = log.append_commitment(c).unwrap_err();
        assert!(matches!(err, ArtifactError::DuplicateCommitment { .. }));
    }

    #[test]
    fn serialized_log_contains_no_private_markers() {
        let mut log = CoordinationLog::new("run-0");
        log.append_commitment(CommitmentRecord {
            node_id: sample_node(0x11),
            round: RoundId::new(0),
            commitment_hex: "ab".repeat(16),
            public_intent: sample_intent(),
        })
        .unwrap();

        let json = serde_json::to_string(&log).unwrap();
        for forbidden in [
            "budget_cents",
            "reservation_cents",
            "[REDACTED]",
            "witness",
        ] {
            assert!(
                !json.contains(forbidden),
                "serialized log must not contain {forbidden:?}"
            );
        }
    }

    #[test]
    fn output_path_rejects_traversal() {
        let err = ArtifactWriter::new("../escape").unwrap_err();
        assert!(matches!(err, ArtifactError::InvalidOutputPath(_)));
    }
}
