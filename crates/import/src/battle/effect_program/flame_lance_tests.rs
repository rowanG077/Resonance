use super::*;

#[test]
#[ignore = "requires the privately extracted US disc; parses effects without encoding textures"]
fn original_flame_lance_resolves_ground_layers_models_trails_and_camera_shakes() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(7).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(package)),
        "ff3bb0492671e8df6a1a6f6ea9f063ec34a24624c3f6b9ef0629b563613fbe57"
    );
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(7);
    let mut cooker = test_cooker();
    // Skip only image encoding; all authored geometry, controller, modifier and UV records parse.
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    for actor in 2..18 {
        let row = actor_source(bytes, EffectId { bank, id: actor }).unwrap();
        if !matches!(row[0], 3 | 11) {
            cooker.material_indices.insert(
                MaterialKey {
                    texture: TextureBank::Magic(7),
                    color: row[3],
                    alpha: (word(row, 0x14).unwrap() & 0x4000000 != 0).then_some(row[4]),
                    stride: row[6],
                },
                0,
            );
        }
    }
    for id in 1..=3 {
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
        (2..18).collect()
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ModelRef::Magic {
                package: 7,
                index: 0
            },
            ModelRef::Magic {
                package: 7,
                index: 1
            },
        ])
    );
    let program = cooker.result.program(EffectId { bank, id: 1 }).unwrap();
    assert_eq!(program.end_tick, 105);
    let shakes = program
        .emissions
        .iter()
        .filter_map(|e| match e.command {
            EffectCommand::Controller {
                controller:
                    EffectController::Shake {
                        duration,
                        amplitude,
                    },
                ..
            } => Some((e.tick, duration, amplitude)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(shakes, [(40, 20, 24), (92, 40, 32)]);
    let sounds = program
        .emissions
        .iter()
        .filter_map(|e| match e.command {
            EffectCommand::Sound { sound, .. } => Some((e.tick, sound)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sounds, [(40, 93), (90, 134)]);
    for actor in [14, 15] {
        let actor = cooker.result.actor(EffectId { bank, id: actor }).unwrap();
        assert!(actor.follow_emitter && actor.cull_back && actor.lifetime.is_none());
        assert!(matches!(
            actor.geometry,
            Geometry::Model {
                animation: None,
                presentation: ModelPresentation {
                    orientation: ModelOrientation::FollowMotion,
                    ..
                },
                ..
            }
        ));
    }
    assert!(
        cooker
            .result
            .actor(EffectId { bank, id: 15 })
            .unwrap()
            .uv_animation
            .as_ref()
            .unwrap()
            .model_scroll
            .is_some()
    );
    let trail = cooker.result.program(EffectId { bank, id: 3 }).unwrap();
    assert_eq!(trail.end_tick, 3);
    assert!(matches!(
        trail.emissions[0].repeat,
        Some(Repeat {
            count: 3,
            interval: 1
        })
    ));
    let mut row = actor_source(bytes, EffectId { bank, id: 14 })
        .unwrap()
        .to_vec();
    row[0] = 5;
    assert!(
        cooker
            .actor(&row, EffectId { bank, id: 14 }, bytes, false)
            .is_err(),
        "motion-facing sprites remain unsupported"
    );
}
