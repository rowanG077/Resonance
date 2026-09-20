use super::*;

#[test]
#[ignore = "requires original US effects, model and palettes; no encoding"]
fn original_icicle_closes_every_particle_repeat_material_model_and_sound() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let mut cooker = test_cooker();
    let textures = member(&usual, 4).unwrap();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(textures, 1).unwrap()).unwrap(),
    );
    cooker
        .textures
        .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
    let id = EffectId {
        bank: EffectBank::Techniques,
        id: 32,
    };
    cooker.program(member(&usual, 3).unwrap(), id).unwrap();
    let root = cooker.result.program(id).unwrap();
    assert_eq!(root.end_tick, 66);
    assert_eq!(
        root.emissions
            .iter()
            .filter_map(|e| match e.command {
                EffectCommand::Sound { sound, priority } => Some((e.tick, sound, priority)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(6, 97, 0)]
    );
    assert_eq!(
        root.emissions
            .iter()
            .filter_map(|e| match &e.command {
                EffectCommand::Particle { actor, .. } =>
                    Some((e.tick, actor.id, e.repeat.map(|r| (r.count, r.interval)))),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            (0, 52, None),
            (0, 53, Some((2, 35))),
            (0, 50, None),
            (0, 49, None),
            (4, 51, None),
            (4, 51, None),
            (6, 48, None),
            (12, 57, Some((10, 4))),
            (30, 54, Some((6, 0))),
            (30, 56, None),
            (30, 58, Some((2, 8))),
            (30, 55, None),
            (34, 55, None),
        ]
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        (48..=58).collect()
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        BTreeSet::from([ModelRef::Common { index: 5 }])
    );
    assert_eq!(
        cooker.result.programs.len(),
        1,
        "no hidden secondary effects"
    );
    assert_eq!(cooker.material_indices.len(), 4);
    assert!(!cooker.pending_images.is_empty());
    let common = member(&usual, 6).unwrap();
    crate::model_preview::preflight(crate::model_preview::Layer {
        model: member(common, 4).unwrap(),
        outline: None,
        animation: None,
        attached_to: None,
        additive: false,
    })
    .unwrap();
    cooker.result.validate().unwrap();
}
