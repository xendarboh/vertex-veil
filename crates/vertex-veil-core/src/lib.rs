//! Vertex Veil core: shared protocol types, configuration, and public
//! coordination artifact schema.
//!
//! Phase 0 scope (see `intent/plan.md`):
//! - Shared intent, match, round, and artifact types.
//! - Runtime configuration with a 4-node baseline.
//! - Public coordination artifact schema that excludes private witness fields
//!   by construction.
//!
//! Private economic constraints (requester budgets, provider reservation
//! prices) live in `private_intent` and are wrapped in [`Secret`] so debug
//! formatting and default serialization never expose their values. Public
//! coordination records never hold a [`Secret`] field.

pub mod artifacts;
pub mod capability;
pub mod config;
pub mod error;
pub mod keys;
pub mod private_intent;
pub mod shared_types;

pub use artifacts::{
    ArtifactWriter, CommitmentRecord, CompletionReceiptRecord, CoordinationLog,
    ProofArtifactRecord, ProposalRecord, VerifierReport,
};
pub use capability::{CapabilityTag, CapabilityTagSet};
pub use config::{NodeConfig, Role, TopologyConfig};
pub use error::{ArtifactError, ConfigError};
pub use keys::NodeId;
pub use private_intent::{PrivateProviderIntent, PrivateRequesterIntent, Secret};
pub use shared_types::{PublicIntent, RoundId, RoundState};
