//! Deterministic round state machine.
//!
//! The [`RoundMachine`] accepts commitments and proposals for the active
//! round and rejects anything that violates the round model:
//!
//! - round binding: every accepted record must carry the active round id
//! - double-commit: a given `(node_id, round)` pair may appear at most once
//! - replay: a commitment hex that was already seen in any prior round is
//!   rejected even if labelled with the active round
//! - unknown node: commitments from keys that are not in the topology are
//!   ignored with a structured error
//! - proposer identity: a proposal is accepted only from the current
//!   proposer chosen by stable-order rotation
//! - proposal integrity: candidate identities, roles, and matched capability
//!   must agree with the referenced commitments
//!
//! All acceptance paths validate before mutating state, so a rejected input
//! leaves the state identical to its pre-call value.

use std::collections::{BTreeMap, BTreeSet};

use crate::artifacts::{CommitmentRecord, ProposalRecord};
use crate::config::{Role, TopologyConfig};
use crate::keys::NodeId;
use crate::predicate::{validate_proposal_annotation, PredicateDenial};
use crate::proposer::{proposer_for_round, ProposerError};
use crate::shared_types::{PublicIntent, RoundId};

/// Errors produced by [`RoundMachine`] acceptance paths.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RoundError {
    #[error("record round {actual} does not match active round {expected}")]
    RoundMismatch { expected: u64, actual: u64 },

    #[error("duplicate commitment for node in round {round}")]
    DuplicateCommitment { round: u64 },

    #[error("replay detected: commitment hex already seen in a prior round")]
    ReplayDetected,

    #[error("commitment from unknown node id")]
    UnknownNode,

    #[error("commitment role does not match topology configuration")]
    RoleMismatch,

    #[error("backward round transition is not permitted")]
    BackwardTransition,

    #[error("proposer error: {0}")]
    Proposer(#[from] ProposerError),

    #[error("wrong proposer for round")]
    WrongProposer,

    #[error("proposal references a requester with no accepted commitment")]
    UnknownRequesterCommitment,

    #[error("proposal references a provider with no accepted commitment")]
    UnknownProviderCommitment,

    #[error("proposal annotation is tampered: {0:?}")]
    TamperedProposal(PredicateDenial),

    #[error("round has already been finalized")]
    AlreadyFinalized,
}

/// Snapshot of a finalized round, retained for replay detection and audit.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FinalizedRound {
    #[allow(dead_code)] // retained for future audit; not read by Phase 1 logic
    round: RoundId,
    commitments: BTreeMap<NodeId, CommitmentRecord>,
    #[allow(dead_code)]
    proposal: Option<ProposalRecord>,
}

/// Round-state machine.
#[derive(Debug)]
pub struct RoundMachine {
    topology: TopologyConfig,
    current: RoundId,
    commitments: BTreeMap<NodeId, CommitmentRecord>,
    proposal: Option<ProposalRecord>,
    /// Every commitment hex ever accepted, regardless of round. Used for
    /// replay detection.
    history_hex: BTreeSet<String>,
    past: Vec<FinalizedRound>,
}

impl RoundMachine {
    /// Build a new machine starting at the given round.
    pub fn new(topology: TopologyConfig, start: RoundId) -> Self {
        RoundMachine {
            topology,
            current: start,
            commitments: BTreeMap::new(),
            proposal: None,
            history_hex: BTreeSet::new(),
            past: Vec::new(),
        }
    }

    pub fn topology(&self) -> &TopologyConfig {
        &self.topology
    }

    pub fn current_round(&self) -> RoundId {
        self.current
    }

    pub fn commitments(&self) -> &BTreeMap<NodeId, CommitmentRecord> {
        &self.commitments
    }

    pub fn proposal(&self) -> Option<&ProposalRecord> {
        self.proposal.as_ref()
    }

    /// Provider list in stable (byte-lex) order.
    pub fn providers_stable(&self) -> Vec<NodeId> {
        self.topology
            .providers_stable_order()
            .into_iter()
            .map(|n| n.id)
            .collect()
    }

    /// Proposer for the active round.
    pub fn current_proposer(&self) -> Result<NodeId, RoundError> {
        let providers = self.providers_stable();
        Ok(proposer_for_round(self.current, &providers)?)
    }

    /// Accept a commitment for the active round. Validates before mutating,
    /// so a rejection leaves state unchanged.
    pub fn accept_commitment(&mut self, record: CommitmentRecord) -> Result<(), RoundError> {
        if self.proposal.is_some() {
            return Err(RoundError::AlreadyFinalized);
        }
        if record.round != self.current {
            return Err(RoundError::RoundMismatch {
                expected: self.current.value(),
                actual: record.round.value(),
            });
        }

        // The commitment must come from a configured node, and the public
        // intent's role must match the topology.
        let node_cfg = self
            .topology
            .nodes
            .iter()
            .find(|n| n.id == record.node_id)
            .ok_or(RoundError::UnknownNode)?;
        let expected_role = node_cfg.role;
        let actual_role = record.public_intent.role();
        if actual_role != expected_role {
            return Err(RoundError::RoleMismatch);
        }

        // Public intent's embedded round must also match (defense in depth).
        if record.public_intent.round() != self.current {
            return Err(RoundError::RoundMismatch {
                expected: self.current.value(),
                actual: record.public_intent.round().value(),
            });
        }

        // Public intent's embedded node id must match the outer record's
        // node id (tamper resistance against mismatched public metadata).
        if record.public_intent.node_id() != record.node_id {
            return Err(RoundError::RoleMismatch);
        }

        // Reject duplicates before touching state.
        if self.commitments.contains_key(&record.node_id) {
            return Err(RoundError::DuplicateCommitment {
                round: self.current.value(),
            });
        }

        // Replay detection against any prior round.
        if self.history_hex.contains(&record.commitment_hex) {
            return Err(RoundError::ReplayDetected);
        }

        // For a requester commitment, the required capability must be
        // present (otherwise proposal construction will fail).
        if expected_role == Role::Requester {
            match &record.public_intent {
                PublicIntent::Requester {
                    required_capability,
                    ..
                } if required_capability.as_str().is_empty() => {
                    return Err(RoundError::RoleMismatch);
                }
                _ => {}
            }
        }

        // All checks passed. Mutate state.
        self.history_hex.insert(record.commitment_hex.clone());
        self.commitments.insert(record.node_id, record);
        Ok(())
    }

    /// Accept a proposal for the active round. Validates that the proposer
    /// is correct, references known commitments, and that the proposal's
    /// public metadata matches the referenced commitments.
    pub fn accept_proposal(&mut self, proposal: ProposalRecord) -> Result<(), RoundError> {
        if self.proposal.is_some() {
            return Err(RoundError::AlreadyFinalized);
        }
        if proposal.round != self.current {
            return Err(RoundError::RoundMismatch {
                expected: self.current.value(),
                actual: proposal.round.value(),
            });
        }

        let expected_proposer = self.current_proposer()?;
        if proposal.proposer != expected_proposer {
            return Err(RoundError::WrongProposer);
        }

        let req_commit = self
            .commitments
            .get(&proposal.candidate_requester)
            .ok_or(RoundError::UnknownRequesterCommitment)?;
        let prov_commit = self
            .commitments
            .get(&proposal.candidate_provider)
            .ok_or(RoundError::UnknownProviderCommitment)?;

        validate_proposal_annotation(
            proposal.candidate_requester,
            &req_commit.public_intent,
            proposal.candidate_provider,
            &prov_commit.public_intent,
            &proposal.matched_capability,
        )
        .map_err(RoundError::TamperedProposal)?;

        // Additionally, the provider must actually claim the matched
        // capability (predicate-level invariant).
        match &prov_commit.public_intent {
            PublicIntent::Provider {
                capability_claims, ..
            } => {
                if !capability_claims.contains(&proposal.matched_capability) {
                    return Err(RoundError::TamperedProposal(
                        PredicateDenial::ProviderLacksCapability,
                    ));
                }
            }
            _ => {
                return Err(RoundError::TamperedProposal(
                    PredicateDenial::WrongProviderRole,
                ));
            }
        }

        self.proposal = Some(proposal);
        Ok(())
    }

    /// Advance to a specific next round, finalizing the current round into
    /// history. The next round must be strictly greater than the current
    /// round.
    pub fn advance_to(&mut self, next: RoundId) -> Result<(), RoundError> {
        if next <= self.current {
            return Err(RoundError::BackwardTransition);
        }
        let finalized = FinalizedRound {
            round: self.current,
            commitments: std::mem::take(&mut self.commitments),
            proposal: self.proposal.take(),
        };
        self.past.push(finalized);
        self.current = next;
        Ok(())
    }

    /// Convenience: advance exactly one round.
    pub fn advance_fallback(&mut self) -> Result<(), RoundError> {
        let next = self.current.next();
        self.advance_to(next)
    }

    /// Number of finalized past rounds, for audit.
    pub fn finalized_round_count(&self) -> usize {
        self.past.len()
    }

    /// Commitments captured in a previously finalized round, by index.
    pub fn past_commitments(&self, past_index: usize) -> Option<&BTreeMap<NodeId, CommitmentRecord>> {
        self.past.get(past_index).map(|f| &f.commitments)
    }
}
