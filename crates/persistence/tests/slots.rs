use resonance_persistence::*;

fn header() -> Header {
    Header {
        identity: Identity {
            schema: resonance_content::save_identity::SCHEMA,
            content: [7; 32],
        },
        label: "School courtyard".into(),
        location: "Iselia".into(),
        played_ticks: 12345,
        saved_unix_seconds: 1700000000,
    }
}

#[test]
fn slots_are_independent_and_invalid_writes_preserve_previous_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let first = SlotId::new("1").unwrap();
    let second = SlotId::new("courtyard").unwrap();
    let header = header();
    for kind in [Kind::Save, Kind::Quicksave] {
        assert!(store.list(kind).unwrap().is_empty());
        let bytes = encode(&header, &vec![1u32, 2, 3]).unwrap();
        store.write(kind, &first, &bytes).unwrap();
        let replacement = encode(&header, &vec![4u32, 5]).unwrap();
        store
            .write_async(kind, second.clone(), replacement.clone())
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(store.list(kind).unwrap(), [first.clone(), second.clone()]);
        assert_eq!(store.read(kind, &first).unwrap(), bytes);
        let saved = decode::<Vec<u32>>(&store.read(kind, &second).unwrap()).unwrap();
        assert_eq!(saved.state, [4, 5]);
        assert_eq!(
            saved.admit(&header.identity).unwrap(),
            (header.clone(), vec![4, 5])
        );
        assert!(store.write(kind, &first, b"incomplete write").is_err());
        assert_eq!(store.read(kind, &first).unwrap(), bytes);
        store.write(kind, &first, &replacement).unwrap();
        assert_eq!(store.read(kind, &first).unwrap(), replacement);
        let mut incompatible = header.identity.clone();
        incompatible.schema += 1;
        assert!(
            decode::<Vec<u32>>(&bytes)
                .unwrap()
                .admit(&incompatible)
                .is_err()
        );
        incompatible = header.identity.clone();
        incompatible.content[0] ^= 1;
        assert!(
            decode::<Vec<u32>>(&bytes)
                .unwrap()
                .admit(&incompatible)
                .is_err()
        );
        assert!(decode::<Vec<u32>>(&bytes[..bytes.len() - 1]).is_err());
    }
    for name in ["", "../outside", "/absolute", "x/y", "x\\y", ".", "a.b"] {
        assert!(SlotId::new(name).is_err());
    }
    assert!(decode::<Vec<u32>>(&vec![b' '; MAX_FILE_BYTES + 1]).is_err());
}

#[test]
fn obsolete_schemas_are_rejected_even_when_both_identities_match() {
    let current = header();
    let bytes = encode(&current, &vec![1u32, 2, 3]).unwrap();
    for schema in [current.identity.schema - 1, current.identity.schema + 1] {
        let mut unsupported = current.clone();
        unsupported.identity.schema = schema;
        assert!(encode(&unsupported, &vec![1u32, 2, 3]).is_err());
        let mut document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        document["header"]["identity"]["schema"] = schema.into();
        let obsolete = serde_json::to_vec(&document).unwrap();
        assert!(decode::<Vec<u32>>(&obsolete).is_err());
        assert!(
            Save {
                header: unsupported.clone(),
                state: vec![1u32, 2, 3]
            }
            .admit(&unsupported.identity)
            .is_err()
        );
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        let slot = SlotId::new("unchanged").unwrap();
        store.write(Kind::Save, &slot, &bytes).unwrap();
        assert!(store.write(Kind::Save, &slot, &obsolete).is_err());
        assert_eq!(store.read(Kind::Save, &slot).unwrap(), bytes);
    }
}
