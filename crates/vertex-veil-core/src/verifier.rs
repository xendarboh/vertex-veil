//! Standalone Phase 3 verifier.
//!
//! Reads a saved [`crate::CoordinationLog`] and a [`crate::TopologyConfig`]
//! and produces a [`crate::VerifierReport`] describing whether the run is
//! structurally sound. The verifier reads only the public coordination
//! record: no private witness inputs, no secret files, no network access.
//!
//! The verifier enforces the same invariants the runtime enforced locally,
//! independently, so tampering between runtime and log surfaces here:
//!
//! - schema version is supported
//! - every commitment's node id is configured in the topology and its role
//!   matches
//! - no `(node_id, round)` pair appears more than once in commitments
//! - every proposal is produced by the correct deterministic proposer for
//!   its round and its annotation survives `validate_proposal_annotation`
//! - the final round's predicate holds between the requester and matched
//!   provider
//! - every proof artifact's embedded public inputs match the logged
//!   commitment's commitment_hash, node_id, and round, and the role byte
//!   matches the commitment's public intent
//! - the completion receipt on the final round is present, carries the
//!   matched provider id, and its signature matches the deterministic tag
//!   the runtime would have emitted
//! - rejections are structurally consistent: their round values fit the
//!   log's span
//!
//! Tampering any one of these invariants produces a [`VerifierReport`] with
//! `valid = false` and a descriptive reason. The verifier never panics on
//! malformed input; corruption surfaces as a report, not a crash.

use crate::artifacts::{CoordinationLog, VerifierReport};
use crate::config::TopologyConfig;
use crate::keys::NodeId;
use crate::predicate::validate_proposal_annotation;
use crate::runtime::{deterministic_proposer, expected_signature_hex, parse_public_inputs, predicate_holds_for_log};
use crate::shared_types::{PublicIntent, RoundId};

/// Error returned when verification cannot even start — for example if the
/// input log cannot be read. Per-invariant failures are returned as a
/// [`VerifierReport`] with `valid = false` instead.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifierError {
    #[error("verifier startup error: {0}")]
    Startup(String),
}

/// Phase 3 standalone verifier.
pub struct StandaloneVerifier {
    topology: TopologyConfig,
}

impl StandaloneVerifier {
    pub fn new(topology: TopologyConfig) -> Self {
        StandaloneVerifier { topology }
    }

    pub fn topology(&self) -> &TopologyConfig {
        &self.topology
    }

    /// Verify a coordination log. Always returns a report; even hard
    /// structural problems like an unsupported schema version become
    /// `valid = false` reasons.
    pub fn verify_log(&self, log: &CoordinationLog) -> VerifierReport {
        let mut reasons: Vec<String> = Vec::new();

        // Schema version.
        if log.schema_version != 1 {
            reasons.push(format!(
                "unsupported_schema_version={}",
                log.schema_version
            ));
        }

        // 1. Commitments: node membership, role alignment, no dup.
        let mut seen_commit_keys: std::collections::BTreeSet<(NodeId, RoundId)> =
            std::collections::BTreeSet::new();
        for c in &log.commitments {
            let node = self.topology.nodes.iter().find(|n| n.id == c.node_id);
            let Some(node) = node else {
                reasons.push(format!(
                    "commitment_from_unknown_node={}",
                    c.node_id.to_hex()
                ));
                continue;
            };
            let actual = c.public_intent.role();
            if actual != node.role {
                reasons.push(format!(
                    "commitment_role_mismatch={}",
                    c.node_id.to_hex()
                ));
            }
            if c.public_intent.node_id() != c.node_id {
                reasons.push(format!(
                    "commitment_public_intent_node_mismatch={}",
                    c.node_id.to_hex()
                ));
            }
            if c.public_intent.round() != c.round {
                reasons.push(format!(
                    "commitment_round_mismatch={}_{}",
                    c.node_id.to_hex(),
                    c.round.value()
                ));
            }
            if !seen_commit_keys.insert((c.node_id, c.round)) {
                reasons.push(format!(
                    "duplicate_commitment={}_{}",
                    c.node_id.to_hex(),
                    c.round.value()
                ));
            }
        }

        // 2. Proposals: proposer identity matches rotation; annotation valid.
        for p in &log.proposals {
            match deterministic_proposer(&self.topology, p.round) {
                Ok(expected) if expected == p.proposer => {}
                Ok(_) => reasons.push(format!(
                    "proposal_wrong_proposer_round={}",
                    p.round.value()
                )),
                Err(_) => reasons.push(format!(
                    "proposer_rotation_error_round={}",
                    p.round.value()
                )),
            }

            let req_commit = log.commitments.iter().find(|c| {
                c.node_id == p.candidate_requester && c.round == p.round
            });
            let prov_commit = log.commitments.iter().find(|c| {
                c.node_id == p.candidate_provider && c.round == p.round
            });
            let (Some(req), Some(prov)) = (req_commit, prov_commit) else {
                reasons.push(format!(
                    "proposal_unknown_commitment_round={}",
                    p.round.value()
                ));
                continue;
            };
            if let Err(d) = validate_proposal_annotation(
                p.candidate_requester,
                &req.public_intent,
                p.candidate_provider,
                &prov.public_intent,
                &p.matched_capability,
            ) {
                reasons.push(format!(
                    "proposal_annotation_tampered_round={}:{}",
                    p.round.value(),
                    d.tag()
                ));
            }
            // Provider must actually claim the capability.
            if let PublicIntent::Provider {
                capability_claims, ..
            } = &prov.public_intent
            {
                if !capability_claims.contains(&p.matched_capability) {
                    reasons.push(format!(
                        "proposal_provider_lacks_capability_round={}",
                        p.round.value()
                    ));
                }
            }
        }

        // 3. Proofs: role, round, node, commitment_hash match.
        for proof in &log.proofs {
            let parsed = parse_public_inputs(&proof.public_inputs_hex);
            let Some((pi_round, pi_node, pi_commit, pi_role)) = parsed else {
                reasons.push(format!(
                    "proof_malformed_public_inputs={}",
                    proof.node_id.to_hex()
                ));
                continue;
            };
            if pi_round != proof.round {
                reasons.push(format!(
                    "proof_public_inputs_round_mismatch={}_{}",
                    proof.node_id.to_hex(),
                    proof.round.value()
                ));
            }
            if pi_node != proof.node_id {
                reasons.push(format!(
                    "proof_public_inputs_node_mismatch={}",
                    proof.node_id.to_hex()
                ));
            }
            let Some(commit) = log.commitments.iter().find(|c| {
                c.node_id == proof.node_id && c.round == proof.round
            }) else {
                reasons.push(format!(
                    "proof_no_matching_commitment={}_{}",
                    proof.node_id.to_hex(),
                    proof.round.value()
                ));
                continue;
            };
            if hex::encode(pi_commit) != commit.commitment_hex {
                reasons.push(format!(
                    "proof_public_inputs_commitment_mismatch={}_{}",
                    proof.node_id.to_hex(),
                    proof.round.value()
                ));
            }
            let expected_role = match commit.public_intent {
                PublicIntent::Requester { .. } => 0,
                PublicIntent::Provider { .. } => 1,
            };
            if pi_role != expected_role {
                reasons.push(format!(
                    "proof_role_byte_mismatch={}",
                    proof.node_id.to_hex()
                ));
            }
            // proof_hex must start with the "attest-ok" marker (1) or the
            // forthcoming UltraHonk marker (2); anything else is malformed.
            let proof_bytes = match hex::decode(&proof.proof_hex) {
                Ok(b) => b,
                Err(_) => {
                    reasons.push(format!(
                        "proof_hex_malformed={}",
                        proof.node_id.to_hex()
                    ));
                    continue;
                }
            };
            if proof_bytes.is_empty() || (proof_bytes[0] != 1 && proof_bytes[0] != 2) {
                reasons.push(format!(
                    "proof_marker_unexpected={}",
                    proof.node_id.to_hex()
                ));
            }
        }

        // 4. Final round sanity.
        let final_round = log.final_round;
        let final_proposal = log
            .proposals
            .iter()
            .rev()
            .find(|p| p.round == final_round)
            .cloned();

        if log.finalized {
            let Some(proposal) = final_proposal else {
                reasons.push("final_round_missing_proposal".into());
                return VerifierReport {
                    run_id: log.run_id.clone(),
                    valid: false,
                    final_round,
                    reasons,
                };
            };
            if let Err(code) = predicate_holds_for_log(
                log,
                final_round,
                proposal.candidate_requester,
                proposal.candidate_provider,
            ) {
                reasons.push(format!("final_round_predicate_failed:{}", code));
            }
            let receipt = log
                .receipts
                .iter()
                .find(|r| r.round == final_round && r.provider == proposal.candidate_provider);
            let Some(r) = receipt else {
                reasons.push("final_round_missing_receipt".into());
                return VerifierReport {
                    run_id: log.run_id.clone(),
                    valid: false,
                    final_round,
                    reasons,
                };
            };
            if r.signature_hex.is_empty() {
                reasons.push("final_round_empty_signature".into());
            } else {
                let expected = expected_signature_hex(
                    proposal.candidate_provider,
                    final_round,
                    &proposal.matched_capability.to_string(),
                );
                if r.signature_hex != expected {
                    reasons.push("final_round_signature_mismatch".into());
                }
            }
            let has_req_proof = log.proofs.iter().any(|p| {
                p.round == final_round && p.node_id == proposal.candidate_requester
            });
            let has_prov_proof = log.proofs.iter().any(|p| {
                p.round == final_round && p.node_id == proposal.candidate_provider
            });
            if !has_req_proof {
                reasons.push("final_round_missing_requester_proof".into());
            }
            if !has_prov_proof {
                reasons.push("final_round_missing_provider_proof".into());
            }
        } else {
            // Aborted run. Must have no completion receipt on `final_round`.
            let has_receipt = log
                .receipts
                .iter()
                .any(|r| r.round == final_round);
            if has_receipt {
                reasons.push("aborted_run_has_receipt".into());
            }
        }

        // 5. Rejection records — structural sanity only (targeted rounds
        // must be within [0, final_round]).
        for rj in &log.rejections {
            if rj.round > final_round {
                reasons.push(format!(
                    "rejection_round_out_of_range={}_{}",
                    rj.node_id.to_hex(),
                    rj.round.value()
                ));
            }
            if rj.kind.is_empty() || rj.reason_code.is_empty() {
                reasons.push("rejection_record_missing_fields".into());
            }
        }

        VerifierReport {
            run_id: log.run_id.clone(),
            valid: reasons.is_empty(),
            final_round,
            reasons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{CoordinationRuntime, OrderedBus, AgentState};
    use crate::adversarial::Scenario;
    use crate::capability::CapabilityTag;
    use crate::config::TopologyConfig;
    use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent, Secret};
    use std::collections::BTreeMap;

    fn hex64(b: u8) -> String {
        let mut s = String::new();
        for _ in 0..32 {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }

    fn topology_text() -> String {
        format!(
            r#"
version = 1
capability_tags = ["GPU", "CPU", "LLM", "ZK_DEV"]

[[nodes]]
id = "{}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["GPU"]

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["GPU"]

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["CPU"]
"#,
            hex64(0x11),
            hex64(0x22),
            hex64(0x33),
            hex64(0x44),
        )
    }

    fn agents() -> BTreeMap<NodeId, AgentState> {
        let mut m = BTreeMap::new();
        m.insert(
            NodeId::from_bytes([0x11; 32]),
            AgentState::requester(PrivateRequesterIntent {
                node_id: NodeId::from_bytes([0x11; 32]),
                required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
                budget_cents: Secret::new(1000),
            }),
        );
        for (b, cs) in [(0x22u8, vec!["GPU"]), (0x33u8, vec!["GPU"]), (0x44u8, vec!["CPU"])] {
            m.insert(
                NodeId::from_bytes([b; 32]),
                AgentState::provider(PrivateProviderIntent {
                    node_id: NodeId::from_bytes([b; 32]),
                    capability_claims: cs
                        .into_iter()
                        .map(|c| CapabilityTag::parse_shape(c).unwrap())
                        .collect(),
                    reservation_cents: Secret::new(100),
                }),
            );
        }
        m
    }

    fn run_happy_log() -> (TopologyConfig, CoordinationLog) {
        let t = TopologyConfig::from_toml_str(&topology_text()).unwrap();
        let rt = CoordinationRuntime::new(
            t.clone(),
            OrderedBus::new(),
            agents(),
            Scenario::empty(),
            4,
        )
        .unwrap();
        let out = rt.run("run-verifier").unwrap();
        (t, out.log)
    }

    #[test]
    fn happy_log_verifies() {
        let (t, log) = run_happy_log();
        let v = StandaloneVerifier::new(t);
        let r = v.verify_log(&log);
        assert!(r.valid, "report reasons: {:?}", r.reasons);
        assert!(r.reasons.is_empty());
    }

    #[test]
    fn tampered_proof_public_inputs_detected() {
        let (t, mut log) = run_happy_log();
        // Flip a byte in the commitment hash of the first proof's public inputs.
        let mut bytes = hex::decode(&log.proofs[0].public_inputs_hex).unwrap();
        bytes[40] ^= 0xFF;
        log.proofs[0].public_inputs_hex = hex::encode(bytes);
        let v = StandaloneVerifier::new(t);
        let r = v.verify_log(&log);
        assert!(!r.valid);
        assert!(r
            .reasons
            .iter()
            .any(|s| s.contains("proof_public_inputs_commitment_mismatch")));
    }

    #[test]
    fn tampered_proposal_capability_detected() {
        let (t, mut log) = run_happy_log();
        let alt = CapabilityTag::parse_shape("CPU").unwrap();
        log.proposals[0].matched_capability = alt;
        let v = StandaloneVerifier::new(t);
        let r = v.verify_log(&log);
        assert!(!r.valid);
        assert!(r
            .reasons
            .iter()
            .any(|s| s.contains("proposal_annotation_tampered")
                || s.contains("proposal_provider_lacks_capability")));
    }

    #[test]
    fn receipt_signature_tampering_detected() {
        let (t, mut log) = run_happy_log();
        log.receipts[0].signature_hex = "00".repeat(32);
        let v = StandaloneVerifier::new(t);
        let r = v.verify_log(&log);
        assert!(!r.valid);
        assert!(r.reasons.iter().any(|s| s.contains("signature_mismatch")));
    }

    #[test]
    fn unsupported_schema_version_detected() {
        let (t, mut log) = run_happy_log();
        log.schema_version = 99;
        let v = StandaloneVerifier::new(t);
        let r = v.verify_log(&log);
        assert!(!r.valid);
        assert!(r
            .reasons
            .iter()
            .any(|s| s.contains("unsupported_schema_version")));
    }
}
