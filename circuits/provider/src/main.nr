//! Provider acceptance circuit.
//!
//! Public inputs:
//!   * `round`                  u64
//!   * `node_id`                [u8; 32] - provider stable public key
//!   * `commitment_hash`        [u8; 32] - published commitment digest
//!   * `claim_bytes`            [[u8; 32]; 4] - zero-padded public claim bytes
//!   * `claim_lens`             [u32; 4] - per-slot actual byte lengths
//!   * `n_claims`               u32 - actual claim count (1..=4)
//!   * `threshold`              u64 - maximum acceptable reservation
//!
//! Private witness:
//!   * `reservation_cents`      u64
//!   * `nonce`                  [u8; 32]
//!
//! Constraints:
//!   * `n_claims` is in [1, MAX_CAPABILITY_CLAIMS]
//!   * per-slot lengths are in [1, MAX_CAPABILITY_BYTES] for active slots
//!     and exactly 0 for inactive slots
//!   * `reservation_cents <= threshold`      (private acceptance check)
//!   * blake2s of the canonical 264-byte preimage equals `commitment_hash`
//!
//! Round-bound replay rejection comes "for free": `round` is embedded in the
//! preimage; substituting a prior-round input fails the blake2s constraint.

use vertex_veil_shared::{
    build_provider_preimage, hash_provider_preimage, MAX_CAPABILITY_BYTES, MAX_CAPABILITY_CLAIMS,
};

fn main(
    round: pub u64,
    node_id: pub [u8; 32],
    commitment_hash: pub [u8; 32],
    claim_bytes: pub [[u8; 32]; 4],
    claim_lens: pub [u32; 4],
    n_claims: pub u32,
    threshold: pub u64,
    reservation_cents: u64,
    nonce: [u8; 32],
) {
    assert(n_claims > 0);
    assert(n_claims <= MAX_CAPABILITY_CLAIMS);

    // Per-slot shape checks: active slot lengths in [1, MAX], inactive slots
    // have length 0 and (by convention) zero byte content.
    for i in 0..4 {
        let active = (i as u32) < n_claims;
        let len = claim_lens[i];
        if active {
            assert(len > 0);
            assert(len <= MAX_CAPABILITY_BYTES);
        } else {
            assert(len == 0);
        }
    }

    // Private threshold check: reservation must not exceed the provider's
    // own maximum acceptable value.
    assert(reservation_cents <= threshold);

    // Reconstruct the canonical preimage and hash with blake2s.
    let preimage = build_provider_preimage(
        round,
        node_id,
        claim_bytes,
        claim_lens,
        n_claims,
        reservation_cents,
        nonce,
    );
    let actual = hash_provider_preimage(preimage);
    for i in 0..32 {
        assert(actual[i] == commitment_hash[i]);
    }
}

#[test]
fn provider_accepts_reservation_at_threshold() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55; // "GPU"
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    let claim_lens: [u32; 4] = [3, 0, 0, 0];

    let preimage = build_provider_preimage(0, node_id, claim_bytes, claim_lens, 1, 200, nonce);
    let commit = hash_provider_preimage(preimage);

    main(0, node_id, commit, claim_bytes, claim_lens, 1, 200, 200, nonce);
}

#[test]
fn provider_accepts_reservation_below_threshold() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55;
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    let claim_lens: [u32; 4] = [3, 0, 0, 0];

    let preimage = build_provider_preimage(7, node_id, claim_bytes, claim_lens, 1, 150, nonce);
    let commit = hash_provider_preimage(preimage);

    main(7, node_id, commit, claim_bytes, claim_lens, 1, 300, 150, nonce);
}

#[test(should_fail)]
fn provider_rejects_reservation_above_threshold() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55;
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    let claim_lens: [u32; 4] = [3, 0, 0, 0];

    let preimage = build_provider_preimage(0, node_id, claim_bytes, claim_lens, 1, 301, nonce);
    let commit = hash_provider_preimage(preimage);

    // Reservation 301 exceeds threshold 300 - expected to fail.
    main(0, node_id, commit, claim_bytes, claim_lens, 1, 300, 301, nonce);
}

#[test(should_fail)]
fn provider_rejects_wrong_round_public_input() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55;
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    let claim_lens: [u32; 4] = [3, 0, 0, 0];

    let preimage = build_provider_preimage(0, node_id, claim_bytes, claim_lens, 1, 150, nonce);
    let commit = hash_provider_preimage(preimage);

    main(1, node_id, commit, claim_bytes, claim_lens, 1, 200, 150, nonce);
}

#[test(should_fail)]
fn provider_rejects_inactive_slot_with_nonzero_len() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55;
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    // Active slots n_claims=1, but claim_lens[1] is non-zero - invalid.
    let claim_lens: [u32; 4] = [3, 5, 0, 0];

    let preimage = build_provider_preimage(0, node_id, claim_bytes, claim_lens, 1, 150, nonce);
    let commit = hash_provider_preimage(preimage);

    main(0, node_id, commit, claim_bytes, claim_lens, 1, 200, 150, nonce);
}
