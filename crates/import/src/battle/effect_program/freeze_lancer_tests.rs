use super::*;

#[test]
#[ignore = "requires original US Magic22 effects, models and palettes; no encoding"]
fn original_freeze_lancer_closes_casting_ring_lance_models_trails_and_uv_tracks() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Update, world Euler matrix, and ring draw dispatch: all three consume the orientation flag.
    for (offset, size, expected) in [
        (
            0x403f4,
            0xa4c,
            "9369dfac3f467163c41793b3a52da6e3138d2ae564700f15ff11f98ada3cd9e1",
        ),
        (
            0x43340,
            0x214,
            "3d053709a683a510ce906ea4e6c28ae0d6c9be801f7c05ff7e3d304c692a46da",
        ),
        (
            0x7a498,
            0x434,
            "f79a836a6981cb8f7f13f29915efffdb76465e8f86f2ea79e0febedcd0052769",
        ),
    ] {
        assert_eq!(
            crate::digest(&rel.at((1, offset)).unwrap()[..size]),
            expected
        );
    }
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = archive.package(22).unwrap();
    let bytes = magic_member(source, 4).unwrap().unwrap();
    assert_eq!(
        crate::digest(bytes),
        "f679fb5f25afdcc7c4d9d3281c02582a50d01f1e5ee371cdd6b3d6071b43b5c4"
    );
    let bank = EffectBank::Magic(22);
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(22),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    for id in 1..=3 {
        cooker.program(bytes, EffectId { bank, id }).unwrap();
    }
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        (1..=10).collect()
    );
    assert_eq!(
        cooker
            .result
            .programs
            .iter()
            .map(|p| (p.id.id, p.end_tick))
            .collect::<Vec<_>>(),
        [(1, 68), (2, 0), (3, 0)]
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        (0..3)
            .map(|index| ModelRef::Magic { package: 22, index })
            .collect()
    );
    let ring = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
    assert_eq!(
        ring.emissions
            .iter()
            .filter_map(|e| match e.command {
                EffectCommand::Particle { actor, .. } if actor.id == 9 => Some(e.tick),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [20, 28, 36, 44, 52, 60]
    );
    for id in [1, 2] {
        let lance = cooker.result.actor(EffectId { bank, id }).unwrap();
        assert!(lance.follow_emitter && lance.lifetime.is_none());
        assert!(matches!(
            lance.geometry,
            Geometry::Model {
                presentation: ModelPresentation {
                    orientation: ModelOrientation::FollowMotion,
                    ..
                },
                ..
            }
        ));
        assert!(lance.uv_animation.as_ref().unwrap().model_scroll.is_some());
    }
    let trail_ring = cooker.result.actor(EffectId { bank, id: 4 }).unwrap();
    assert_eq!(trail_ring.orientation, Orientation::FixedWorld);
    assert!(!trail_ring.follow_emitter && trail_ring.lifetime == Some(20));
    assert!(matches!(trail_ring.geometry, Geometry::Ring { .. }));
    let row = actor_source(bytes, trail_ring.id).unwrap();
    assert_eq!(word(row, 0x14).unwrap(), 0x04000080);
    assert_eq!(
        crate::digest(row),
        "de8d832cf9a1a8c4394fe9a64ce462e0b6da93900381514ffac4c884297f1aa0"
    );
    for incompatible in [0x40, 0x400, 0x2000000, 0x20000000, 0x40000000] {
        let mut row = row.to_vec();
        row[0x14..0x18].copy_from_slice(&(0x04000080u32 | incompatible).to_be_bytes());
        assert!(
            cooker
                .actor(&row, EffectId { bank, id: 4 }, bytes, false)
                .is_err()
        );
    }
    assert!(!cooker.material_indices.is_empty() && !cooker.pending_images.is_empty());
    for index in 0..3 {
        crate::model_preview::preflight(crate::model_preview::Layer {
            model: magic_member(source, 12 + index * 4).unwrap().unwrap(),
            outline: None,
            animation: None,
            attached_to: None,
            additive: false,
        })
        .unwrap();
    }
    cooker.result.validate().unwrap();
}
