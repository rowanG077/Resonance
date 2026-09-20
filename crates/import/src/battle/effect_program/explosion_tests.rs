use super::*;

#[test]
#[ignore = "requires the original extracted disc; parses effects without encoding textures"]
fn original_explosion_closes_startup_birth_trail_models_scroll_and_shake() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(6).unwrap();
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(6);
    let mut cooker = test_cooker();
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    for id in [0, 1, 2, 3, 4, 9, 10, 11, 12, 13, 14, 16] {
        let row = actor_source(bytes, EffectId { bank, id }).unwrap();
        cooker.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Magic(6),
                color: row[3],
                alpha: (word(row, 0x14).unwrap() & 0x4000000 != 0).then_some(row[4]),
                stride: row[6],
            },
            0,
        );
    }
    for id in [1, 2, 3] {
        cooker.program(bytes, EffectId { bank, id }).unwrap();
    }
    cooker.result.validate().unwrap();
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 16])
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 2, 3].map(|index| ModelRef::Magic { package: 6, index }))
    );
    let startup = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
    assert_eq!(startup.end_tick, 150);
    assert_eq!(
        startup.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [
            0, 0, 0, 0, 0, 62, 62, 62, 62, 62, 62, 62, 63, 63, 64, 67, 67, 67, 97, 97, 107, 107,
            117, 117
        ]
    );
    assert!(matches!(
        startup.emissions[5].command,
        EffectCommand::Sound {
            sound: 133,
            priority: 0
        }
    ));
    assert!(matches!(
        startup.emissions[6].command,
        EffectCommand::Controller {
            controller: EffectController::Shake {
                duration: 50,
                amplitude: 36
            },
            ..
        }
    ));
    for emission in &startup.emissions[..5] {
        assert!(matches!(
            emission.repeat,
            Some(Repeat {
                count: 15,
                interval: 4
            })
        ));
    }
    assert!(matches!(
        startup.emissions[14].repeat,
        Some(Repeat {
            count: 8,
            interval: 5
        })
    ));
    // Sequential dispatch cannot rewind to this record's authored timestamp65.
    assert!(matches!(
        startup.emissions[17].repeat,
        Some(Repeat {
            count: 60,
            interval: 1
        })
    ));
    for (id, origin, step) in [(9, [64, 128], [0, 4]), (10, [128, 128], [0, 8])] {
        let actor = cooker.result.actor(EffectId { bank, id }).unwrap();
        assert!(matches!(actor.geometry, Geometry::Hemisphere { .. }));
        let track = actor.uv_animation.as_ref().unwrap();
        assert_eq!(track.frames[0].duration, 1);
        assert!(
            matches!(track.frames[0].update, UvUpdate::Scroll { origin: o, step: s }
            if o == origin && s == step)
        );
    }
    for id in [5, 7, 8] {
        let actor = cooker.result.actor(EffectId { bank, id }).unwrap();
        assert!(actor.uv_animation.as_ref().unwrap().model_scroll.is_some());
    }
    let birth = cooker.result.actor(EffectId { bank, id: 0 }).unwrap();
    assert!(birth.lifetime.is_none() && birth.follow_emitter);
    let trail = cooker.result.program(EffectId { bank, id: 3 }).unwrap();
    assert_eq!((trail.end_tick, trail.emissions.len()), (3, 1));
    assert!(matches!(
        trail.emissions[0].repeat,
        Some(Repeat {
            count: 3,
            interval: 1
        })
    ));
}
