//! Requester circuit bridge.
//!
//! Flattens Rust-side inputs into the ACIR witness vector required by
//! `circuits/requester/src/main.nr`. The parameter order (used both for the
//! witness map and for the ABI sanity check) is:
//!
//! ```text
//! round: u64
//! node_id: [u8; 32]
//! commitment_hash: [u8; 32]
//! required_capability: [u8; 32]
//! capability_len: u32
//! threshold: u64
//! budget_cents: u64
//! nonce: [u8; 32]
//! ```

use noir_rs::acir::FieldElement;

use vertex_veil_core::{
    build_requester_preimage, hash_preimage_requester, CommitmentBytes, PrivateRequesterIntent,
    RoundId, MAX_CAPABILITY_BYTES,
};

use crate::{
    execute_circuit, field_from_u32, field_from_u64, field_from_u8, witness_map_from_fields,
    CircuitArtifact, NoirBridgeError,
};

/// Public inputs for the requester circuit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequesterPublicInputs {
    pub round: RoundId,
    pub node_id: [u8; 32],
    pub commitment_hash: [u8; 32],
    /// Zero-padded capability bytes, length `MAX_CAPABILITY_BYTES`.
    pub required_capability: [u8; 32],
    /// Actual capability byte length; 1..=MAX_CAPABILITY_BYTES.
    pub capability_len: u32,
    pub threshold: u64,
}

/// Private witness for the requester circuit.
#[derive(Clone, Debug)]
pub struct RequesterWitness {
    pub budget_cents: u64,
    pub nonce: [u8; 32],
}

/// High-level wrapper around the compiled requester artifact.
pub struct RequesterCircuit {
    artifact: CircuitArtifact,
}

impl RequesterCircuit {
    pub const EXPECTED_PARAM_NAMES: &'static [&'static str] = &[
        "round",
        "node_id",
        "commitment_hash",
        "required_capability",
        "capability_len",
        "threshold",
        "budget_cents",
        "nonce",
    ];

    /// Load the circuit from a compiled nargo artifact.
    pub fn load(artifact: CircuitArtifact) -> Result<Self, NoirBridgeError> {
        let names = artifact.parameter_names();
        if names != Self::EXPECTED_PARAM_NAMES {
            return Err(NoirBridgeError::Parse(format!(
                "requester ABI parameter mismatch: expected {:?}, got {:?}",
                Self::EXPECTED_PARAM_NAMES,
                names
            )));
        }
        Ok(RequesterCircuit { artifact })
    }

    pub fn bytecode(&self) -> &str {
        self.artifact.bytecode()
    }

    /// Helper: construct a `RequesterPublicInputs` from a private intent +
    /// nonce + threshold, by computing the commitment hash the Rust way.
    pub fn public_inputs_from_intent(
        intent: &PrivateRequesterIntent,
        nonce: &[u8; 32],
        round: RoundId,
        threshold: u64,
    ) -> Result<(RequesterPublicInputs, CommitmentBytes), NoirBridgeError> {
        let cap_bytes = intent.required_capability.as_str().as_bytes();
        if cap_bytes.len() > MAX_CAPABILITY_BYTES {
            return Err(NoirBridgeError::CapabilityTooLong {
                actual: cap_bytes.len(),
                limit: MAX_CAPABILITY_BYTES,
            });
        }
        let mut padded = [0u8; 32];
        padded[..cap_bytes.len()].copy_from_slice(cap_bytes);

        let preimage = build_requester_preimage(intent, nonce, round)?;
        let commit = hash_preimage_requester(&preimage);

        Ok((
            RequesterPublicInputs {
                round,
                node_id: *intent.node_id.as_bytes(),
                commitment_hash: *commit.as_bytes(),
                required_capability: padded,
                capability_len: cap_bytes.len() as u32,
                threshold,
            },
            commit,
        ))
    }

    /// Flatten public + witness inputs into the ACIR witness vector.
    pub fn build_witness_map(
        &self,
        public: &RequesterPublicInputs,
        witness: &RequesterWitness,
    ) -> noir_rs::acir::native_types::WitnessMap<FieldElement> {
        let mut fields: Vec<FieldElement> = Vec::with_capacity(1 + 32 + 32 + 32 + 1 + 1 + 1 + 32);
        // round
        fields.push(field_from_u64(public.round.value()));
        // node_id[32]
        for b in public.node_id.iter() {
            fields.push(field_from_u8(*b));
        }
        // commitment_hash[32]
        for b in public.commitment_hash.iter() {
            fields.push(field_from_u8(*b));
        }
        // required_capability[32]
        for b in public.required_capability.iter() {
            fields.push(field_from_u8(*b));
        }
        // capability_len
        fields.push(field_from_u32(public.capability_len));
        // threshold
        fields.push(field_from_u64(public.threshold));
        // budget_cents
        fields.push(field_from_u64(witness.budget_cents));
        // nonce[32]
        for b in witness.nonce.iter() {
            fields.push(field_from_u8(*b));
        }

        witness_map_from_fields(&fields)
    }

    /// Execute the circuit against the given witness. Returns `Ok(())` if
    /// every constraint is satisfied.
    pub fn execute(
        &self,
        public: &RequesterPublicInputs,
        witness: &RequesterWitness,
    ) -> Result<(), NoirBridgeError> {
        let map = self.build_witness_map(public, witness);
        execute_circuit(self.bytecode(), map)
    }
}
