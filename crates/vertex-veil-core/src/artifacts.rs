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

/// Rejection record. Captures an adversarial or malformed message that the
/// runtime refused to accept, so the coordination log carries a visible
/// forensic trace usable by the standalone verifier.
///
/// `kind` is a short machine-readable label for the rejected message class
/// (`"commitment"`, `"proposal"`, `"proof"`, `"receipt"`). `reason_code` is a
/// short machine-readable tag matching the protocol denial code family
/// (e.g. `"duplicate_commitment"`, `"replay_detected"`, `"wrong_proposer"`,
/// `"invalid_proof"`).
///
/// A rejection record never carries private witness values. When the reason
/// would otherwise echo private data, it is replaced by its tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionRecord {
    pub round: RoundId,
    pub node_id: NodeId,
    pub kind: String,
    pub reason_code: String,
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

/// Judge-facing summary artifact. Describes whether the run finalized,
/// which round ended the run, whether a completion receipt is present, the
/// optional abort reason, and the set of filenames a third party needs to
/// verify the run from public data alone.
///
/// Values never carry private witness material by construction: every field
/// is either a public identifier, a counter, or a machine-readable tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStatus {
    pub run_id: String,
    pub finalized: bool,
    pub final_round: RoundId,
    pub receipt_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<String>,
    pub rejection_count: usize,
    pub bundle_files: Vec<String>,
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
    /// Forensic trace of rejected messages. Empty on a purely happy-path run;
    /// populated when adversarial or malformed inputs were refused. Written
    /// and read with `serde(default)` so a v1 log without this field still
    /// deserializes.
    #[serde(default)]
    pub rejections: Vec<RejectionRecord>,
    /// Round at which the run ended. For a finalized happy-path run, this is
    /// the round whose proposal produced a completion receipt. For an aborted
    /// run, this is the last attempted round. Defaulted for backward compat.
    #[serde(default)]
    pub final_round: RoundId,
    /// True when the run produced a valid completion receipt on `final_round`.
    #[serde(default)]
    pub finalized: bool,
    /// Short machine-readable reason the run aborted (e.g.
    /// `"max_rounds_exceeded"`, `"requester_persistently_silent"`). Absent
    /// on a finalized run. Phase 4 addition; back-compat via serde-default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<String>,
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
            rejections: Vec::new(),
            final_round: RoundId::new(0),
            finalized: false,
            abort_reason: None,
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

    pub fn append_rejection(&mut self, r: RejectionRecord) {
        self.rejections.push(r);
    }

    pub fn set_final_round(&mut self, round: RoundId, finalized: bool) {
        self.final_round = round;
        self.finalized = finalized;
    }

    /// Record a machine-readable abort reason. Only meaningful when
    /// `finalized = false`.
    pub fn set_abort_reason(&mut self, reason: impl Into<String>) {
        self.abort_reason = Some(reason.into());
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
///   (use [`ArtifactWriter::open_versioned`] for rotate-on-exists behavior)
#[derive(Debug)]
pub struct ArtifactWriter {
    dir: PathBuf,
}

impl ArtifactWriter {
    /// Open a writer rooted at `dir`, creating the directory if needed.
    ///
    /// If the directory already contains a `coordination_log.json`, the
    /// writer refuses to clobber it. Use [`Self::open_versioned`] to rotate
    /// an existing bundle into a timestamped sibling directory before
    /// writing the new one.
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

    /// Open a writer rooted at `dir`, rotating any existing bundle to
    /// `<dir>.prev-<version>` first. Version suffix uses a monotonic
    /// `version` integer so repeated runs produce `.prev-1`, `.prev-2`, etc.
    /// Files not produced by this writer (unknown filenames) are left
    /// untouched when the directory is rotated: the whole directory is
    /// moved to the `.prev-*` sibling, preserving every file intact.
    ///
    /// Returns `(writer, rotated_to)` where `rotated_to` is `Some(path)`
    /// when a rotation happened.
    pub fn open_versioned(
        dir: impl AsRef<Path>,
    ) -> Result<(Self, Option<PathBuf>), ArtifactError> {
        let dir = dir.as_ref().to_path_buf();
        validate_output_path(&dir)?;
        let rotated_to = if dir.exists() {
            if !dir.is_dir() {
                return Err(ArtifactError::InvalidOutputPath(
                    "artifact path exists and is not a directory".into(),
                ));
            }
            // Only rotate if the directory is non-empty.
            let empty = fs::read_dir(&dir)
                .map_err(ArtifactError::io)?
                .next()
                .transpose()
                .map_err(ArtifactError::io)?
                .is_none();
            if empty {
                None
            } else {
                let parent = dir.parent().ok_or_else(|| {
                    ArtifactError::InvalidOutputPath(
                        "artifact dir has no parent for rotation".into(),
                    )
                })?;
                let stem = dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| {
                        ArtifactError::InvalidOutputPath(
                            "artifact dir has non-utf8 name".into(),
                        )
                    })?
                    .to_string();
                let mut version: u64 = 1;
                let mut target = parent.join(format!("{stem}.prev-{version}"));
                while target.exists() {
                    version += 1;
                    target = parent.join(format!("{stem}.prev-{version}"));
                }
                fs::rename(&dir, &target).map_err(ArtifactError::io)?;
                fs::create_dir_all(&dir).map_err(ArtifactError::io)?;
                Some(target)
            }
        } else {
            fs::create_dir_all(&dir).map_err(ArtifactError::io)?;
            None
        };
        Ok((ArtifactWriter { dir }, rotated_to))
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

    /// Write the judge-facing run status summary. Safe to overwrite.
    pub fn write_run_status(&self, status: &RunStatus) -> Result<PathBuf, ArtifactError> {
        let out = self.dir.join("run_status.json");
        let json = serde_json::to_string_pretty(status).map_err(ArtifactError::serialization)?;
        fs::write(&out, json).map_err(ArtifactError::io)?;
        Ok(out)
    }

    /// Write the extracted completion receipt (if any) as its own file for
    /// judge convenience. Overwriting is allowed. Returns `None` when the
    /// log contains no receipt.
    pub fn write_receipt_copy(
        &self,
        receipt: Option<&CompletionReceiptRecord>,
    ) -> Result<Option<PathBuf>, ArtifactError> {
        match receipt {
            None => Ok(None),
            Some(r) => {
                let out = self.dir.join("completion_receipt.json");
                let json = serde_json::to_string_pretty(r).map_err(ArtifactError::serialization)?;
                fs::write(&out, json).map_err(ArtifactError::io)?;
                Ok(Some(out))
            }
        }
    }

    /// Write the bundle README. Contains only public information: the
    /// verifier command, a short file manifest, and a note about the run
    /// outcome. Safe to overwrite.
    pub fn write_bundle_readme(&self, body: &str) -> Result<PathBuf, ArtifactError> {
        let out = self.dir.join("bundle_README.md");
        fs::write(&out, body).map_err(ArtifactError::io)?;
        Ok(out)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Read a coordination log from `<dir>/coordination_log.json`. Re-indexes the
/// internal commitment-key set after deserialization.
pub fn read_coordination_log(dir: &Path) -> Result<CoordinationLog, ArtifactError> {
    let path = dir.join("coordination_log.json");
    let text = fs::read_to_string(&path).map_err(ArtifactError::io)?;
    let mut log: CoordinationLog =
        serde_json::from_str(&text).map_err(ArtifactError::serialization)?;
    log.reindex()?;
    Ok(log)
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
