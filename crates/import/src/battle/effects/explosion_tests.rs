use super::*;

fn row(id: u8) -> [u8; 400] {
    let encoded = match id {
        1 => concat!(
            "000000000000000000000009003c000000020007010402000000000000000000",
            "0000000000000000c20c00000000000000000000000000000000000000000000",
            "3f40000042c8000042c800000000000000000000000000000000000000020003",
            "0000000045034000000000000000000000000000000000000000000000000000",
            "000000000000000a401010808020208000010400000000000000000004000000"
        ),
        2 => concat!(
            "0000000000000000000000090020000000020012010402000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "3f40000042000000420000000000000041200000412000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000101040802020808000000000000000000000000004000000"
        ),
        _ => panic!("unknown Explosion projectile"),
    };
    let mut row = [0; 400];
    for (i, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    row
}

#[test]
fn explosion_projectiles_keep_the_falling_lance_and_expanding_impact_separate() {
    use resonance_content::battle::effects::{KnockbackDirection, ProjectileMovement};
    let id = |id| EffectId {
        bank: EffectBank::Magic(6),
        id,
    };
    let lance = projectile(&row(1), id(1), 0.001).unwrap();
    assert_eq!(
        (lance.lifetime, lance.spawn_offset, lance.active),
        (60, [0., 2100., 0.], Some([0, 10]))
    );
    assert!(matches!(
        lance.movement,
        ProjectileMovement::Ballistic {
            velocity: [0., -35., 0.],
            acceleration: [0., 0., 0.],
            steering: None
        }
    ));
    assert!(matches!(
        lance.shape.kind,
        resonance_content::battle::actions::HitShapeKind::Sphere
    ));
    assert_eq!(
        (
            lance.shape.radius,
            lance.shape.height,
            lance.shape.damage_kind,
            lance.shape.hit_class,
            lance.shape.reaction
        ),
        (100., 100., 2, 0, 7)
    );
    assert!(matches!(
        lance.knockback,
        KnockbackDirection::AwayFromProjectile
    ));
    assert_eq!(
        (
            lance.birth_bank,
            lance.spawn_effect,
            lance.trail_effect,
            lance.trail_interval
        ),
        (Some(id(1).bank), Some(id(2)), Some(id(3)), 1)
    );
    assert!(lance.persist_after_hit && !lance.clashable && !lance.behavior.clamp_ground);
    let impact = projectile(&row(2), id(2), 0.001).unwrap();
    assert_eq!(
        (
            impact.lifetime,
            impact.shape.radius,
            impact.shape.height,
            impact.shape.reaction
        ),
        (32, 32., 32., 18)
    );
    assert_eq!(impact.behavior.hit_growth, [10., 10.]);
    assert!(
        impact.active.is_none() && impact.spawn_effect.is_none() && impact.trail_effect.is_none()
    );
    assert!(matches!(
        impact.movement,
        ProjectileMovement::Ballistic {
            velocity: [0., 0., 0.],
            acceleration: [0., 0., 0.],
            steering: None
        }
    ));
    let mut ground = row(1);
    ground[0xe..0x10].copy_from_slice(&[0, 3]);
    assert_eq!(
        projectile(&ground, id(1), 0.001).unwrap().ground_effect,
        Some(EffectId {
            bank: EffectBank::Common,
            id: 3
        })
    );
}

#[test]
#[ignore = "requires the original extracted disc; reads source records only"]
fn original_explosion_projectiles_match_the_complete_embedded_rows() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = magic_member(archive.package(6).unwrap(), 252)
        .unwrap()
        .unwrap();
    for id in [1, 2] {
        let start = usize::from(id) * 400;
        assert_eq!(row(id).as_slice(), &source[start..start + 400]);
    }
}
