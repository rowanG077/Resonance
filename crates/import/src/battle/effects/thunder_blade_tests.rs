use super::*;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires the original extracted disc"]
fn original_thunder_blade_grounds_the_sword_without_stopping_its_shockwave() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&root).unwrap();
    let rows = magic_member(archive.package(19).unwrap(), 252)
        .unwrap()
        .unwrap();
    for (id, hash, lifetime, radius, height, spawn_height) in [
        (
            0u8,
            "a26cbe3f6b4cd03badd02366e45e7ecdc765c39cea45e91475c4d4c674c263d4",
            30,
            70.,
            200.,
            1100.,
        ),
        (
            2,
            "154769fefb8d7583516294f9dcccef81005b1597e11fadcb6db8368eb0e2add1",
            16,
            360.,
            50.,
            10.,
        ),
    ] {
        let row = &rows[usize::from(id) * 400..(usize::from(id) + 1) * 400];
        assert_eq!(format!("{:x}", Sha256::digest(row)), hash);
        assert_eq!(word(row, 8).unwrap(), if id == 0 { 0x80409 } else { 0x409 });
        let recipe = projectile(
            row,
            EffectId {
                bank: EffectBank::Magic(19),
                id,
            },
            0.001,
        )
        .unwrap();
        recipe.validate().unwrap();
        assert_eq!(recipe.behavior.stop_on_ground, id == 0);
        assert_eq!(recipe.lifetime, lifetime);
        assert_eq!((recipe.shape.radius, recipe.shape.height), (radius, height));
        assert!(matches!(recipe.shape.kind, HitShapeKind::Cylinder));
        assert_eq!(recipe.spawn_offset, [0., spawn_height, 0.]);
        let ProjectileMovement::Ballistic {
            velocity,
            acceleration,
            steering: None,
        } = recipe.movement
        else {
            panic!("Thunder Blade motion");
        };
        assert_eq!(velocity, [0., if id == 0 { -75. } else { 0. }, 0.]);
        assert_eq!(acceleration, [0.; 3]);
        assert!(recipe.persist_after_hit && recipe.active.is_none());
        assert!(
            recipe.spawn_effect.is_none()
                && recipe.trail_effect.is_none()
                && recipe.ground_effect.is_none()
        );
    }
    let rel = super::super::actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    // Impact precedes the strict floor test. Stop replaces worldY and velocity
    // only; each attached child's motion-derived orientation is then disabled.
    for (at, instruction) in [
        (0x144b8, 0x4802bd59),
        (0x144c0, 0x981f000f),
        (0x144d0, 0xfc010040),
        (0x144f4, 0x54000319),
        (0x1450c, 0xd01f001c),
        (0x14510, 0xd01f0024),
        (0x14514, 0xd01f0028),
        (0x14518, 0xd01f002c),
        (0x1452c, 0x8003003c),
        (0x14530, 0x5400066e),
        (0x14534, 0x9003003c),
    ] {
        assert_eq!(
            word(rel.at((1, at)).unwrap(), 0).unwrap(),
            instruction,
            "{at:#x}"
        );
    }
    assert_eq!(float(rel.at((4, 0xca8)).unwrap(), 0).unwrap(), 0.1);
    assert_eq!(float(rel.at((4, 0xcc8)).unwrap(), 0).unwrap(), 0.);
}
