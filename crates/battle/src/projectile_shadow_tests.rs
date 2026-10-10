use super::*;
use crate::{Battle, BattleInput, PreparedBattle, Side};
use std::sync::Arc;

fn battle() -> Battle {
    let mut owner = crate::tests::actor(Side::Party);
    owner.control = crate::Control::Manual;
    owner.equipment.normal_combo_limit = 1;
    let mut target = crate::tests::actor(Side::Enemy);
    target.position[0] = 1000.;
    let mut actions = crate::ActionDefinitions::default();
    let normals =
        crate::tests::normal_controls(&mut actions, crate::tests::attack(200), [0., 100.]);
    let mut prepared = PreparedBattle::new(
        vec![(owner, Default::default()), (target, Default::default())],
        actions,
        1,
    )
    .unwrap();
    prepared.resources.actor_setup[0].control = Some(Arc::new(crate::ControlDefinition {
        normals,
        shortcuts: [0; 4],
        walk_speed: 5.,
        run_speed: 10.,
        turn_ticks: 8,
        motions: None,
    }));
    prepared.finish().unwrap()
}

fn definition(lifetime: u32, velocity: [f32; 3]) -> Arc<ProjectileDefinition> {
    Arc::new(ProjectileDefinition {
        lifetime: (lifetime != 0).then_some(lifetime),
        velocity,
        acceleration: [0.; 3],
        offset: [0.; 3],
        clamp_ground: false,
        active: None,
        birth: None,
        motion: Default::default(),
        effects: ProjectileEffects {
            shadow: Some(ProjectileShadow {
                color: [128, 32, 32, 128],
                additive: true,
                radius: 25.,
            }),
            ..Default::default()
        },
        contact: None,
    })
}

#[test]
fn projectile_shadow_draw_birth_and_expiry_keep_current_simulation() -> Result<()> {
    for hit_stop in [0, 20] {
        let mut battle = battle();
        battle.actors[0].hit_stop = hit_stop;
        let mut definition = definition(1, [2., 0., 0.]);
        Arc::get_mut(&mut definition).unwrap().offset = [3., 0., 0.];
        battle.emit(
            definition,
            ActionId(0),
            ActorId(0),
            ActorId(1),
            [0., 10., 0.],
        )?;
        let born = battle.step(BattleInput::default())?;
        assert_eq!(born.projectile_shadows[0].position, [5., 1.1, 0.]);
        assert_eq!(born.projectiles[0].position, [5., 10., 0.]);
        let id = born.projectiles[0].id;
        let last = battle.step(BattleInput::default())?;
        assert_eq!(last.projectile_shadows.len(), 1);
        assert_eq!(last.projectile_shadows[0].projectile, id);
        assert_eq!(last.projectile_shadows[0].position, [7., 1.1, 0.]);
        assert_eq!(last.projectiles[0].position, [7., 10., 0.]);
        assert!(battle.projectiles[&id].retiring);
        assert_eq!(
            battle.snapshot().projectile_shadows,
            last.projectile_shadows
        );
        let removed = battle.step(BattleInput::default())?;
        assert!(removed.projectiles.is_empty());
        assert!(removed.projectile_shadows.is_empty());
        assert!(
            battle
                .step(BattleInput::default())?
                .projectile_shadows
                .is_empty()
        );
    }
    Ok(())
}

#[test]
fn projectile_shadow_draw_pause_entry_hold_and_resume_sample_without_motion() -> Result<()> {
    for pause in 0..2 {
        let mut battle = battle();
        battle.emit(
            definition(0, [2., 0., 0.]),
            ActionId(0),
            ActorId(0),
            ActorId(1),
            [0., 10., 0.],
        )?;
        battle.step(BattleInput::default())?;
        let current = battle.projectiles.values().next().unwrap().frame.position;
        let expected = [current[0], 1.1, current[2]];
        let random = battle.random_state();
        for _ in 0..3 {
            let mut input = BattleInput::default();
            match pause {
                0 => input.paused = true,
                1 => {
                    battle.target_selector = Some(ActorId(0));
                    let mut control = crate::ControlInput::neutral(ActorId(0));
                    control.target.held = true;
                    input.controllers.push(control);
                }
                _ => {}
            }
            let frame = battle.step(input)?;
            assert_eq!(frame.projectile_shadows[0].position, expected);
            assert_eq!(frame.projectiles[0].position, current);
            assert_eq!(battle.random_state(), random);
        }
        battle.target_selector = None;
        let resumed = battle.step(BattleInput::default())?;
        assert_eq!(
            resumed.projectile_shadows[0].position,
            [current[0] + 2., 1.1, current[2]]
        );
        assert_eq!(resumed.projectiles[0].position[0], current[0] + 2.);
        let next = battle.step(BattleInput::default())?;
        assert_eq!(next.projectile_shadows[0].position[0], current[0] + 4.);
    }
    Ok(())
}

#[test]
fn projectile_shadow_draw_ground_admission_uses_completed_record() -> Result<()> {
    for (height, velocity, visible) in [
        (3., -2., [true, false]),
        (2., -2., [true, false]),
        (-3., 2., [false, true]),
    ] {
        let mut battle = battle();
        battle.emit(
            definition(0, [0., velocity, 0.]),
            ActionId(0),
            ActorId(0),
            ActorId(1),
            [0., height, 0.],
        )?;
        for admitted in visible {
            let frame = battle.step(BattleInput::default())?;
            assert_eq!(!frame.projectile_shadows.is_empty(), admitted);
            if admitted {
                assert_eq!(frame.projectile_shadows[0].position[1], 1.1);
            }
        }
    }
    Ok(())
}

#[test]
fn projectiles_and_shadows_have_no_shared_object_capacity() -> Result<()> {
    let mut battle = battle();
    for _ in 0..500 {
        battle.emit(
            definition(0, [0.; 3]),
            ActionId(0),
            ActorId(0),
            ActorId(1),
            [0., 10., 0.],
        )?;
    }
    let frame = battle.step(BattleInput::default())?;
    assert_eq!(frame.projectiles.len(), 500);
    assert_eq!(frame.projectile_shadows.len(), 500);
    assert!(
        frame
            .projectiles
            .iter()
            .all(|projectile| projectile.shadow.is_some())
    );
    Ok(())
}
