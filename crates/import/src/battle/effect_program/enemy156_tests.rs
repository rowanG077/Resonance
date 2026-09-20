//! Enemy156's model keeps its live bone and spin; recipe +0x80 has no model consumer.
use super::*;

#[test]
#[ignore = "requires privately extracted US assets; parses the full original program/model without encoding"]
fn original_enemy156_program6_keeps_bone26_model_and_ignores_only_its_dormant_motion_word() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    assert_eq!(rel.pointer(5, 0x5bc8 + 3 * 4).unwrap(), (1, 0x7a8cc));
    for kind in [16, 17] {
        assert_eq!(rel.pointer(5, 0x5bc8 + kind * 4).unwrap(), (1, 0x7a8cc));
    }
    assert!(rel.pointer(5, 0x5bc8 + 2 * 4).is_err());
    assert_eq!(&rel.at((5, 0x5bc8 + 2 * 4)).unwrap()[..4], &[0; 4]);
    // Complete model draw, common update/initializer, allocator and emission dispatcher.
    // Raw recipe +0x80 is object +0xa8; no model path reads it. In particular,
    // draw3FC24 copies object +0x80 (recipe +0x58) into the model's Euler angles.
    for (offset, size, expected) in [
        (
            0x3fc24,
            0x51c,
            "60f37608f2562c4f3248cf9ef2978766a5008d36550b30a0148a0dc58205ca41",
        ),
        (
            0x403f4,
            0xa4c,
            "9369dfac3f467163c41793b3a52da6e3138d2ae564700f15ff11f98ada3cd9e1",
        ),
        (
            0x40e40,
            0x2b8,
            "fca41e56e3561d05a371f5f2ee1529803d7f8b64668f75e774a4c88f78fe6440",
        ),
        (
            0x413b8,
            0x14c,
            "c0c1074ab7ca98a6fa1db517bfdcf13750fb550ee6433cc29ad511b5c2843bb2",
        ),
        (
            0x418b4,
            0x810,
            "fada0d5d1b343b1b6216356950ccca82f86a48c25fd859f7a52ac966bea3d661",
        ),
        (
            0x7a8cc,
            4,
            "f332ea5b5437103cbb6f1508679da89eec9288ad775c96c439a17fccabe3de8e",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..size])
            ),
            expected
        );
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let directory = member(&usual, 10).unwrap();
    let start = word(directory, 156 * 4).unwrap() as usize;
    let end = word(directory, 157 * 4).unwrap() as usize;
    assert_eq!((start, end), (39115648, 39372384));
    assert_eq!(
        format!("{:x}", Sha256::digest(&archive[start..end])),
        "c736d0bc8f3cd7e7c0ce085a3595659109fa3bb33571f1d8f85702f180535e14"
    );
    let enemy = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&enemy)),
        "e140caa78713463f59f25f8c10db12b37654ca0a44b9d8cd452e6047c88147e6"
    );
    let start = word(&enemy, 0x1cc).unwrap() as usize;
    let end = word(&enemy, 0x1d0).unwrap() as usize;
    assert_eq!((start, end), (391648, 393664));
    let bank = &enemy[start..end];
    assert_eq!(
        format!("{:x}", Sha256::digest(bank)),
        "ada5c1cbc1153b4f385fa11d3edac7784597ef6ba5415b35ffa0233f44b8c4c0"
    );
    let id = EffectId {
        bank: EffectBank::Enemy(156),
        id: 0,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "47dfdbf6d8aef0779c345e8ff44cd35259bd36f924ecdf3507cd88a46e14a263"
    );
    assert_eq!(
        (row[0], word(row, 0x14).unwrap(), float(row, 0x80).unwrap()),
        (3, 0x20000000, 10.)
    );
    assert_eq!(
        &bank[1960..1972],
        &[0, 20, 0, 26, 0, 0, 0, 40, 254, 0, 0, 0]
    );
    let mut cooker = test_cooker();
    cooker.program(bank, EffectId { id: 6, ..id }).unwrap();
    cooker.result.validate().unwrap();
    let program = cooker.result.program(EffectId { id: 6, ..id }).unwrap();
    assert_eq!((program.end_tick, program.emissions.len()), (40, 1));
    assert_eq!(program.emissions[0].tick, 20);
    assert!(program.emissions[0].repeat.is_none());
    assert!(
        matches!(&program.emissions[0].command, EffectCommand::Particle { actor, attachment: Attachment::Bone(26), modifiers } if *actor == id && modifiers.is_empty())
    );
    let actor = cooker.result.actor(id).unwrap();
    assert_eq!(actor.space, EffectSpace::OwnerBonePosition { bone: 26 });
    assert_eq!(actor.angles, [45., 0., 0.]);
    assert_eq!(actor.angular_velocity, [0., 20., 0.]);
    assert_eq!(actor.lifetime, Some(60));
    assert_eq!(actor.position, [0.; 3]);
    assert_eq!(actor.velocity, [0.; 3]);
    assert_eq!(actor.dimensions, [1.; 3]);
    assert!(matches!(
        actor.geometry,
        Geometry::Model {
            model: ModelRef::Enemy {
                monster: 156,
                index: 0
            },
            animation: None,
            loop_animation: false,
            presentation: ModelPresentation {
                orientation: ModelOrientation::Authored,
                texture_rows: 26,
                texture_frame: 0,
                ..
            }
        }
    ));
    let expected = serde_json::to_value(actor).unwrap();
    for bits in [0, 10f32.to_bits(), (-1f32).to_bits(), f32::NAN.to_bits()] {
        let mut changed = row.to_vec();
        changed[0x80..0x84].copy_from_slice(&bits.to_be_bytes());
        assert_eq!(
            serde_json::to_value(cooker.actor(&changed, id, bank, false).unwrap()).unwrap(),
            expected
        );
    }
    for offset in [0x7c, 0x84, 0x8a] {
        let mut changed = row.to_vec();
        changed[offset] = 1;
        assert!(
            cooker
                .actor(&changed, id, bank, false)
                .unwrap_err()
                .to_string()
                .contains("unsupported effect actor motion state")
        );
    }
    for kind in [4, 5] {
        let mut changed = row.to_vec();
        changed[0] = kind;
        assert!(
            cooker.actor(&changed, id, bank, false).is_err(),
            "other geometries must not inherit the model-only dormant word"
        );
    }
    // The emitted model is the original model0 resource, not a replacement body mesh.
    let model = word(&enemy, 0x180).unwrap() as usize;
    assert_eq!(model, 381248);
    assert_eq!(
        (word(&enemy, 0x198).unwrap(), word(&enemy, 0x1b0).unwrap()),
        (0, 0)
    );
    crate::model_preview::preflight(crate::model_preview::Layer {
        model: &enemy[model..start],
        outline: None,
        animation: None,
        attached_to: None,
        additive: false,
    })
    .unwrap();
}
