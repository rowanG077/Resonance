use super::*;

#[test]
fn idle_formation_occupancy_uses_size_and_a_strict_editable_boundary() -> Result<()> {
    for (enemy_x, unit, available, moved) in [
        (607.5, 100., true, true),
        (608.5, 100., true, false),
        (657.5, 100., true, false),
        (657.5, 50., true, true),
        (807.5, 100., false, true),
    ] {
        let mut prepared = prepared()?;
        for i in [1, 4] {
            prepared.actors[i].availability = crate::ActorAvailability::Absent;
        }
        let companion = prepared.resources.actor_setup[2]
            .companion
            .as_mut()
            .unwrap();
        companion.initial_policy = [1, 5, 3];
        companion.positioning.formation_occupancy_unit = unit;
        prepared.actors[3].position = [enemy_x, 0., 0.];
        prepared.actors[3].movement.steering.positioning_radius = 1;
        if !available {
            prepared.actors[3].availability = crate::ActorAvailability::Dead;
            prepared.actors[3].hp = 0;
        }
        let mut battle = prepared.finish()?;
        battle.actors[0].movement.steering.home = [300., 0., -300.];
        battle.actors[2].movement.steering.home_offset = [600., 0., 300.];
        assert_eq!(
            battle.return_position(ActorId(2))?,
            moved,
            "enemy {enemy_x}, unit {unit}, available {available}"
        );
    }
    Ok(())
}

#[test]
fn post_action_formation_destination_remains_anchored_when_an_actor_occupies_it() -> Result<()> {
    let mut prepared = prepared()?;
    for i in [1, 4] {
        prepared.actors[i].availability = crate::ActorAvailability::Absent;
    }
    prepared.resources.actor_setup[2]
        .companion
        .as_mut()
        .unwrap()
        .initial_policy = [1, 5, 2];
    prepared.resources.actor_setup[2].recovery_return = Some(crate::RecoveryReturnDefinition {
        speed: 9.5,
        run_speed: 9.5,
        turn_ticks: 8,
        disabled: false,
    });
    prepared.actors[3].position = [0.; 3];
    let mut battle = prepared.finish()?;
    battle.actors[0].movement.steering.home = [-600., 0., -300.];
    battle.actors[2].movement.steering.home_offset = [600., 0., 300.];
    battle.actors[2].position = [250., 0., 0.];
    assert!(battle.begin_recovery_return(ActorId(2))?);
    assert_eq!(battle.actors[2].movement.steering.home, [0.; 3]);
    assert!(matches!(
        battle.runtime[2].task(),
        crate::state::ActorTask::Returning(_)
    ));
    Ok(())
}
