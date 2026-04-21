//! Vertex Veil core: shared protocol types, configuration, and public
//! coordination artifact schema.
//!
//! Phases covered here:
//!
//! - Phase 0: shared types, config loading, public artifact schema.
//! - Phase 1: commitments, proposer rotation, candidate formation,
//!   runtime match predicate, round state machine with replay and
//!   double-commit rejection, predicate parity fixtures.
//!
//! Private economic constraints (requester budgets, provider reservation
//! prices) live in `private_intent` and are wrapped in [`Secret`] so debug
//! formatting and default serialization never expose their values. Public
//! coordination records never hold a [`Secret`] field.

pub mod artifacts;
pub mod candidate;
pub mod capability;
pub mod commitments;
pub mod config;
pub mod error;
pub mod keys;
pub mod parity;
pub mod predicate;
pub mod private_intent;
pub mod proposer;
pub mod round_machine;
pub mod shared_types;

pub use artifacts::{
    ArtifactWriter, CommitmentRecord, CompletionReceiptRecord, CoordinationLog,
    ProofArtifactRecord, ProposalRecord, VerifierReport,
};
pub use candidate::{derive_candidate, Candidate, CandidateRejection};
pub use capability::{CapabilityTag, CapabilityTagSet};
pub use commitments::{
    build_provider_preimage, build_requester_preimage, commit_provider, commit_requester,
    derive_test_nonce, hash_preimage_provider, hash_preimage_requester, CommitmentBytes,
    CommitmentError, COMMIT_DOMAIN_PROVIDER, COMMIT_DOMAIN_REQUESTER, COMMIT_SCHEMA_VERSION,
    MAX_CAPABILITY_BYTES, MAX_CAPABILITY_CLAIMS, PROVIDER_PREIMAGE_LEN, REQUESTER_PREIMAGE_LEN,
};
pub use config::{NodeConfig, Role, TopologyConfig};
pub use error::{ArtifactError, ConfigError};
pub use keys::NodeId;
pub use parity::{ExpectedOutcome, ParityFixture};
pub use predicate::{match_predicate, validate_proposal_annotation, PredicateDenial};
pub use private_intent::{PrivateProviderIntent, PrivateRequesterIntent, Secret};
pub use proposer::{proposer_for_round, ProposerError};
pub use round_machine::{RoundError, RoundMachine};
pub use shared_types::{PublicIntent, RoundId, RoundState};
