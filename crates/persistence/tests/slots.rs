use resonance_persistence::*;

fn header() -> Header {
    Header {
        identity: Identity {
            schema: 1,
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
        assert_eq!(
            decode::<Vec<u32>>(&store.read(kind, &second).unwrap(), &header.identity).unwrap(),
            (header.clone(), vec![4, 5])
        );
        assert!(store.write(kind, &first, b"incomplete write").is_err());
        assert_eq!(store.read(kind, &first).unwrap(), bytes);
        store.write(kind, &first, &replacement).unwrap();
        assert_eq!(store.read(kind, &first).unwrap(), replacement);
        let mut incompatible = header.identity.clone();
        incompatible.schema += 1;
        assert!(decode::<Vec<u32>>(&bytes, &incompatible).is_err());
        incompatible = header.identity.clone();
        incompatible.content[0] ^= 1;
        assert!(decode::<Vec<u32>>(&bytes, &incompatible).is_err());
        assert!(inspect(&bytes[..bytes.len() - 1]).is_err());
    }
    for name in ["", "../outside", "/absolute", "x/y", "x\\y", ".", "a.b"] {
        assert!(SlotId::new(name).is_err());
    }
    assert!(inspect(&vec![b' '; MAX_FILE_BYTES + 1]).is_err());
}
