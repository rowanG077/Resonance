use super::*;

#[test]
#[ignore = "requires original extracted US guardian projectile rows; no asset cooking"]
fn original_guardian_projectiles_keep_unclamped_steering() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
    let rel = super::super::actions::Rel::read(&files.join("US_r_Top2Btl.rel")).unwrap();
    assert_eq!(float(rel.at((4, 0x27dc)).unwrap(), 0).unwrap(), 1.);
    let table = word(&usual, 0x2c).unwrap() as usize;
    for monster in [208, 209, 210] {
        let start = word(&usual, table + usize::from(monster) * 4).unwrap() as usize;
        let end = word(&usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let rows = super::super::enemy_inventory::offset_section(
            &bytes,
            word(&bytes, 0x1c8).unwrap() as usize,
        )
        .unwrap();
        assert_eq!(rows.len(), 6 * 400);
        for (id, row) in rows.chunks_exact(400).enumerate() {
            let id = EffectId {
                bank: EffectBank::Enemy(monster),
                id: id as u8,
            };
            let mut recipe = projectile(row, id, 0.001).unwrap();
            recipe.validate().unwrap();
            if id.id == 4 {
                assert_eq!(
                    crate::digest(row),
                    "e2d37aa08827080a21d17cca4d640f03b83ac6ce194e9bef7c39517a14656bff"
                );
                assert!(matches!(
                    recipe.movement,
                    ProjectileMovement::Homing {
                        direction: [0., 0., 0.],
                        speed: 4.,
                        blend: 90.,
                        start: 0,
                        end: None,
                        horizontal: true,
                    }
                ));
                let ProjectileMovement::Homing { ref mut blend, .. } = recipe.movement else {
                    unreachable!()
                };
                *blend = f32::INFINITY;
                assert!(recipe.validate().is_err());
            } else {
                assert!(matches!(
                    recipe.movement,
                    ProjectileMovement::Ballistic { steering: None, .. }
                ));
            }
        }
    }
}
