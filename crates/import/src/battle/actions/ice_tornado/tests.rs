use super::*;
use resonance_content::battle::effects::{EffectBank, KnockbackDirection, ProjectileMovement};

#[test]
#[ignore = "requires privately extracted US assets; recipe and complete binding recovery only"]
fn original_ice_tornado_retains_its_contact_casting_and_all_enemy_bindings() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let actions = technique_actions(&extracted, &rel, &usual, &[83]).unwrap();
    let TechniqueProgram::IceTornado {
        casters,
        recipe,
        resume,
    } = &actions[0].program
    else {
        panic!("lost Ice Tornado identity")
    };
    assert_eq!(actions[0].enemy_spell(), Some(EnemySpell::IceTornado));
    assert!(EnemySpell::IceTornado.stored());
    assert_eq!(
        casters
            .iter()
            .map(|c| (c.character, c.tp, c.time_adjustment))
            .collect::<Vec<_>>(),
        [(3, 30, 150)]
    );
    assert_eq!(
        (
            casters[0].voices.begin,
            casters[0].voices.release,
            casters[0].voices.begin_remaining
        ),
        (33112, 33084, 79)
    );
    assert!(matches!(
        casters[0].release,
        Some(AnimationCommand::Play { clip: 13, .. })
    ));
    assert!(matches!(resume, AnimationCommand::Play { clip: 12, .. }));
    assert_eq!(
        (recipe.lifetime, recipe.projectile_tick, recipe.effect_scale),
        (200, 45, 1.)
    );
    assert_eq!(
        recipe.origin.capture([0.; 3], [300., 80., 400.]),
        [299.4, 0., 399.2]
    );
    assert_eq!(recipe.presentation.color, [16, 16, 32, 255]);
    assert_eq!(
        (
            recipe.presentation.camera_distance,
            recipe.presentation.camera_elevation
        ),
        (3150., 10.)
    );
    assert_eq!(
        (
            recipe.rule.flags,
            recipe.rule.power,
            recipe.rule.hitstun,
            recipe.rule.contact_cooldown
        ),
        (0x22, 95, 45, 8)
    );
    let archive = MagicArchive::read(&extracted).unwrap();
    let records = magic_member(archive.package(21).unwrap(), 252)
        .unwrap()
        .unwrap();
    let flight =
        crate::battle::effects::projectile(&records[400..800], IceTornadoRecipe::EFFECT, 0.5)
            .unwrap();
    assert_eq!((flight.lifetime, flight.behavior.repeat_limit), (60, 6));
    assert_eq!(
        (
            flight.shape.radius,
            flight.shape.height,
            flight.shape.reaction
        ),
        (256., 300., 6)
    );
    assert!(matches!(flight.shape.kind, HitShapeKind::Box));
    assert!(matches!(
        flight.knockback,
        KnockbackDirection::AwayFromProjectile
    ));
    assert!(matches!(
        flight.movement,
        ProjectileMovement::Ballistic {
            velocity: [0., 0., 0.],
            acceleration: [0., 0., 0.],
            steering: None
        }
    ));
    assert_eq!(flight.spawn_offset, [0., 150., 0.]);
    assert_eq!(flight.birth_bank, Some(EffectBank::Magic(21)));
    assert!(
        flight.persist_after_hit
            && flight.shadow.is_none()
            && flight.spawn_effect.is_none()
            && flight.trail_effect.is_none()
            && flight.ground_effect.is_none()
    );
    let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
    assert_eq!(inventory.artes.artes.len(), 253);
    assert_eq!(inventory.enemies.packages.len(), 251);
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .map(|e| e.actions.len())
            .sum::<usize>(),
        1210
    );
    assert_eq!(
        inventory
            .artes
            .artes
            .iter()
            .filter(|a| a.native_id == 221)
            .map(|a| (a.id, a.owners.clone()))
            .collect::<Vec<_>>(),
        [(83, vec![3])]
    );
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .flat_map(|e| e
                .actions
                .iter()
                .filter(|a| a.native_technique == Some(221))
                .map(move |a| (e.monster, e.variant_count, a.id)))
            .collect::<Vec<_>>(),
        [
            (108, 1, 4),
            (116, 2, 3),
            (153, 1, 0),
            (199, 1, 7),
            (209, 1, 5),
            (222, 1, 7),
            (223, 1, 10),
            (248, 1, 4)
        ]
    );
    let dependencies = crate::battle::selection::Dependencies::actions(&BattleActions {
        party: vec![],
        enemies: vec![],
        projectiles: vec![],
        techniques: actions,
        chains: None,
    })
    .unwrap();
    assert!(dependencies.programs.contains(&IceTornadoRecipe::EFFECT));
    assert!(dependencies.projectiles.contains(&IceTornadoRecipe::EFFECT));
    let text = rel.sections[1].0;
    rel.bytes[text + 0x7bcac + 0x18] ^= 1;
    assert!(
        read_parameters(&rel).is_err(),
        "changed callback cannot reuse the recovered recipe"
    );
}
