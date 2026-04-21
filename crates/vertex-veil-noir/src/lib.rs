//! Rust bridge to the Vertex Veil Noir circuits.
//!
//! Wraps `noir_rs` at v1.0.0-beta.20. Two public circuits live under
//! `circuits/`:
//!
//! - `vertex_veil_requester` — proves requester-side acceptance
//! - `vertex_veil_provider`  — proves provider-side acceptance
//!
//! The default feature set uses `noir_rs`'s ACIR executor to validate that a
//! witness satisfies every circuit constraint. Enable the `barretenberg`
//! feature to pull in `noir_rs/barretenberg` and obtain full UltraHonk
//! proof generation and verification.

use std::fs;
use std::path::Path;

use noir_rs::acir::native_types::{Witness, WitnessMap};
use noir_rs::acir::FieldElement;
use noir_rs::execute::execute;
use noir_rs::witness::serialize_witness;
use serde::{Deserialize, Serialize};

use vertex_veil_core::CommitmentError;

pub mod provider;
pub mod requester;

pub use provider::{ProviderCircuit, ProviderPublicInputs, ProviderWitness};
pub use requester::{RequesterCircuit, RequesterPublicInputs, RequesterWitness};

/// Errors produced by the Rust/Noir bridge. Never carries private witness
/// values.
#[derive(Debug, thiserror::Error)]
pub enum NoirBridgeError {
    #[error("failed to read circuit artifact: {0}")]
    Io(String),

    #[error("failed to parse circuit artifact JSON: {0}")]
    Parse(String),

    #[error("circuit bytecode missing or empty")]
    EmptyBytecode,

    #[error("commitment helper rejected the input: {0}")]
    CommitmentError(#[from] CommitmentError),

    #[error("witness execution failed (likely a constraint violation)")]
    ExecuteFailed(String),

    #[error("capability byte length {actual} exceeds MAX_CAPABILITY_BYTES={limit}")]
    CapabilityTooLong { actual: usize, limit: usize },

    #[error("claim count {actual} exceeds MAX_CAPABILITY_CLAIMS={limit}")]
    TooManyClaims { actual: usize, limit: usize },
}

/// Minimal view of the nargo-compiled circuit JSON artifact.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CircuitArtifact {
    pub noir_version: String,
    /// The nargo hash is emitted as a decimal string in the JSON (u64 values
    /// exceed the safe integer range).
    pub hash: String,
    pub bytecode: String,
    pub abi: AbiView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AbiView {
    pub parameters: Vec<AbiParameter>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AbiParameter {
    pub name: String,
    pub visibility: String,
    #[serde(rename = "type")]
    pub ty: serde_json::Value,
}

impl CircuitArtifact {
    /// Load a nargo-compiled artifact JSON file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, NoirBridgeError> {
        let text = fs::read_to_string(path).map_err(|e| NoirBridgeError::Io(e.to_string()))?;
        let art: CircuitArtifact =
            serde_json::from_str(&text).map_err(|e| NoirBridgeError::Parse(e.to_string()))?;
        if art.bytecode.is_empty() {
            return Err(NoirBridgeError::EmptyBytecode);
        }
        Ok(art)
    }

    pub fn bytecode(&self) -> &str {
        &self.bytecode
    }

    pub fn parameter_names(&self) -> Vec<&str> {
        self.abi.parameters.iter().map(|p| p.name.as_str()).collect()
    }
}

/// Execute a circuit with the given witness map. Returns `Ok(())` if every
/// constraint is satisfied.
pub(crate) fn execute_circuit(
    bytecode: &str,
    initial_witness: WitnessMap<FieldElement>,
) -> Result<(), NoirBridgeError> {
    let solved = execute(bytecode, initial_witness).map_err(NoirBridgeError::ExecuteFailed)?;
    // Exercise the full serialization path so we match the shape the prover
    // would consume.
    let _serialized = serialize_witness(solved).map_err(NoirBridgeError::ExecuteFailed)?;
    Ok(())
}

/// Convert a sequence of field values into a Noir-compatible input witness
/// map. ACIR input witnesses are zero-indexed, starting at `Witness(0)`.
pub(crate) fn witness_map_from_fields(values: &[FieldElement]) -> WitnessMap<FieldElement> {
    let mut map = WitnessMap::<FieldElement>::new();
    for (i, v) in values.iter().enumerate() {
        map.insert(Witness(i as u32), *v);
    }
    map
}

pub(crate) fn field_from_u8(b: u8) -> FieldElement {
    FieldElement::from(b as u128)
}

pub(crate) fn field_from_u32(v: u32) -> FieldElement {
    FieldElement::from(v as u128)
}

pub(crate) fn field_from_u64(v: u64) -> FieldElement {
    FieldElement::from(v as u128)
}
