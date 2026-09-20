use super::*;
use resonance_content::battle::effects::{EffectBank, ProjectileAim, ProjectileMovement};

#[test]
#[ignore = "requires privately extracted US data; parses all native222 bindings and resources without encoding"]
fn original_freeze_lancer_recovers_six_aimed_lances_and_every_enemy_binding() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let actions = technique_actions(&extracted, &rel, &usual, &[84]).unwrap();
    let action = &actions[0];
    let TechniqueProgram::FreezeLancer {
        casters, recipe, ..
    } = &action.program
    else {
        panic!("lost native222 identity")
    };
    assert_eq!(action.enemy_spell(), Some(EnemySpell::FreezeLancer));
    assert!(EnemySpell::FreezeLancer.stored());
    assert_eq!(
        casters
            .iter()
            .map(|c| (c.character, c.tp))
            .collect::<Vec<_>>(),
        [(3, 29)]
    );
    assert_eq!(
        (recipe.lifetime, recipe.first_tick, recipe.interval),
        (210, 20, 8)
    );
    assert_eq!(recipe.order, [0, 2, 4, 1, 3, 5]);
    assert_eq!(
        (
            recipe.forward_distance,
            recipe.minimum_height,
            recipe.radius
        ),
        (100., 170., 150.)
    );
    assert_eq!(
        (
            recipe.forward_threshold,
            recipe.direction_threshold,
            recipe.vertical_limit
        ),
        (0.5, 0.5, 0.35)
    );
    assert_eq!(
        (
            recipe.rule.power,
            recipe.rule.hitstun,
            recipe.rule.contact_cooldown
        ),
        (105, 35, 30)
    );
    assert_eq!(recipe.presentation.color, [16, 16, 32, 255]);
    assert_eq!(
        (
            recipe.presentation.camera_distance,
            recipe.presentation.camera_elevation
        ),
        (2700., 8.)
    );
    assert_eq!(recipe.sound, 95);
    assert_eq!(
        (
            casters[0].time_adjustment,
            casters[0].voices.begin,
            casters[0].voices.begin_remaining,
            casters[0].voices.release
        ),
        (150, 33112, 79, 33085)
    );
    let archive = MagicArchive::read(&extracted).unwrap();
    let package = archive.package(22).unwrap();
    let raw = magic_member(package, 252).unwrap().unwrap();
    assert_eq!(
        crate::digest(raw),
        "03b067ef34f507c678002d9b688948c9c4e4badb42a3bad69a467a1431df0d3f"
    );
    let projectile =
        crate::battle::effects::projectile(&raw[400..800], FreezeLancerRecipe::effect(1), 0.5)
            .unwrap();
    assert_eq!(projectile.lifetime, 90);
    assert_eq!(projectile.spawn_offset, [0.; 3]);
    assert!(matches!(
        projectile.movement,
        ProjectileMovement::Directed {
            direction: [0., 0., 30.],
            speed: 35.
        }
    ));
    assert!(matches!(projectile.behavior.aim, ProjectileAim::World));
    assert!(projectile.behavior.face_velocity && projectile.persist_after_hit);
    assert_eq!(
        (projectile.shape.radius, projectile.shape.height),
        (30., 30.)
    );
    assert_eq!(projectile.birth_bank, Some(EffectBank::Magic(22)));
    assert_eq!(projectile.spawn_effect, Some(FreezeLancerRecipe::effect(2)));
    assert_eq!(projectile.trail_effect, Some(FreezeLancerRecipe::effect(3)));
    assert_eq!(projectile.trail_interval, 4);
    assert!(projectile.shadow.is_some());
    let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
    assert_eq!(
        (
            inventory.artes.artes.len(),
            inventory.enemies.packages.len()
        ),
        (253, 251)
    );
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .map(|p| p.actions.len())
            .sum::<usize>(),
        1210
    );
    assert_eq!(
        inventory
            .artes
            .artes
            .iter()
            .filter(|a| a.native_id == 222)
            .map(|a| (a.id, a.owners.clone()))
            .collect::<Vec<_>>(),
        [(84, vec![3])]
    );
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .flat_map(|e| e
                .actions
                .iter()
                .filter(|a| a.native_technique == Some(222))
                .map(move |a| (e.monster, e.variant_count, a.id)))
            .collect::<Vec<_>>(),
        [(199, 1, 6), (209, 1, 6), (230, 1, 8), (233, 1, 5)]
    );
    let text = rel.sections[1].0;
    rel.bytes[text + 0x73344 + 0x74] ^= 1;
    assert!(
        read_parameters(&rel).is_err(),
        "changed timing or aim must not retain a stale recipe"
    );
}
