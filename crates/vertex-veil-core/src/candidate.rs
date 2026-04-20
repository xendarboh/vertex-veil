//! Candidate formation from public commitments.
//!
//! The proposer derives a candidate `(requester, provider, matched_capability)`
//! tuple from the public commitment set. Candidate formation only inspects
//! public fields: node ids, roles, round, and capability tags. The runtime
//! match predicate is applied to filter infeasible pairs; the surviving
//! providers are tiebroken by stable public key order.

use crate::artifacts::CommitmentRecord;
use crate::capability::CapabilityTag;
use crate::keys::NodeId;
use crate::predicate::match_predicate;
use crate::shared_types::{PublicIntent, RoundId};

/// Candidate match derived from public commitments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub requester: NodeId,
    pub provider: NodeId,
    pub matched_capability: CapabilityTag,
}

/// Reasons [`derive_candidate`] refuses to run at all. Returned for
/// malformed inputs; "no feasible provider" is not an error and is returned
/// as `Ok(None)` instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateRejection {
    MissingRequesterCapability,
    WrongRoundInRequester,
}

/// Derive a candidate match for `round` from the commitment set.
///
/// Returns `Ok(Some(candidate))` if any provider is feasible,
/// `Ok(None)` if none is feasible but the inputs themselves are structurally
/// valid, and `Err(CandidateRejection)` if the inputs are malformed (e.g.
/// requester with a missing required capability).
pub fn derive_candidate(
    round: RoundId,
    requester: &CommitmentRecord,
    providers: &[CommitmentRecord],
) -> Result<Option<Candidate>, CandidateRejection> {
    let req_cap = match &requester.public_intent {
        PublicIntent::Requester {
            required_capability,
            round: r,
            ..
        } => {
            if *r != round {
                return Err(CandidateRejection::WrongRoundInRequester);
            }
            if required_capability.as_str().is_empty() {
                return Err(CandidateRejection::MissingRequesterCapability);
            }
            required_capability.clone()
        }
        PublicIntent::Provider { .. } => {
            return Err(CandidateRejection::MissingRequesterCapability);
        }
    };

    let mut feasible: Vec<NodeId> = Vec::new();
    for p in providers {
        if match_predicate(&requester.public_intent, &p.public_intent, round).is_ok() {
            feasible.push(p.node_id);
        }
    }

    if feasible.is_empty() {
        // No feasible provider is a valid flow outcome: the runtime falls
        // back to the next round and a different proposer.
        return Ok(None);
    }

    // Stable public key order: smallest bytes wins.
    feasible.sort();
    let provider = feasible[0];

    Ok(Some(Candidate {
        requester: requester.node_id,
        provider,
        matched_capability: req_cap,
    }))
}
