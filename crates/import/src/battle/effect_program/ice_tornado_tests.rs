use super::*;

#[test]
#[ignore = "requires original US Magic21 effects and palettes; no encoding"]
fn original_ice_tornado_closes_every_repeat_spiral_trail_palette_and_sound() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = archive.package(21).unwrap();
    let bytes = magic_member(source, 4).unwrap().unwrap();
    let id = resonance_content::battle::actions::ice_tornado::IceTornadoRecipe::EFFECT;
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(21),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    cooker.program(bytes, id).unwrap();
    let root = cooker.result.program(id).unwrap();
    assert_eq!(root.end_tick, 180);
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
            (10, 0, Some((12, 8))),
            (10, 0, Some((12, 8))),
            (20, 5, Some((3, 30))),
            (40, 1, Some((8, 8))),
            (40, 1, Some((8, 8))),
            (40, 2, Some((14, 5))),
            (40, 3, Some((15, 4))),
            (50, 4, Some((8, 10)))
        ]
    );
    assert_eq!(
        root.emissions
            .iter()
            .filter_map(|e| match e.command {
                EffectCommand::Sound { sound, priority } => Some((
                    e.tick,
                    sound,
                    priority,
                    e.repeat.map(|r| (r.count, r.interval))
                )),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(10, 88, 0, Some((6, 20)))]
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        (0..6).collect()
    );
    assert_eq!(cooker.result.programs.len(), 1);
    assert_eq!(cooker.result.models().count(), 0);
    assert!((0..10).all(|i| magic_member(source, 12 + i * 4).unwrap().is_none()));
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| matches!(a.geometry, Geometry::Spiral { .. }))
            .count(),
        2
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| matches!(a.geometry, Geometry::BillboardTrail { .. }))
            .count(),
        1
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| matches!(a.geometry, Geometry::BillboardRing { .. }))
            .count(),
        3
    );
    let uv = cooker
        .result
        .actors
        .iter()
        .find(|a| a.id.id == 3)
        .unwrap()
        .uv_animation
        .as_ref()
        .unwrap();
    assert_eq!((uv.frames.len(), uv.loop_to), (14, Some(0)));
    assert!(uv.frames.iter().all(|frame| frame.duration == 2));
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .filter(|a| a.uv_animation.is_some())
            .count(),
        1
    );
    assert!(!cooker.pending_images.is_empty());
    cooker.result.validate().unwrap();
    let mut changed = bytes.to_vec();
    changed[2144] = 254;
    let mut invalid = test_cooker();
    invalid.textures.insert(
        TextureBank::Magic(21),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    assert!(
        invalid.program(&changed, id).is_err(),
        "unknown spiral modifier must fail cooking"
    );
}
