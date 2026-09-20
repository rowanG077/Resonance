use super::*;

fn put_half(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
fn put_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
fn put_names(bytes: &mut Vec<u8>, pointer: usize, names: &[&[u8]]) {
    let offset = bytes.len() as u32;
    put_word(bytes, pointer, offset);
    for name in names {
        bytes.extend_from_slice(name);
        bytes.push(0);
    }
    if pointer == 20 {
        let size = bytes.len() as u32 - offset;
        put_word(bytes, 16, size);
    } else {
        put_word(bytes, 24, 1);
    }
}

fn fixtures(ids: &[u16], nodes: &[u16]) -> (Vec<u8>, Vec<u8>) {
    let keys = 36 + ids.len() * 16;
    let values = keys + ids.len() * 12;
    let mut anm = vec![0; values + ids.len() * 6];
    put_word(&mut anm, 0, 0x007b7960);
    put_word(&mut anm, 4, 24);
    put_half(&mut anm, 10, 1);
    put_half(&mut anm, 12, ids.len() as u16);
    put_half(&mut anm, 14, ids.len() as u16);
    put_word(&mut anm, 28, 36);
    put_half(&mut anm, 32, ids.len() as u16);
    for (index, &id) in ids.iter().enumerate() {
        let row = 36 + index * 16;
        let key = keys + index * 12;
        let value = values + index * 6;
        put_word(&mut anm, row, (10. * (index + 1) as f32).to_bits());
        put_word(&mut anm, row + 4, key as u32);
        put_half(&mut anm, row + 8, 1);
        put_half(&mut anm, row + 10, id);
        anm[row + 12..row + 16].copy_from_slice(&[0x30, 1, 0, 1]);
        put_word(&mut anm, key + 4, value as u32);
        put_half(&mut anm, value, (index + 1) as u16);
    }
    let mut model = vec![0; 32 + nodes.len() * 28];
    put_word(&mut model, 0, 0x007b7960);
    put_half(&mut model, 6, nodes.len() as u16);
    put_word(&mut model, 12, if nodes.is_empty() { 0 } else { 32 });
    for (index, &id) in nodes.iter().enumerate() {
        put_half(&mut model, 32 + index * 28 + 22, id);
        if index + 1 < nodes.len() {
            put_word(
                &mut model,
                32 + index * 28 + 8,
                (32 + (index + 1) * 28) as u32,
            );
        }
    }
    (anm, model)
}

fn sampled(motion: &Motion) -> Vec<(u16, f32)> {
    motion
        .tracks
        .iter()
        .map(|track| {
            (
                track.bone,
                track.sample(0., Transform::default()).unwrap().translation[0],
            )
        })
        .collect()
}

#[test]
fn numeric_aliases_share_the_first_descriptor_and_keep_the_full_period() {
    let (anm, model) = fixtures(&[7, 7, 11], &[7, 7, 11, 19]);
    let result = motion(&anm, &model).unwrap();
    assert_eq!(sampled(&result), [(0, 1.), (1, 1.), (2, 3.)]);
    assert_eq!(result.duration_frames, 30.);
    // An external clip can play on another actor, including a rig that binds
    // no tracks. Its clock belongs to the source clip, not the selected bones.
    for (nodes, expected) in [(vec![11, 7], vec![(0, 3.), (1, 1.)]), (vec![19], vec![])] {
        let (_, model) = fixtures(&[7, 7, 11], &nodes);
        let rebound = motion(&anm, &model).unwrap();
        assert_eq!(sampled(&rebound), expected);
        assert_eq!(rebound.duration_frames, result.duration_frames);
    }
}

#[test]
fn named_bindings_use_exact_first_matches_and_only_fall_back_when_all_are_missing() {
    let (mut anm, mut model) = fixtures(&[0, 1, 2], &[0, 1, 2]);
    put_names(&mut anm, 20, &[b"root", b"root", b"HEAD"]);
    put_names(&mut model, 28, &[b"root", b"root", b"head"]);
    // Both root descriptors resolve to the first model root; the second curve
    // is shadowed. Case-sensitive HEAD is unbound, without numeric fallback.
    let result = motion(&anm, &model).unwrap();
    assert_eq!(sampled(&result), [(0, 1.)]);
    assert_eq!(result.duration_frames, 30.);
    put_names(&mut anm, 20, &[b"ROOT", b"ROOT", b"HEAD"]);
    assert_eq!(
        sampled(&motion(&anm, &model).unwrap()),
        [(0, 1.), (1, 2.), (2, 3.)]
    );
}

#[test]
fn reordered_model_nodes_keep_names_ids_meshes_and_motion_in_the_same_order() {
    let (mut anm, mut model) = fixtures(&[7, 8, 9], &[1, 0, 2]);
    put_word(&mut model, 12, 60);
    put_word(&mut model, 40, 0);
    put_word(&mut model, 48, 88);
    put_word(&mut model, 68, 32);
    put_names(&mut model, 28, &[b"first", b"second", b"third"]);
    put_names(&mut anm, 20, &[b"third", b"first", b"second"]);
    let parsed = crate::model::Model::parse(&model).unwrap();
    let nodes = crate::geometry::model_node_info(&parsed);
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert_eq!(ModelBindings::read(&model).unwrap().node_ids, [0, 1, 2]);
    assert_eq!(
        sampled(&motion(&anm, &model).unwrap()),
        [(0, 2.), (1, 3.), (2, 1.)]
    );
}
