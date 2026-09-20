use super::*;

#[test]
fn sequential_timestamps_keep_command_order_and_repeat_birth() {
    let mut bytes = [0; 48];
    bytes[..5].copy_from_slice(b"ef1\0\x01");
    bytes[10..12].copy_from_slice(&20u16.to_be_bytes());
    bytes[16..20].copy_from_slice(&[0, 44, 0, 46]);
    bytes[20..44].copy_from_slice(&[
        0, 16, 252, 1, 0, 0, 0, 15, 255, 12, 0, 8, 0, 0, 252, 87, 0, 0, 0, 14, 254, 0, 0, 0,
    ]);
    let id = EffectId {
        bank: EffectBank::Common,
        id: 0,
    };
    let mut cooker = test_cooker();
    cooker.program(&bytes, id).unwrap();
    cooker.result.validate().unwrap();
    let program = cooker.result.program(id).unwrap();
    assert_eq!(program.end_tick, 16);
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [16, 16]
    );
    assert!(matches!(
        program.emissions[0].command,
        EffectCommand::Sound { sound: 1, .. }
    ));
    assert!(matches!(
        program.emissions[1].command,
        EffectCommand::Sound { sound: 87, .. }
    ));
    assert!(matches!(
        program.emissions[1].repeat,
        Some(Repeat {
            count: 12,
            interval: 8
        })
    ));
    bytes[20..22].copy_from_slice(&(-1i16).to_be_bytes());
    assert!(test_cooker().program(&bytes, id).is_err());
}

#[test]
#[ignore = "requires the privately extracted US disc; parses full effects without encoding textures"]
fn original_air_thrust_resolves_every_particle_model_and_repeat() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(9).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(package)),
        "3206933b0eb960ff5addf5952edcf4f073c770b51c6f5a6213319202e02075ed"
    );
    let bytes = magic_member(package, 4).unwrap().unwrap();
    let bank = EffectBank::Magic(9);
    let id = EffectId { bank, id: 1 };
    let mut cooker = test_cooker();
    // Only texture encoding is bypassed; actor, geometry, modifier and UV parsing remain strict.
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    for actor in [0, 5, 6] {
        let row = actor_source(bytes, EffectId { bank, id: actor }).unwrap();
        cooker.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Magic(9),
                color: row[3],
                alpha: Some(row[4]),
                stride: row[6],
            },
            0,
        );
    }
    cooker.program(bytes, id).unwrap();
    cooker.result.validate().unwrap();
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|a| a.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 1, 2, 5, 6])
    );
    assert_eq!(
        cooker.result.models().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ModelRef::Magic {
                package: 9,
                index: 0
            },
            ModelRef::Magic {
                package: 9,
                index: 1
            }
        ])
    );
    let program = cooker.result.program(id).unwrap();
    assert_eq!(program.end_tick, 110);
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        [8, 10, 16, 16, 16, 21, 21]
    );
    assert!(matches!(
        program.emissions[3].command,
        EffectCommand::Sound {
            sound: 87,
            priority: 0
        }
    ));
    assert!(matches!(
        program.emissions[3].repeat,
        Some(Repeat {
            count: 12,
            interval: 8
        })
    ));
    for actor in [1, 2] {
        let actor = cooker.result.actor(EffectId { bank, id: actor }).unwrap();
        assert!(matches!(
            actor.geometry,
            Geometry::Model {
                animation: None,
                ..
            }
        ));
        assert!(actor.uv_animation.as_ref().unwrap().model_scroll.is_some());
    }
    let mut row = actor_source(bytes, EffectId { bank, id: 0 })
        .unwrap()
        .to_vec();
    row[0x14] |= 0x20;
    assert!(
        cooker
            .actor(&row, EffectId { bank, id: 0 }, bytes, false)
            .is_err(),
        "unknown flags must remain rejected"
    );
}
