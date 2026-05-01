const SIGMA: &[u8; 16] = b"A3AxwtfWD<PbxMx$";
const BLOB_NONCE_FILL: u32 = 0x63686368;
const KEY_NONCE_FILL: u32 = 0x00000000;

pub const GLOBAL_KEY: [u8; 32] = [
    0xcf, 0xd3, 0xf5, 0xcb, 0x76, 0x0b, 0x81, 0xe7,
    0x3b, 0x3d, 0x3b, 0x41, 0x17, 0x3f, 0x11, 0x5f,
    0xe0, 0x42, 0x94, 0x9d, 0xc4, 0x60, 0x42, 0x74,
    0x9e, 0xcc, 0x87, 0x7d, 0x58, 0xd2, 0x29, 0x6c,
];

// Hardcoded into libil2cpp.so as <PrivateImplementationDetails>.D051B739CB...215BC5DB.
// keyId="MasterData" → xxHash64 = 0x075b2dfa1a1e4302. Loaded into an InMemoryCryptKeyRepository
// (no on-disk encrypted file like DefaultMasterData). SHA-256 of the bytes below = the field name.
pub const ALADIN_KEY_BODY: [u8; 32] = [
    0xf9, 0x61, 0x06, 0x6d, 0x49, 0xfe, 0xd6, 0xf5,
    0x96, 0x39, 0xea, 0x91, 0x7e, 0x28, 0x66, 0x02,
    0xf5, 0x72, 0x99, 0x8e, 0xd1, 0x6b, 0x73, 0xf9,
    0xc6, 0x51, 0x74, 0x90, 0xba, 0x87, 0xe6, 0x1b,
];

const TURNS_TABLE: [u8; 16] = [6, 5, 6, 5, 5, 6, 5, 6, 6, 6, 5, 5, 5, 6, 6, 5];

fn read_le_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]); s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]); s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]); s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]); s[b] = (s[b] ^ s[c]).rotate_left(7);
}

fn acp_init_state(key: &[u8; 32], nonce: &[u8; 12]) -> [u32; 16] {
    let mut s = [0u32; 16];
    s[0]  = read_le_u32(SIGMA, 0);  s[1]  = read_le_u32(SIGMA, 4);
    s[2]  = read_le_u32(SIGMA, 8);  s[3]  = read_le_u32(SIGMA, 12);
    s[4]  = read_le_u32(key, 0);    s[5]  = read_le_u32(key, 4);
    s[6]  = read_le_u32(key, 8);    s[7]  = read_le_u32(key, 12);
    s[8]  = read_le_u32(key, 16);   s[9]  = read_le_u32(key, 20);
    s[10] = read_le_u32(key, 24);   s[11] = read_le_u32(key, 28);
    s[12] = 0;
    s[13] = read_le_u32(nonce, 0);
    s[14] = read_le_u32(nonce, 4);
    s[15] = read_le_u32(nonce, 8);
    s
}

fn compute_max_turn(key: &[u8; 32], nonce: &[u8; 12]) -> usize {
    let k0 = read_le_u32(key, 0);
    let k5 = read_le_u32(key, 20);
    let k7 = read_le_u32(key, 28);
    let n0 = read_le_u32(nonce, 0);
    let n1 = read_le_u32(nonce, 4);
    let n2 = read_le_u32(nonce, 8);
    // NOTE: Python `+` has higher precedence than `^`, so `k5 + k0 ^ k7` = `(k5+k0)^k7`.
    // The plan doc had this wrong (k5 + (k0^k7)). Verified correct via Frida test vector.
    let u = ((k5.wrapping_add(k0)) ^ k7).wrapping_add((n1.wrapping_add(n0)) ^ n2);
    let idx = ((u >> 7) & 2) | ((u >> 2) & 1) | ((u >> 13) & 4) | ((u >> 2) & 8);
    TURNS_TABLE[idx as usize] as usize
}

fn acp_block(initial: &[u32; 16], counter: u32, max_turn: usize) -> [u8; 64] {
    let mut w = *initial;
    // ACP counter is 1-based: caller passes 0,1,2,... and the block uses 1,2,3,...
    let counter = counter.wrapping_add(1);
    w[12] = counter;
    for _ in 0..max_turn {
        quarter_round(&mut w, 0, 4,  8, 12);
        quarter_round(&mut w, 1, 5,  9, 13);
        quarter_round(&mut w, 2, 6, 10, 14);
        quarter_round(&mut w, 3, 7, 11, 15);
        quarter_round(&mut w, 0, 5, 10, 15);
        quarter_round(&mut w, 1, 6, 11, 12);
        quarter_round(&mut w, 2, 7,  8, 13);
        quarter_round(&mut w, 3, 4,  9, 14);
    }
    for i in 0..16usize {
        w[i] = w[i].wrapping_add(if i == 12 { counter } else { initial[i] });
    }
    let mut out = [0u8; 64];
    for (i, word) in w.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Encrypts/decrypts `data` with custom ChaCha20 (counter_start=0).
pub fn acp_transform(key: &[u8; 32], nonce: &[u8; 12], data: &[u8]) -> Vec<u8> {
    let state = acp_init_state(key, nonce);
    let max_turn = compute_max_turn(key, nonce);
    let mut result = Vec::with_capacity(data.len());
    let mut counter: u32 = 0;
    let mut offset = 0;
    while offset < data.len() {
        let ks = acp_block(&state, counter, max_turn);
        counter = counter.wrapping_add(1);
        let chunk = (data.len() - offset).min(64);
        for i in 0..chunk {
            result.push(data[offset + i] ^ ks[i]);
        }
        offset += chunk;
    }
    result
}

pub fn make_blob_nonce(content_hash: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&content_hash.to_le_bytes());
    n[8..].copy_from_slice(&BLOB_NONCE_FILL.to_le_bytes());
    n
}

pub fn make_key_nonce(key_id_hash: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&key_id_hash.to_le_bytes());
    n[8..].copy_from_slice(&KEY_NONCE_FILL.to_le_bytes());
    n
}

/// Decrypts a .aladin data blob.
pub fn decrypt_blob(ciphertext: &[u8], key_body: &[u8; 32], content_hash: u64) -> Vec<u8> {
    let nonce = make_blob_nonce(content_hash);
    acp_transform(key_body, &nonce, ciphertext)
}

/// Decrypts a 32-byte key file (DefaultMasterData).
pub fn decrypt_key_file(
    encrypted_key: &[u8; 32],
    global_key: &[u8; 32],
    key_id_hash: u64,
) -> [u8; 32] {
    let nonce = make_key_nonce(key_id_hash);
    // Safety: acp_transform preserves input length; input is [u8;32] → output is 32 bytes.
    acp_transform(global_key, &nonce, encrypted_key)
        .try_into()
        .unwrap_or_else(|_| unreachable!("acp_transform preserves input length"))
}

/// Parses the hex stem of a .aladin filename to u64.
pub fn filename_to_hash(stem: &str) -> Result<u64, std::num::ParseIntError> {
    u64::from_str_radix(stem, 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acp_transform_frida_vector() {
        // Test vector captured via Frida Transform #1 (session 2025-04-14)
        let key_body = hex_to_bytes32(
            "f961066d49fed6f59639ea917e286602f572998ed16b73f9c6517490ba87e61b"
        );
        let content_hash: u64 = 0x6a2aa5848a7313ad;
        let nonce = make_blob_nonce(content_hash);
        let cipher = hex_to_vec(concat!(
            "9b216084ee6cc6e0ef6e263427f7651bc2a3f875625e3650b486d4cc7c6c50e8",
            "4dacef3b7e2fcbacd15b32600715bbb1404420551cd16c1dee4a48cb4a38ad8a"
        ));
        let expected = hex_to_vec(
            "de00b8a7506f6b656d6f6e92d200000000d20000e2bcb6436f6c6c656374696f6e426f6172644861736854616792d20000e2bcd200000107b646656564537461"
        );
        let result = acp_transform(&key_body, &nonce, &cipher);
        assert_eq!(result, expected, "ChaCha20 custom vector mismatch");
        assert!(result.windows(7).any(|w| w == b"Pokemon"), "Pokemon not found in plaintext");
    }

    #[test]
    fn test_make_blob_nonce() {
        let hash: u64 = 0x0b12c56ceea1835b;
        let nonce = make_blob_nonce(hash);
        let expected = {
            let mut n = [0u8; 12];
            n[..8].copy_from_slice(&0x0b12c56ceea1835bu64.to_le_bytes());
            n[8..].copy_from_slice(&0x63686368u32.to_le_bytes());
            n
        };
        assert_eq!(nonce, expected);
    }

    #[test]
    fn test_symmetry() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let nonce = make_blob_nonce(0x1234567890abcdef);
        let data = b"Hello, Pokemon TCGP!Hello, Pokemon TCGP!Hello, Pokemon TCGP!";
        let cipher = acp_transform(&key, &nonce, data);
        let plain = acp_transform(&key, &nonce, &cipher);
        assert_eq!(plain, data);
    }

    fn hex_to_bytes32(s: &str) -> [u8; 32] {
        let v = hex_to_vec(s);
        v.try_into().expect("not 32 bytes")
    }

    fn hex_to_vec(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
