//! Requester acceptance circuit.
//!
//! Public inputs (order matches `vertex-veil-noir` Rust-side witness writer):
//!   * `round`                  u64   - active round id
//!   * `node_id`                [u8; 32] - requester stable public key
//!   * `commitment_hash`        [u8; 32] - published commitment digest
//!   * `required_capability`    [u8; 32] - zero-padded public capability tag
//!   * `capability_len`         u32    - actual tag byte length
//!   * `threshold`              u64    - minimum acceptable budget
//!
//! Private witness:
//!   * `budget_cents`           u64    - private budget
//!   * `nonce`                  [u8; 32] - per-round nonce
//!
//! Constraints:
//!   * `capability_len` is in [1, MAX_CAPABILITY_BYTES]
//!   * `budget_cents >= threshold`           (private acceptance check)
//!   * blake2s of the canonical 153-byte preimage equals `commitment_hash`
//!     (this binds node_id, round, capability, budget, nonce together)
//!
//! Round-bound replay rejection comes "for free": `round` is embedded in
//! the preimage, so substituting a prior-round public input leaves the
//! blake2s constraint unsatisfied.

use vertex_veil_shared::{
    build_requester_preimage, hash_requester_preimage, MAX_CAPABILITY_BYTES,
};

fn main(
    round: pub u64,
    node_id: pub [u8; 32],
    commitment_hash: pub [u8; 32],
    required_capability: pub [u8; 32],
    capability_len: pub u32,
    threshold: pub u64,
    budget_cents: u64,
    nonce: [u8; 32],
) {
    // Shape checks on public capability length.
    assert(capability_len > 0);
    assert(capability_len <= MAX_CAPABILITY_BYTES);

    // Private threshold check: budget must clear the requester's threshold.
    assert(budget_cents >= threshold);

    // Reconstruct the canonical preimage from witness + public inputs and
    // hash with blake2s. The result must equal the published commitment.
    let preimage = build_requester_preimage(
        round,
        node_id,
        required_capability,
        capability_len,
        budget_cents,
        nonce,
    );
    let actual = hash_requester_preimage(preimage);
    for i in 0..32 {
        assert(actual[i] == commitment_hash[i]);
    }
}

#[test]
fn requester_accepts_exact_threshold() {
    // budget == threshold is the boundary case; must verify.
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; cap[1] = 0x50; cap[2] = 0x55;

    let preimage = build_requester_preimage(0, node_id, cap, 3, 100, nonce);
    let commit = hash_requester_preimage(preimage);

    main(0, node_id, commit, cap, 3, 100, 100, nonce);
}

#[test]
fn requester_accepts_budget_above_threshold() {
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; cap[1] = 0x50; cap[2] = 0x55;

    let preimage = build_requester_preimage(7, node_id, cap, 3, 500, nonce);
    let commit = hash_requester_preimage(preimage);

    // threshold=250, budget=500. 500 >= 250 holds.
    main(7, node_id, commit, cap, 3, 250, 500, nonce);
}

#[test(should_fail)]
fn requester_rejects_budget_below_threshold() {
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; cap[1] = 0x50; cap[2] = 0x55;

    let preimage = build_requester_preimage(0, node_id, cap, 3, 99, nonce);
    let commit = hash_requester_preimage(preimage);

    // Budget 99, threshold 100 - should fail.
    main(0, node_id, commit, cap, 3, 100, 99, nonce);
}

#[test(should_fail)]
fn requester_rejects_wrong_round_public_input() {
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; cap[1] = 0x50; cap[2] = 0x55;

    // Commitment bound to round 0.
    let preimage = build_requester_preimage(0, node_id, cap, 3, 500, nonce);
    let commit = hash_requester_preimage(preimage);

    // Attempt to verify against public round 1 - commitment mismatch.
    main(1, node_id, commit, cap, 3, 100, 500, nonce);
}

#[test(should_fail)]
fn requester_rejects_tampered_commitment_hash() {
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; cap[1] = 0x50; cap[2] = 0x55;

    let preimage = build_requester_preimage(0, node_id, cap, 3, 500, nonce);
    let mut commit = hash_requester_preimage(preimage);
    // Flip one bit in the published commitment.
    commit[0] = commit[0] ^ 0x01;

    main(0, node_id, commit, cap, 3, 100, 500, nonce);
}
