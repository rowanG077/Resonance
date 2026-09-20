use super::*;

#[test]
fn screen_draw_mode_requires_the_alpha_image_with_or_without_the_dual_texture_flag() {
    let mut cooker = test_cooker();
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Fixed(0),
            color: 0,
            alpha: Some(0),
            stride: 0,
        },
        0,
    );
    let id = EffectId {
        bank: EffectBank::Enemy(183),
        id: 2,
    };
    let mut row = [0; ACTOR_BYTES];
    row[0] = 4;
    row[2] = 10;
    row[0x32] = 255;
    for flags in [0x20080040u32, 0x04000040] {
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        let actor = cooker.actor(&row, id, &[], false).unwrap();
        assert!(actor.screen_texture.is_some());
        assert_eq!(actor.material, Some(0));
        for unsupported in [0x20, 0x400000, 0x2000, 0x200000] {
            row[0x14..0x18].copy_from_slice(&(flags | unsupported).to_be_bytes());
            assert!(cooker.actor(&row, id, &[], false).is_err());
        }
    }
    // A screen request cannot silently reuse the single-image material path.
    row[0x14..0x18].copy_from_slice(&0x20080040u32.to_be_bytes());
    cooker.material_indices.clear();
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Fixed(0),
            color: 0,
            alpha: None,
            stride: 0,
        },
        0,
    );
    assert!(
        cooker
            .actor(&row, id, &[], false)
            .unwrap_err()
            .to_string()
            .contains("absent texture bank")
    );
}

#[test]
#[ignore = "requires privately extracted US assets; parses original screen effect and mask, no export"]
fn original_enemy183_screen_rings_close_the_fixed_alpha_atlas_and_both_repeat_groups() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let rel = Rel::read(&extracted.join("US_r_Top2Btl.rel")).unwrap();
    // Screen list -> draw mode 2; slot10 -> Fixed0; mode2 -> alpha image1.
    for (offset, instruction) in [
        (0x48960, 0x4bffe46d),
        (0x48980, 0x38800002),
        (0x48984, 0x48031f4d),
        (0x48108, 0x2c00000a),
        (0x48110, 0x63a00020),
        (0x48120, 0x2c00000a),
        (0x48128, 0x40820008),
        (0x4812c, 0x3bc00000),
        (0x481b0, 0x28000002),
        (0x481b4, 0x40820020),
        (0x481c4, 0x38600000),
        (0x481c8, 0x38a00001),
        (0x481d0, 0x48002d2d),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    let usual = fs::read(extracted.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let start = word(&usual, table + 183 * 4).unwrap() as usize;
    let end = word(&usual, table + 184 * 4).unwrap() as usize;
    let enemy = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&enemy)),
        "e335b6b57a06739d0902ed15bd59d6b6448e5de3781f9d73451053b22b252bce"
    );
    let start = word(&enemy, 0x1cc).unwrap() as usize;
    let end = word(&enemy, 0x1d0).unwrap() as usize;
    let bytes = &enemy[start..end];
    let id = EffectId {
        bank: EffectBank::Enemy(183),
        id: 2,
    };
    let row = actor_source(bytes, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "70f96a4ca032401ec988e4ece7817bc2526c4500c3ae248afaa493a620fb1d96"
    );
    assert_eq!(word(row, 0x14).unwrap(), 0x20080040);
    let fixed = compression::decode(member(member(&usual, 4).unwrap(), 1).unwrap()).unwrap();
    let images = tpl::parse_tpl(&fixed).unwrap();
    let alpha = tpl::decode_texture(&fixed, &images[1]).unwrap();
    let color = tpl::decode_texture(&fixed, &images[0]).unwrap();
    assert!(
        alpha
            .chunks_exact(4)
            .zip(color.chunks_exact(4))
            .any(|(a, c)| a[3] != c[3])
    );
    let mut cooker = test_cooker();
    cooker.textures.insert(TextureBank::Fixed(0), fixed);
    cooker.program(bytes, EffectId { id: 4, ..id }).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(id).unwrap();
    assert_eq!(actor.space, EffectSpace::OwnerBonePosition { bone: 26 });
    assert_eq!(actor.orientation, Orientation::Billboard);
    assert!(!actor.depth_test && !actor.depth_write && !actor.cull_back);
    assert!(actor.owner_layer.is_none());
    assert_eq!(actor.position, [0., 30., 0.]);
    assert_eq!(actor.dimensions, [0., 1., 8.]);
    assert_eq!(actor.dimension_velocity, [0., 1., 5.]);
    assert_eq!(actor.lifetime, Some(20));
    assert_eq!(actor.screen_texture.unwrap().offset, [5, 5]);
    assert_eq!(actor.uv, [193, 65, 32, 32]);
    assert_eq!(cooker.pending_images.len(), 1);
    for layer in [OwnerLayer::Before, OwnerLayer::After] {
        let mut invalid = cooker.result.clone();
        invalid
            .actors
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .owner_layer = Some(layer);
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("unsupported screen-textured effect")
        );
    }
    assert!(
        cooker.pending_images[0]
            .1
            .chunks_exact(4)
            .zip(alpha.chunks_exact(4))
            .all(|(p, a)| p[3] == a[3])
    );
    let program = cooker.result.program(EffectId { id: 4, ..id }).unwrap();
    assert_eq!(program.end_tick, 49);
    assert_eq!(program.emissions.len(), 2);
    for (emission, tick) in program.emissions.iter().zip([8, 39]) {
        assert_eq!(emission.tick, tick);
        assert!(matches!(
            emission.repeat,
            Some(Repeat {
                count: 3,
                interval: 4
            })
        ));
        assert!(
            matches!(emission.command, EffectCommand::Particle { actor, attachment: Attachment::Emitter, ref modifiers } if actor == id && modifiers.is_empty())
        );
    }
}
