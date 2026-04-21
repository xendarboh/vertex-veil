//! Shared commitment layout helpers.
//!
//! These builders reproduce the canonical byte layouts documented in
//! `crates/vertex-veil-core/src/commitments.rs`. The Rust helper
//! `build_requester_preimage` and this library's `build_requester_preimage`
//! MUST produce identical bytes for the same logical inputs; the
//! Rust-side integration tests assert parity over shared fixtures.

global COMMIT_DOMAIN_REQUESTER: [u8; 31] = [
    // "vertex-veil/v1/commit-requester"
    0x76, 0x65, 0x72, 0x74, 0x65, 0x78, 0x2d, 0x76, 0x65, 0x69, 0x6c, 0x2f, 0x76, 0x31,
    0x2f, 0x63, 0x6f, 0x6d, 0x6d, 0x69, 0x74, 0x2d, 0x72, 0x65, 0x71, 0x75, 0x65, 0x73,
    0x74, 0x65, 0x72,
];

global COMMIT_DOMAIN_PROVIDER: [u8; 30] = [
    // "vertex-veil/v1/commit-provider"
    0x76, 0x65, 0x72, 0x74, 0x65, 0x78, 0x2d, 0x76, 0x65, 0x69, 0x6c, 0x2f, 0x76, 0x31,
    0x2f, 0x63, 0x6f, 0x6d, 0x6d, 0x69, 0x74, 0x2d, 0x70, 0x72, 0x6f, 0x76, 0x69, 0x64,
    0x65, 0x72,
];

global COMMIT_SCHEMA_VERSION: u8 = 1;

pub global MAX_CAPABILITY_BYTES: u32 = 32;
pub global MAX_CAPABILITY_CLAIMS: u32 = 4;
pub global REQUESTER_PREIMAGE_LEN: u32 = 153;
pub global PROVIDER_PREIMAGE_LEN: u32 = 264;

/// Write the 8 bytes of `value` (big-endian u64) into `buf` starting at `pos`.
fn write_u64_be(mut buf: [u8; 264], pos: u32, value: u64) -> [u8; 264] {
    buf[pos + 0] = ((value >> 56) & 0xff) as u8;
    buf[pos + 1] = ((value >> 48) & 0xff) as u8;
    buf[pos + 2] = ((value >> 40) & 0xff) as u8;
    buf[pos + 3] = ((value >> 32) & 0xff) as u8;
    buf[pos + 4] = ((value >> 24) & 0xff) as u8;
    buf[pos + 5] = ((value >> 16) & 0xff) as u8;
    buf[pos + 6] = ((value >> 8) & 0xff) as u8;
    buf[pos + 7] = (value & 0xff) as u8;
    buf
}

fn write_u32_be(mut buf: [u8; 264], pos: u32, value: u32) -> [u8; 264] {
    buf[pos + 0] = ((value >> 24) & 0xff) as u8;
    buf[pos + 1] = ((value >> 16) & 0xff) as u8;
    buf[pos + 2] = ((value >> 8) & 0xff) as u8;
    buf[pos + 3] = (value & 0xff) as u8;
    buf
}

/// Build the canonical 153-byte requester preimage.
///
/// * `round`                 public round id
/// * `node_id`               32-byte public key
/// * `required_capability`   32-byte buffer (zero-padded) containing the
///                           required capability tag
/// * `capability_len`        actual byte length of the capability tag (1..=32)
/// * `budget_cents`          private budget value
/// * `nonce`                 32-byte per-round nonce
pub fn build_requester_preimage(
    round: u64,
    node_id: [u8; 32],
    required_capability: [u8; 32],
    capability_len: u32,
    budget_cents: u64,
    nonce: [u8; 32],
) -> [u8; 153] {
    // Use a buffer sized to the larger of the two preimages so the helpers
    // can be shared; the returned slice is trimmed to requester length.
    let mut big: [u8; 264] = [0; 264];

    // [0..4) u32 BE domain_len = 31
    big = write_u32_be(big, 0, 31);
    // [4..35) domain bytes (31 bytes)
    for i in 0..31 {
        big[4 + i] = COMMIT_DOMAIN_REQUESTER[i];
    }
    // [35] schema_version
    big[35] = COMMIT_SCHEMA_VERSION;
    // [36..44) round (u64 BE)
    big = write_u64_be(big, 36, round);
    // [44..76) node_id
    for i in 0..32 {
        big[44 + i] = node_id[i];
    }
    // [76] role = 0 (already zero, but be explicit)
    big[76] = 0;
    // [77..81) cap_len (u32 BE)
    big = write_u32_be(big, 77, capability_len);
    // [81..113) cap_bytes padded to 32
    for i in 0..32 {
        big[81 + i] = required_capability[i];
    }
    // [113..121) budget (u64 BE)
    big = write_u64_be(big, 113, budget_cents);
    // [121..153) nonce
    for i in 0..32 {
        big[121 + i] = nonce[i];
    }

    // Trim to requester-sized buffer.
    let mut out: [u8; 153] = [0; 153];
    for i in 0..153 {
        out[i] = big[i];
    }
    out
}

/// Build the canonical 264-byte provider preimage.
///
/// * `round`                 public round id
/// * `node_id`               32-byte public key
/// * `claim_bytes`           4 slots  32-byte capability bytes (zero-padded)
/// * `claim_lens`            4 actual claim byte lengths (0 for unused slots)
/// * `n_claims`              actual number of claims (1..=4)
/// * `reservation_cents`     private reservation value
/// * `nonce`                 32-byte per-round nonce
pub fn build_provider_preimage(
    round: u64,
    node_id: [u8; 32],
    claim_bytes: [[u8; 32]; 4],
    claim_lens: [u32; 4],
    n_claims: u32,
    reservation_cents: u64,
    nonce: [u8; 32],
) -> [u8; 264] {
    let mut big: [u8; 264] = [0; 264];

    // [0..4) u32 BE domain_len = 30
    big = write_u32_be(big, 0, 30);
    // [4..34) domain bytes (30 bytes)
    for i in 0..30 {
        big[4 + i] = COMMIT_DOMAIN_PROVIDER[i];
    }
    // [34] schema_version
    big[34] = COMMIT_SCHEMA_VERSION;
    // [35..43) round (u64 BE)
    big = write_u64_be(big, 35, round);
    // [43..75) node_id
    for i in 0..32 {
        big[43 + i] = node_id[i];
    }
    // [75] role = 1
    big[75] = 1;
    // [76..80) n_claims (u32 BE)
    big = write_u32_be(big, 76, n_claims);
    // [80..224) claim slots - 4 slots of 36 bytes each
    //   slot layout: [0..4) u32 BE len, [4..36) claim bytes padded
    //   unused slots stay all-zero because big was zero-initialized
    //   AND we only write into slots 0..n_claims-1 via runtime index - we
    //   use a constant-range loop and gate with n_claims to keep the
    //   circuit shape static.
    for slot_idx in 0..4 {
        let slot_start = 80 + slot_idx * 36;
        let active = (slot_idx as u32) < n_claims;
        // Write len (zero if inactive)
        let len_if_active = claim_lens[slot_idx];
        let len = if active { len_if_active } else { 0 };
        big = write_u32_be(big, slot_start, len);
        // Write bytes (zero if inactive)
        for i in 0..32 {
            let b_if_active = claim_bytes[slot_idx][i];
            big[slot_start + 4 + i] = if active { b_if_active } else { 0 };
        }
    }
    // [224..232) reservation (u64 BE)
    big = write_u64_be(big, 224, reservation_cents);
    // [232..264) nonce
    for i in 0..32 {
        big[232 + i] = nonce[i];
    }

    big
}

pub fn hash_requester_preimage(preimage: [u8; 153]) -> [u8; 32] {
    std::hash::blake2s(preimage)
}

pub fn hash_provider_preimage(preimage: [u8; 264]) -> [u8; 32] {
    std::hash::blake2s(preimage)
}

#[test]
fn requester_preimage_layout_smoke() {
    let node_id: [u8; 32] = [0x11; 32];
    let nonce: [u8; 32] = [0xa1; 32];
    let mut cap: [u8; 32] = [0; 32];
    cap[0] = 0x47; // 'G'
    cap[1] = 0x50; // 'P'
    cap[2] = 0x55; // 'U'

    let preimage = build_requester_preimage(3, node_id, cap, 3, 100, nonce);
    // Domain length prefix lives at offset 0.
    assert(preimage[0] == 0);
    assert(preimage[1] == 0);
    assert(preimage[2] == 0);
    assert(preimage[3] == 31);
    // Schema version at 35.
    assert(preimage[35] == 1);
    // Role byte at 76.
    assert(preimage[76] == 0);
    // Capability length prefix big-endian u32 = 3  [0,0,0,3]
    assert(preimage[77] == 0);
    assert(preimage[78] == 0);
    assert(preimage[79] == 0);
    assert(preimage[80] == 3);
    // Capability bytes at 81..83 are 'G','P','U'.
    assert(preimage[81] == 0x47);
    assert(preimage[82] == 0x50);
    assert(preimage[83] == 0x55);
}

#[test]
fn provider_preimage_layout_smoke() {
    let node_id: [u8; 32] = [0x22; 32];
    let nonce: [u8; 32] = [0xb2; 32];
    let mut slot0: [u8; 32] = [0; 32];
    slot0[0] = 0x47; slot0[1] = 0x50; slot0[2] = 0x55; // "GPU"
    let slot1: [u8; 32] = [0; 32];
    let slot2: [u8; 32] = [0; 32];
    let slot3: [u8; 32] = [0; 32];
    let claim_bytes: [[u8; 32]; 4] = [slot0, slot1, slot2, slot3];
    let claim_lens: [u32; 4] = [3, 0, 0, 0];

    let preimage =
        build_provider_preimage(3, node_id, claim_bytes, claim_lens, 1, 50, nonce);
    // Domain length prefix is 30.
    assert(preimage[3] == 30);
    // Role byte at 75 = 1.
    assert(preimage[75] == 1);
    // n_claims big-endian u32 = 1 at offsets 76..80.
    assert(preimage[79] == 1);
    // First slot claim_len = 3 at offsets 80..84.
    assert(preimage[83] == 3);
    // First slot claim_bytes[0] at offset 84.
    assert(preimage[84] == 0x47);
    // Nonce byte at offset 232.
    assert(preimage[232] == 0xb2);
}
