use super::*;
use std::io::{Cursor, Write};

pub(crate) fn fixture() -> (Vec<u8>, Vec<u8>) {
    let mut header = vec![0; 40];
    header[8..12].copy_from_slice(&32_u32.to_le_bytes());
    header[12..16].copy_from_slice(&1_u32.to_le_bytes());
    header[24..28].copy_from_slice(&24_u32.to_le_bytes());
    header[28..32].copy_from_slice(&2_u32.to_le_bytes());
    header.extend(name_hash("test.bin").to_le_bytes());
    header.extend(0_u32.to_le_bytes());
    for (name, offset) in [("test.bin", 0_u64), ("orphan.bin", 3)] {
        header.extend(3_u64.to_le_bytes());
        header.extend(3_u64.to_le_bytes());
        header.extend(offset.to_le_bytes());
        header.extend(name_hash(name).to_le_bytes());
        header.extend(b"BIN\0\0\0\0\0\0\0");
        header.extend([3, 0]);
    }
    (header, b"abcxyz".to_vec())
}

#[test]
fn physical_records_include_unindexed_resources() -> Result<()> {
    let (bytes, data) = fixture();
    let directory = Directory::parse(&bytes, data.len() as u64)?;
    assert_eq!(directory.entries.len(), 2);
    assert_eq!(
        directory
            .find("TEST.BIN")
            .unwrap()
            .read(&mut Cursor::new(&data))?,
        b"abc"
    );
    assert!(directory.find("orphan.bin").is_none());
    assert!(!directory.entries[1].indexed);
    assert_eq!(directory.entries[1].read(&mut Cursor::new(&data))?, b"xyz");
    // These names and hashes occur in the installed source's own references.
    assert_eq!(name_hash("BTL_EV002.SO"), 0xb3f9_dff7);
    assert_eq!(name_hash("FC_W236.SKP"), 0xba29_f632);
    Ok(())
}

#[test]
fn directory_rejects_truncation_bad_lookup_and_data_ranges() {
    let (header, data) = fixture();
    for end in 0..header.len() {
        assert!(
            Directory::parse(&header[..end], data.len() as u64).is_err(),
            "length {end}"
        );
    }
    for (at, value) in [
        (8, u32::MAX),
        (24, u32::MAX),
        (28, u32::MAX),
        (44, 2),
        (40, 0),
    ] {
        let mut broken = header.clone();
        broken[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            Directory::parse(&broken, data.len() as u64).is_err(),
            "offset {at}"
        );
    }
    assert!(Directory::parse(&header, 5).is_err());
    let mut broken = header;
    broken[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Directory::parse(&broken, u64::MAX).is_err());
}

fn tlzc(payload: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(payload)?;
    let compressed = encoder.finish()?;
    let mut bytes = b"TLZC\x01\x02\0\0".to_vec();
    bytes.extend((24_u32 + compressed.len() as u32).to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend(compressed);
    Ok(bytes)
}

#[test]
fn tlzc_checks_decoder_completion_and_both_lengths() -> Result<()> {
    let payload = b"hello Steam resource".repeat(500);
    let bytes = tlzc(&payload)?;
    assert_eq!(decode_tlzc(&bytes, payload.len() as u64)?, payload);
    for end in 0..bytes.len() {
        let mut truncated = bytes[..end].to_vec();
        if end >= 12 {
            truncated[8..12].copy_from_slice(&(end as u32).to_le_bytes());
        }
        assert!(
            decode_tlzc(&truncated, payload.len() as u64).is_err(),
            "length {end}"
        );
    }
    assert!(decode_tlzc(&bytes, payload.len() as u64 - 1).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    let size = trailing.len() as u32;
    trailing[8..12].copy_from_slice(&size.to_le_bytes());
    assert!(decode_tlzc(&trailing, payload.len() as u64).is_err());
    let mut oversized = bytes;
    oversized[12..16].copy_from_slice(&((RESOURCE_LIMIT + 1) as u32).to_le_bytes());
    assert!(decode_tlzc(&oversized, RESOURCE_LIMIT + 1).is_err());
    Ok(())
}
