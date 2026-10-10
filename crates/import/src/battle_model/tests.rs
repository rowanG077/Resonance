use super::*;

fn model(names: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0; 64 + names.len() * 28];
    let put = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes())
    };
    put(&mut bytes, 4, 32);
    put(&mut bytes, 32, 0x007b7960);
    bytes[38..40].copy_from_slice(&(names.len() as u16).to_be_bytes());
    put(&mut bytes, 44, 32);
    put(&mut bytes, 56, 1);
    put(&mut bytes, 60, (32 + names.len() * 28) as u32);
    for (index, name) in names.iter().enumerate() {
        if index + 1 < names.len() {
            put(
                &mut bytes,
                64 + index * 28 + 8,
                (32 + (index + 1) * 28) as u32,
            );
        }
        bytes[64 + index * 28 + 24] = 1;
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    let size = bytes.len() - 32;
    put(&mut bytes, 8, size as u32);
    bytes
}

#[test]
fn rig_rejects_unsupported_binary_transforms_before_publication() {
    let mut bytes = model(&["root"]);
    bytes[64 + 24] = 2;
    for kind in [RigKind::Body, RigKind::Weapon, RigKind::Effect] {
        assert!(
            rig(&bytes, kind)
                .unwrap_err()
                .to_string()
                .contains("unsupported battle model bone transform")
        );
    }
}

#[test]
fn rig_decodes_only_body_artwork_attachments() -> Result<()> {
    let source = model(&["root", "at", "at0/", "Kk00", "kk01"]);
    let body = rig(&source, RigKind::Body)?;
    assert_eq!(body.attachments[&0], 3);
    assert_eq!(body.attachments[&1], 4);
    for kind in [RigKind::Weapon, RigKind::Effect] {
        assert!(rig(&source, kind)?.attachments.is_empty());
    }
    assert!(rig(&model(&["kk"]), RigKind::Body).is_err());
    assert!(rig(&model(&["kk0/"]), RigKind::Body).is_err());
    assert!(rig(&source[..source.len() - 1], RigKind::Body).is_err());
    Ok(())
}
