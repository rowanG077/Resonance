use super::*;
use crate::BattleClock;
use crate::{
    ActorAvailability, BattleInput, BattleResult, EquipmentReplacement, PreparedBattle, Side,
    tests::actor,
};

#[test]
fn equipment_publication_composes_available_art_without_blocking_stats() -> anyhow::Result<()> {
    let owner = ActorId(0);
    let prepared = PreparedBattle::new(
        vec![(actor(Side::Party), Default::default())],
        Default::default(),
        1,
    )?;
    let mut model = Arc::new(crate::tests::model_definition(7, [(0, 100.)]));
    let part = |slot, resource| {
        Arc::new(crate::WeaponDefinition {
            slot,
            attachment: 0,
            layers: vec![crate::WeaponLayerDefinition {
                resource,
                skeleton: model.skeleton.clone(),
                motions: BTreeMap::new(),
                playback: crate::WeaponPlayback::Rigid,
                secondary_motion: vec![],
            }],
            links: vec![],
        })
    };
    let weapon = part(0, 10);
    let shield = part(1, 11);
    Arc::make_mut(&mut model).weapons.push(weapon.clone());
    let mut battle = prepared.finish()?;
    let mut models = crate::Models::new(
        battle.actors(),
        vec![Some(model.clone())],
        BTreeMap::from([
            ((owner, 1), model.weapons.clone()),
            ((owner, 2), vec![shield]),
        ]),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    for (weapon, arm, expected) in [
        (1, 2, vec![0, 1]),
        (1, 0, vec![0]),
        (99, 2, vec![1]),
        (99, 0, vec![]),
    ] {
        battle.replace_equipment_batch(vec![EquipmentReplacement {
            actor: owner,
            attributes: battle.actors[0].equipment.clone(),
            conditions: battle.actors[0].conditions.clone(),
            equipment: Some([weapon, arm]),
        }])?;
        let mut frame = battle.publish(Vec::new());
        models.advance(&mut frame, crate::BattleClock::Held, false)?;
        let slots: Vec<_> = frame.weapons.iter().map(|part| part.slot).collect();
        assert_eq!(slots, expected);
    }
    let mut attributes = battle.actors[0].equipment.clone();
    attributes.stats.defense += 10;
    battle.replace_equipment_batch(vec![EquipmentReplacement {
        actor: owner,
        attributes: attributes.clone(),
        conditions: Default::default(),
        equipment: Some([1, 2]),
    }])?;
    assert_eq!(battle.actors[0].equipment, attributes);
    Ok(())
}

#[test]
fn completed_frames_own_pose_requests_holds_flights_and_result_visibility() -> Result<()> {
    let owner = ActorId(0);
    let mut stone = actor(Side::Enemy);
    stone.availability = ActorAvailability::Petrified;
    let mut battle = PreparedBattle::new(
        vec![
            (actor(Side::Party), Default::default()),
            (stone, Default::default()),
        ],
        Default::default(),
        1,
    )?
    .finish()?;
    let mut definition = crate::tests::model_definition(7, [(0, 100.), (1, 100.)]);
    definition.initial.rate = 0.5;
    definition.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        attachment: 0,
        layers: vec![WeaponLayerDefinition {
            resource: 8,
            skeleton: definition.skeleton.clone(),
            motions: BTreeMap::new(),
            playback: WeaponPlayback::Rigid,
            secondary_motion: vec![],
        }],
        links: vec![],
    }));
    let definition = Arc::new(definition);
    let mut models = Models::new(
        battle.actors(),
        vec![Some(definition.clone()), Some(definition)],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    battle.request_pose(
        owner,
        Some(MotionBinding { model: 7, clip: 1 }),
        Pose {
            frame: 2.,
            ..Default::default()
        },
    );
    assert!(battle.snapshot().model_requests.is_empty());
    let mut frame = battle.publish(Vec::new());
    assert_eq!(frame.model_requests.len(), 1);
    assert!(battle.publish(Vec::new()).model_requests.is_empty());
    models.advance(&mut frame, BattleClock::Held, false)?;
    assert!(frame.model_requests.is_empty());
    assert_eq!(frame.models[0].frame, 2.);
    let held = frame.models.clone();
    for _ in 0..3 {
        models.advance(&mut frame, BattleClock::Held, false)?;
        assert_eq!(frame.models, held);
    }
    models.advance(&mut frame, BattleClock::Rescue(owner), false)?;
    assert_eq!(frame.models[0].frame, 2.5);
    assert_eq!(frame.models[1], held[1]);
    frame.actors[1].availability = ActorAvailability::Active;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert!(frame.models[1].frame > held[1].frame);

    frame.actors[0].position = [20., 0., 0.];
    frame.weapon_flights.push(crate::WeaponFlightFrame {
        owner,
        slot: 0,
        position: [100., 40., 0.],
        direction: [1., 0., 0.],
    });
    models.advance(&mut frame, BattleClock::Held, false)?;
    assert_eq!(frame.models[0].world[3][0], 20.);
    assert_eq!(frame.weapons[0].world[3], [100., 40., 0., 1.]);
    frame.weapon_flights.clear();
    models.advance(&mut frame, BattleClock::Held, false)?;
    assert_ne!(frame.weapons[0].world[3], [100., 40., 0., 1.]);

    assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
    battle.retire_combat()?;
    battle.reset_result_actor(owner)?;
    battle.hide_result_enemies()?;
    battle.hide_result_weapons(owner)?;
    battle.play_victory_pose(owner, MotionBinding { model: 7, clip: 1 })?;
    let mut frame = battle.step(BattleInput::default())?;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert!(frame.models[0].visible);
    assert_eq!(frame.models[0].tint, [64, 64, 64, 255]);
    assert_eq!(frame.models[0].light, Some([150., 300., 200.]));
    assert!(!frame.models[1].visible);
    assert!(frame.weapons.iter().all(|weapon| !weapon.visible));
    assert!(frame.actions.is_empty());
    Ok(())
}

#[test]
fn visual_faults_cannot_reject_committed_equipment() -> Result<()> {
    for strict in [false, true] {
        let mut battle = PreparedBattle::new(
            vec![(actor(Side::Party), Default::default())],
            Default::default(),
            1,
        )?
        .finish()?;
        let definition = Arc::new(crate::tests::model_definition(7, [(0, 10.)]));
        let broken = Arc::new(WeaponDefinition {
            slot: 0,
            attachment: 99,
            layers: vec![],
            links: vec![],
        });
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(strict);
        battle.set_diagnostics(diagnostics.clone());
        let mut models = Models::new(
            battle.actors(),
            vec![Some(definition)],
            BTreeMap::from([((ActorId(0), 1), vec![broken])]),
            diagnostics.clone(),
        )?;
        let mut attributes = battle.actors[0].equipment.clone();
        attributes.stats.defense += 10;
        battle.replace_equipment_batch(vec![EquipmentReplacement {
            actor: ActorId(0),
            attributes: attributes.clone(),
            conditions: Default::default(),
            equipment: Some([1, 0]),
        }])?;
        let mut frame = battle.publish(Vec::new());
        assert_eq!(
            models
                .advance(&mut frame, BattleClock::Held, false)
                .is_err(),
            strict
        );
        assert_eq!(battle.actors[0].equipment, attributes);
        assert!(frame.weapons.is_empty());
        assert!(!battle.is_diagnostic());
        if !strict {
            assert!(diagnostics.has_errors());
        }
    }
    Ok(())
}

#[test]
fn appearance_follows_casting_defeat_holds_revival_and_results() -> Result<()> {
    let mut definition = crate::tests::model_definition(7, [(0, 30.), (1, 30.), (2, 30.)]);
    definition.tint = [40, 50, 60, 173];
    definition.fade_on_defeat = true;
    definition.reactions.defeated = Some(1);
    definition.reactions.get_up = Some(2);
    let battle = PreparedBattle::new(
        vec![(actor(Side::Party), Default::default())],
        Default::default(),
        1,
    )?
    .finish()?;
    let mut models = Models::new(
        battle.actors(),
        vec![Some(Arc::new(definition))],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let mut frame = battle.snapshot();
    frame.actors[0].position = [10., 0., 20.];
    frame.actors[0].activity = crate::Activity::Casting { held: true };
    models.advance(&mut frame, BattleClock::Held, true)?;
    assert_eq!(frame.models[0].tint, [40, 50, 60, 173]);
    assert!(!frame.models[0].depth_write);
    assert_eq!(frame.models[0].light, Some([10., 0., 20.]));
    frame.actors[0].activity = crate::Activity::Idle;
    models.advance(&mut frame, BattleClock::Running, true)?;
    assert_eq!(frame.models[0].light, None);
    frame.actors[0].availability = ActorAvailability::Dead;
    frame.actors[0].activity = crate::Activity::Defeated;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert_eq!(
        frame.models[0].clip, 1,
        "death settles during the ending too"
    );
    assert!(frame.models[0].tint[3] < 173);
    let held = frame.models[0].clone();
    for _ in 0..30 {
        models.advance(&mut frame, BattleClock::Held, false)?;
        assert_eq!(frame.models[0], held);
    }
    for _ in 0..30 {
        models.advance(&mut frame, BattleClock::Running, false)?;
    }
    assert_eq!(frame.models[0].tint[3], 0);
    assert!(!frame.models[0].visible);
    frame.actors[0].availability = ActorAvailability::Active;
    frame.actors[0].activity = crate::Activity::GettingUp;
    models.advance(&mut frame, BattleClock::Running, true)?;
    assert_eq!(frame.models[0].clip, 2);
    assert_eq!(frame.models[0].tint, [40, 50, 60, 173]);
    assert!(frame.models[0].visible);
    frame.model_requests.push(crate::ModelRequest::Result {
        actor: ActorId(0),
        visible: true,
    });
    frame.actors[0].availability = ActorAvailability::Dead;
    frame.actors[0].activity = crate::Activity::Defeated;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert_eq!(frame.models[0].clip, 2, "result poses keep ownership");
    assert_eq!(frame.models[0].tint, [64, 64, 64, 255]);
    assert!(frame.models[0].depth_write);
    assert_eq!(frame.models[0].light, Some([160., 300., 220.]));
    Ok(())
}
