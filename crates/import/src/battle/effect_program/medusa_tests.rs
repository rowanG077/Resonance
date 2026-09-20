use super::*;

#[test]
fn ring_bone_selector_is_dormant_until_owner_bone_space_is_enabled() {
    let mut row = [0; ACTOR_BYTES];
    row[0] = 4;
    row[2] = 255;
    row[0x32] = 255;
    let id = EffectId {
        bank: EffectBank::Enemy(52),
        id: 8,
    };
    let mut cooker = test_cooker();
    let expected = serde_json::to_value(cooker.actor(&row, id, &[], false).unwrap()).unwrap();
    for bone in [1, 5, 255] {
        row[0x8c] = bone;
        assert_eq!(
            serde_json::to_value(cooker.actor(&row, id, &[], false).unwrap()).unwrap(),
            expected
        );
    }
    row[0x8c] = 5;
    row[0x14..0x18].copy_from_slice(&0x40000000u32.to_be_bytes());
    assert_eq!(
        cooker.actor(&row, id, &[], false).unwrap().space,
        EffectSpace::OwnerBone { bone: 5 }
    );
    row[0x14..0x18].copy_from_slice(&0x20000000u32.to_be_bytes());
    assert_eq!(
        cooker.actor(&row, id, &[], false).unwrap().space,
        EffectSpace::OwnerBonePosition { bone: 5 }
    );
    for flags in [0x8000u32, 0x60000000] {
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
    row[0x14..0x18].fill(0);
    for offset in [0x7c, 0x80, 0x84, 0x8a, 0x8b, 0x8d] {
        row[offset] = 1;
        assert!(
            cooker
                .actor(&row, id, &[], false)
                .unwrap_err()
                .to_string()
                .contains("unsupported effect actor motion state")
        );
        row[offset] = 0;
    }
    row[0] = 5;
    assert!(cooker.actor(&row, id, &[], false).is_err());
}

#[test]
#[ignore = "requires original extracted GameCube assets; validates the complete program and actual palette without encoding"]
fn original_medusa_stone_gaze_keeps_world_space_and_all_three_ring_births() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    assert_eq!(rel.pointer(5, 0x5bc8 + 4 * 4).unwrap(), (1, 0x7a498));
    // Complete allocator, initializer, update and native4 draw; bone reads are flag-gated.
    for (offset, size, hash) in [
        (
            0x413b8,
            0x14c,
            "c0c1074ab7ca98a6fa1db517bfdcf13750fb550ee6433cc29ad511b5c2843bb2",
        ),
        (
            0x40e40,
            0x2b8,
            "fca41e56e3561d05a371f5f2ee1529803d7f8b64668f75e774a4c88f78fe6440",
        ),
        (
            0x403f4,
            0xa4c,
            "9369dfac3f467163c41793b3a52da6e3138d2ae564700f15ff11f98ada3cd9e1",
        ),
        (
            0x7a498,
            0x434,
            "f79a836a6981cb8f7f13f29915efffdb76465e8f86f2ea79e0febedcd0052769",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..size])
            ),
            hash
        );
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let directory = member(&usual, 10).unwrap();
    let (start, end) = (
        word(directory, 52 * 4).unwrap() as usize,
        word(directory, 53 * 4).unwrap() as usize,
    );
    let enemy = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&enemy)),
        "23babf6b9a170dad7f9012f51b444bad9dae327cd3568f27232af348fe5b7518"
    );
    let (start, end) = (
        word(&enemy, 0x1cc).unwrap() as usize,
        word(&enemy, 0x1d0).unwrap() as usize,
    );
    assert_eq!((start, end), (424160, 429056));
    let bank = &enemy[start..end];
    assert_eq!(
        format!("{:x}", Sha256::digest(bank)),
        "5242536d03a5e0123d9a25e1f12ae1419a405d1d150aa814221516c280307135"
    );
    let id = EffectId {
        bank: EffectBank::Enemy(52),
        id: 8,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "41c825cea17318c98c7e841522dfc604e3946c76f94cb11dcbc05db224fd4a84"
    );
    assert_eq!(word(row, 0x14).unwrap(), 0x04000000);
    assert_eq!(&row[0x8a..0x8e], &[0, 0, 5, 0]);
    assert_eq!(
        &bank[4592..4616],
        &[
            0, 0, 11, 0, 0, 0, 0, 40, 255, 3, 0, 3, 0, 0, 8, 0, 0, 0, 0, 50, 254, 0, 0, 0
        ]
    );
    let mut cooker = test_cooker();
    cooker
        .textures
        .insert(TextureBank::Enemy(52), enemy[end..].to_vec());
    let program_id = EffectId { id: 7, ..id };
    cooker.program(bank, program_id).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(id).unwrap();
    assert_eq!(actor.space, EffectSpace::World);
    assert_eq!(actor.position, [0., 150., 80.]);
    assert_eq!(actor.velocity, [0., 0., 1.]);
    assert_eq!(actor.angles, [20., 0., 0.]);
    assert_eq!(actor.dimensions, [15., 10., 10.]);
    assert_eq!(actor.dimension_velocity, [-1., 2., 4.]);
    assert_eq!(actor.lifetime, Some(15));
    assert!(matches!(
        actor.geometry,
        Geometry::Ring { segments: 16, .. }
    ));
    assert!(actor.material.is_some() && actor.uv_animation.is_none());
    assert_eq!(cooker.result.actors.len(), 1);
    assert!(!cooker.pending_images.is_empty());
    let program = cooker.result.program(program_id).unwrap();
    assert_eq!(program.end_tick, 50);
    assert_eq!(program.emissions.len(), 2);
    assert_eq!(program.emissions[0].tick, 0);
    assert!(
        matches!(&program.emissions[0].command,EffectCommand::Controller {
        actor:EffectId {bank:EffectBank::Enemy(52),id:11},controller:EffectController::Caption {text}
    } if text=="Stone Gaze")
    );
    let emission = &program.emissions[1];
    assert_eq!(emission.tick, 40);
    assert_eq!(
        (
            emission.repeat.unwrap().count,
            emission.repeat.unwrap().interval
        ),
        (3, 3)
    );
    assert!(
        matches!(&emission.command,EffectCommand::Particle {actor,attachment:Attachment::Emitter,modifiers} if *actor==id && modifiers.is_empty())
    );
}
