use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Ali2Entry {
    pub content_hash: u64,
    pub crypt_key_hash: u64,
    pub key_id_hash: u64,
    pub plaintext_size: u64,
}

/// Parses an ALI2 index (FlatBuffers cleartext).
pub fn parse_ali2_index(data: &[u8]) -> Result<Vec<Ali2Entry>, String> {
    if data.len() < 8 || &data[0..8] != b"\x18\x00\x00\x00ALI2" {
        return Err(format!(
            "ALI2 magic not found: {}",
            hex_prefix(data)
        ));
    }

    let root_off: usize = 24;
    if data.len() < root_off + 4 {
        return Err("Index too short".into());
    }
    let vtable_soff = i32::from_le_bytes(data[root_off..root_off + 4].try_into().unwrap());
    if vtable_soff < 0 || vtable_soff as usize > root_off {
        return Err(format!("invalid vtable_soff: {}", vtable_soff));
    }
    let vtable_start = root_off - vtable_soff as usize;

    if data.len() < vtable_start + 6 {
        return Err("Vtable out of bounds".into());
    }
    let foff = u16::from_le_bytes(data[vtable_start + 4..vtable_start + 6].try_into().unwrap())
        as usize;
    if foff == 0 {
        return Ok(vec![]);
    }

    let vec_offset_ref = root_off + foff;
    if data.len() < vec_offset_ref + 4 {
        return Err("vec_offset_ref out of bounds".into());
    }
    let vec_offset =
        u32::from_le_bytes(data[vec_offset_ref..vec_offset_ref + 4].try_into().unwrap()) as usize;
    let vec_start = vec_offset_ref + vec_offset;
    if data.len() < vec_start + 4 {
        return Err("vec_start out of bounds".into());
    }
    let count = u32::from_le_bytes(data[vec_start..vec_start + 4].try_into().unwrap()) as usize;

    const ENTRY_SIZE: usize = 48;
    let entry_start = vec_start + 4;
    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let off = entry_start + i * ENTRY_SIZE;
        if off + ENTRY_SIZE > data.len() {
            return Err(format!(
                "Entry {} out of bounds (off={}, data.len={})",
                i, off, data.len()
            ));
        }
        // Entry layout: content_hash[0..8], plaintext_size[8..16], crypt_key_hash[16..24],
        //               <reserved>[24..32], key_id_hash[32..40], <padding>[40..48]
        let content_hash   = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        let plaintext_size = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
        let crypt_key_hash = u64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());
        let key_id_hash    = u64::from_le_bytes(data[off + 32..off + 40].try_into().unwrap());
        entries.push(Ali2Entry {
            content_hash,
            crypt_key_hash,
            key_id_hash,
            plaintext_size,
        });
    }

    Ok(entries)
}

/// Builds a cryptKeyHash → Ali2Entry lookup from an ALI2 index.
pub fn build_ali2_lookup(data: &[u8]) -> Result<HashMap<u64, Ali2Entry>, String> {
    Ok(parse_ali2_index(data)?
        .into_iter()
        .map(|e| (e.crypt_key_hash, e))
        .collect())
}

fn hex_prefix(data: &[u8]) -> String {
    data.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ali2_minimal(entries: &[(u64, u64, u64, u64)]) -> Vec<u8> {
        // Builds a minimal in-memory ALI2 index for tests.
        // Header: magic(8) + padding(16) = 24 bytes up to root_off
        // Simplified FlatBuffers: root_off=24, vtable_soff=8, vtable_start=16
        // vtable[16..18] = size=8, vtable[18..20] = data_size=0, vtable[20..22] = foff=4
        // root_off + foff = 24 + 4 = 28 → vec_offset_ref
        // vec_offset=4 → vec_start = 32
        // vec_start: count(4) + entries(count*48)
        let count = entries.len() as u32;
        let mut buf = vec![0u8; 36 + entries.len() * 48];
        // Magic
        buf[0..8].copy_from_slice(b"\x18\x00\x00\x00ALI2");
        // root_off=24: vtable_soff = 24-16 = 8 (i32 LE)
        buf[24..28].copy_from_slice(&8i32.to_le_bytes());
        // vtable at 16: size=8(2), data_size=0(2), foff=4(2) (vtable_start+4)
        buf[16..18].copy_from_slice(&8u16.to_le_bytes());
        buf[20..22].copy_from_slice(&4u16.to_le_bytes());
        // vec_offset_ref = 28, vec_offset = 4 → vec_start=32
        buf[28..32].copy_from_slice(&4u32.to_le_bytes());
        // vec_start=32: count
        buf[32..36].copy_from_slice(&count.to_le_bytes());
        // entries
        for (i, &(ch, ps, ckh, kih)) in entries.iter().enumerate() {
            let off = 36 + i * 48;
            buf[off..off+8].copy_from_slice(&ch.to_le_bytes());
            buf[off+8..off+16].copy_from_slice(&ps.to_le_bytes());
            buf[off+16..off+24].copy_from_slice(&ckh.to_le_bytes());
            buf[off+32..off+40].copy_from_slice(&kih.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_ali2_empty() {
        let buf = make_ali2_minimal(&[]);
        let entries = parse_ali2_index(&buf).unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_parse_ali2_single_entry() {
        let ch = 0x6a2aa5848a7313adu64;
        let ckh = 0x0da9778d9b6bed0au64;
        let kih = 0xcf461af74368f659u64;
        let ps = 1234u64;
        let buf = make_ali2_minimal(&[(ch, ps, ckh, kih)]);
        let entries = parse_ali2_index(&buf).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content_hash, ch);
        assert_eq!(entries[0].crypt_key_hash, ckh);
        assert_eq!(entries[0].key_id_hash, kih);
        assert_eq!(entries[0].plaintext_size, ps);
    }

    #[test]
    fn test_build_lookup() {
        let ckh = 0x0da9778d9b6bed0au64;
        let buf = make_ali2_minimal(&[(0xaabb, 100, ckh, 0xccdd)]);
        let map = build_ali2_lookup(&buf).unwrap();
        assert!(map.contains_key(&ckh));
        assert_eq!(map[&ckh].content_hash, 0xaabb);
    }

    #[test]
    fn test_bad_magic() {
        let buf = b"BADMAGIC01234567".to_vec();
        assert!(parse_ali2_index(&buf).is_err());
    }
}
