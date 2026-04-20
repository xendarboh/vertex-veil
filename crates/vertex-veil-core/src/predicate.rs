//! Runtime-side match predicate.
//!
//! The match predicate is the single logical function that decides whether a
//! (requester, provider) pair is feasible under the public round inputs.
//! It is implemented here in Rust and mirrored in a Noir circuit in Phase 2.
//! Divergence between the two implementations is a correctness bug, not a
//! performance tradeoff (see `INTENT.md`).
//!
//! The predicate reads only public fields: role, round, node ids, and
//! capability tags. Private price constraints are not part of the match
//! predicate; they are checked inside each agent's local Noir proof against
//! its own private witness.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityTag;
use crate::keys::NodeId;
use crate::shared_types::{PublicIntent, RoundId};

/// Why the predicate rejected a (requester, provider) pair.
///
/// Each variant has a short machine-readable [`tag`](Self::tag). Parity
/// tests compare predicate outputs via `tag`, which is stable across minor
/// text changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PredicateDenial {
    WrongRequesterRole,
    WrongProviderRole,
    RequesterRoundMismatch,
    ProviderRoundMismatch,
    RequesterProviderRoundMismatch,
    ProviderLacksCapability,
    /// A claimed requester node does not match the candidate's node id in
    /// the proposal. Used by [`ProposalValidation`] below.
    RequesterIdentityMismatch,
    ProviderIdentityMismatch,
    CapabilityAnnotationMismatch,
}

impl PredicateDenial {
    pub fn tag(&self) -> &'static str {
        match self {
            PredicateDenial::WrongRequesterRole => "wrong_requester_role",
            PredicateDenial::WrongProviderRole => "wrong_provider_role",
            PredicateDenial::RequesterRoundMismatch => "requester_round_mismatch",
            PredicateDenial::ProviderRoundMismatch => "provider_round_mismatch",
            PredicateDenial::RequesterProviderRoundMismatch => {
                "requester_provider_round_mismatch"
            }
            PredicateDenial::ProviderLacksCapability => "provider_lacks_capability",
            PredicateDenial::RequesterIdentityMismatch => "requester_identity_mismatch",
            PredicateDenial::ProviderIdentityMismatch => "provider_identity_mismatch",
            PredicateDenial::CapabilityAnnotationMismatch => "capability_annotation_mismatch",
        }
    }
}

/// Run the runtime match predicate over public inputs.
///
/// - `requester` must be in requester role at `round`.
/// - `provider` must be in provider role at `round`.
/// - The provider's capability claims must contain the requester's required
///   capability.
pub fn match_predicate(
    requester: &PublicIntent,
    provider: &PublicIntent,
    round: RoundId,
) -> Result<(), PredicateDenial> {
    let (req_round, req_cap) = match requester {
        PublicIntent::Requester {
            round,
            required_capability,
            ..
        } => (*round, required_capability),
        PublicIntent::Provider { .. } => return Err(PredicateDenial::WrongRequesterRole),
    };
    let (prov_round, prov_claims) = match provider {
        PublicIntent::Provider {
            round,
            capability_claims,
            ..
        } => (*round, capability_claims),
        PublicIntent::Requester { .. } => return Err(PredicateDenial::WrongProviderRole),
    };

    if req_round != round && prov_round != round {
        return Err(PredicateDenial::RequesterProviderRoundMismatch);
    }
    if req_round != round {
        return Err(PredicateDenial::RequesterRoundMismatch);
    }
    if prov_round != round {
        return Err(PredicateDenial::ProviderRoundMismatch);
    }
    if !prov_claims.contains(req_cap) {
        return Err(PredicateDenial::ProviderLacksCapability);
    }
    Ok(())
}

/// Validate that a proposal's annotated fields match the public intents it
/// points at. Used by [`crate::round_machine::RoundMachine`] to reject
/// tampered proposals.
pub fn validate_proposal_annotation(
    candidate_requester: NodeId,
    requester_intent: &PublicIntent,
    candidate_provider: NodeId,
    provider_intent: &PublicIntent,
    matched_capability: &CapabilityTag,
) -> Result<(), PredicateDenial> {
    let req_id = requester_intent.node_id();
    let prov_id = provider_intent.node_id();
    if req_id != candidate_requester {
        return Err(PredicateDenial::RequesterIdentityMismatch);
    }
    if prov_id != candidate_provider {
        return Err(PredicateDenial::ProviderIdentityMismatch);
    }
    match requester_intent {
        PublicIntent::Requester {
            required_capability,
            ..
        } => {
            if required_capability != matched_capability {
                return Err(PredicateDenial::CapabilityAnnotationMismatch);
            }
        }
        _ => return Err(PredicateDenial::WrongRequesterRole),
    }
    Ok(())
}
