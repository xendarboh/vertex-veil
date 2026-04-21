//! Provider circuit bridge.
//!
//! Flattens Rust-side inputs into the ACIR witness vector for
//! `circuits/provider/src/main.nr`. ABI parameter order:
//!
//! ```text
//! round: u64
//! node_id: [u8; 32]
//! commitment_hash: [u8; 32]
//! claim_bytes: [[u8; 32]; 4]
//! claim_lens: [u32; 4]
//! n_claims: u32
//! threshold: u64
//! reservation_cents: u64
//! nonce: [u8; 32]
//! ```

use noir_rs::acir::FieldElement;

use vertex_veil_core::{
    build_provider_preimage, hash_preimage_provider, CommitmentBytes, PrivateProviderIntent,
    RoundId, MAX_CAPABILITY_BYTES, MAX_CAPABILITY_CLAIMS,
};

use crate::{
    execute_circuit, field_from_u32, field_from_u64, field_from_u8, witness_map_from_fields,
    CircuitArtifact, NoirBridgeError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderPublicInputs {
    pub round: RoundId,
    pub node_id: [u8; 32],
    pub commitment_hash: [u8; 32],
    /// Fixed-size [claim_slot][claim_bytes] with zero padding. Index >= n_claims must be all zero.
    pub claim_bytes: [[u8; 32]; 4],
    /// Per-slot actual byte length; zero for inactive slots.
    pub claim_lens: [u32; 4],
    pub n_claims: u32,
    pub threshold: u64,
}

#[derive(Clone, Debug)]
pub struct ProviderWitness {
    pub reservation_cents: u64,
    pub nonce: [u8; 32],
}

pub struct ProviderCircuit {
    artifact: CircuitArtifact,
}

impl ProviderCircuit {
    pub const EXPECTED_PARAM_NAMES: &'static [&'static str] = &[
        "round",
        "node_id",
        "commitment_hash",
        "claim_bytes",
        "claim_lens",
        "n_claims",
        "threshold",
        "reservation_cents",
        "nonce",
    ];

    pub fn load(artifact: CircuitArtifact) -> Result<Self, NoirBridgeError> {
        let names = artifact.parameter_names();
        if names != Self::EXPECTED_PARAM_NAMES {
            return Err(NoirBridgeError::Parse(format!(
                "provider ABI parameter mismatch: expected {:?}, got {:?}",
                Self::EXPECTED_PARAM_NAMES,
                names
            )));
        }
        Ok(ProviderCircuit { artifact })
    }

    pub fn bytecode(&self) -> &str {
        self.artifact.bytecode()
    }

    pub fn public_inputs_from_intent(
        intent: &PrivateProviderIntent,
        nonce: &[u8; 32],
        round: RoundId,
        threshold: u64,
    ) -> Result<(ProviderPublicInputs, CommitmentBytes), NoirBridgeError> {
        if intent.capability_claims.len() > MAX_CAPABILITY_CLAIMS {
            return Err(NoirBridgeError::TooManyClaims {
                actual: intent.capability_claims.len(),
                limit: MAX_CAPABILITY_CLAIMS,
            });
        }

        let mut claim_bytes = [[0u8; 32]; 4];
        let mut claim_lens = [0u32; 4];
        for (i, claim) in intent.capability_claims.iter().enumerate() {
            let bytes = claim.as_str().as_bytes();
            if bytes.len() > MAX_CAPABILITY_BYTES {
                return Err(NoirBridgeError::CapabilityTooLong {
                    actual: bytes.len(),
                    limit: MAX_CAPABILITY_BYTES,
                });
            }
            claim_bytes[i][..bytes.len()].copy_from_slice(bytes);
            claim_lens[i] = bytes.len() as u32;
        }

        let preimage = build_provider_preimage(intent, nonce, round)?;
        let commit = hash_preimage_provider(&preimage);

        Ok((
            ProviderPublicInputs {
                round,
                node_id: *intent.node_id.as_bytes(),
                commitment_hash: *commit.as_bytes(),
                claim_bytes,
                claim_lens,
                n_claims: intent.capability_claims.len() as u32,
                threshold,
            },
            commit,
        ))
    }

    pub fn build_witness_map(
        &self,
        public: &ProviderPublicInputs,
        witness: &ProviderWitness,
    ) -> noir_rs::acir::native_types::WitnessMap<FieldElement> {
        let mut fields: Vec<FieldElement> =
            Vec::with_capacity(1 + 32 + 32 + 4 * 32 + 4 + 1 + 1 + 1 + 32);
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
        // claim_bytes[4][32]
        for slot in public.claim_bytes.iter() {
            for b in slot.iter() {
                fields.push(field_from_u8(*b));
            }
        }
        // claim_lens[4]
        for l in public.claim_lens.iter() {
            fields.push(field_from_u32(*l));
        }
        // n_claims
        fields.push(field_from_u32(public.n_claims));
        // threshold
        fields.push(field_from_u64(public.threshold));
        // reservation_cents
        fields.push(field_from_u64(witness.reservation_cents));
        // nonce[32]
        for b in witness.nonce.iter() {
            fields.push(field_from_u8(*b));
        }

        witness_map_from_fields(&fields)
    }

    pub fn execute(
        &self,
        public: &ProviderPublicInputs,
        witness: &ProviderWitness,
    ) -> Result<(), NoirBridgeError> {
        let map = self.build_witness_map(public, witness);
        execute_circuit(self.bytecode(), map)
    }
}
