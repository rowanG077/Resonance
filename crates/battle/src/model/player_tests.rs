use super::*;
use crate::BattleClock;
use crate::{
    ActorAvailability, BattleInput, BattleResult, EquipmentReplacement, PreparedBattle, Side,
    tests::actor,
};

#[cfg(feature = "test-support")]
#[test]
fn fixture_hidden_defeat_requires_death_and_ends_on_revival() -> Result<()> {
    let mut battle = crate::tests::prepared(vec![actor(Side::Enemy)], 1).finish()?;
    let mut models = Models::new(
        battle.actors(),
        vec![Some(Arc::new(crate::tests::model_definition(
            7,
            [(0, 100.)],
        )))],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let restore = ModelRequest::FixtureClock {
        actor: ActorId(0),
        loop_start: 0.,
        elapsed_updates: 0,
        secondary: None,
        defeated_hidden: true,
    };
    let mut frame = battle.publish(Vec::new());
    frame.model_requests.push(restore.clone());
    assert!(
        models
            .advance(&mut frame, BattleClock::Held, false)
            .is_err()
    );
    frame.actors[0].state.hp = 0;
    frame.actors[0].state.availability = ActorAvailability::Dead;
    frame.model_requests.push(restore);
    for _ in 0..10 {
        frame.update += 1;
        models.advance(&mut frame, BattleClock::Running, false)?;
        assert!(!frame.models[0].visible);
        assert!(!models.models[0].as_ref().unwrap().in_results);
    }
    frame.actors[0].state.hp = 1;
    frame.actors[0].state.availability = ActorAvailability::Active;
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert!(frame.models[0].visible);
    Ok(())
}

#[test]
fn spell_release_animates_caster_attachments_without_advancing_textures_or_peers() -> Result<()> {
    let mut battle = crate::tests::prepared(vec![actor(Side::Enemy); 2], 1).finish()?;
    let mut definition = crate::tests::model_definition(7, [(0, 100.)]);
    let initial = definition.initial;
    for (slot, playback) in [
        crate::WeaponPlayback::Local(initial),
        crate::WeaponPlayback::Owner {
            offset: 0,
            fallback: 0,
            initial,
        },
        crate::WeaponPlayback::Body { offset: 0, initial },
        crate::WeaponPlayback::Rigid,
    ]
    .into_iter()
    .enumerate()
    {
        definition
            .weapons
            .push(Arc::new(crate::WeaponDefinition::new(
                slot as u8,
                0,
                vec![crate::WeaponLayerDefinition {
                    resource: 8 + slot as u32,
                    skeleton: definition.skeleton.clone(),
                    motions: definition.motions.clone(),
                    playback,
                    secondary_motion: vec![],
                    texture_animation: Some(crate::WeaponTextureAnimation::Scroll {
                        texture: 0,
                        initial: [0, 0],
                        step: [1, 0],
                    }),
                }],
                vec![],
            )?));
    }
    let mut models = Models::new(
        battle.actors(),
        vec![Some(Arc::new(definition)); 2],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    let mut frame = battle.publish(Vec::new());
    models.advance(&mut frame, BattleClock::Held, false)?;
    let bodies = frame.models.clone();
    let attachments = frame.weapons.clone();
    for tick in 1..=3 {
        frame.update += 1;
        for _ in 0..2 {
            models.advance_with_release(
                &mut frame,
                BattleClock::SpellTransition,
                false,
                Some(ActorId(0)),
                &[],
            )?;
            assert_eq!(
                frame.models[0].frame,
                bodies[0].frame + tick as f32 * initial.rate
            );
            assert_eq!(frame.models[1].frame, bodies[1].frame);
            for (weapon, before) in frame.weapons.iter().zip(&attachments) {
                let expected = before.frame
                    + if weapon.owner == ActorId(0) && weapon.slot < 3 {
                        tick as f32 * initial.rate
                    } else {
                        0.
                    };
                assert_eq!(
                    weapon.frame, expected,
                    "attachment {} of {:?}",
                    weapon.slot, weapon.owner
                );
                assert_eq!(weapon.texture_scroll, before.texture_scroll);
            }
        }
    }
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, false)?;
    for (weapon, before) in frame.weapons.iter().zip(&attachments) {
        let (texture, [u, v]) = before.texture_scroll.unwrap();
        assert_eq!(weapon.texture_scroll, Some((texture, [u + 0.001, v])));
    }
    Ok(())
}

#[test]
fn action_tint_restores_stage_color_during_hit_stop_but_not_global_pause() -> Result<()> {
    let mut battle = crate::tests::prepared(vec![actor(Side::Party)], 1).finish()?;
    let mut definition = crate::tests::model_definition(7, [(0, 100.)]);
    definition.tint = [61, 60, 58, 255];
    let mut models = Models::new(
        battle.actors(),
        vec![Some(Arc::new(definition))],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    battle.model_requests.push(ModelRequest::Tint {
        actor: ActorId(0),
        color: [64, 0, 0],
    });
    let mut frame = battle.publish(Vec::new());
    frame.paused_actors.push(ActorId(0));
    models.advance(&mut frame, BattleClock::Held, false)?;
    assert_eq!(frame.models[0].tint, [64, 0, 0, 255]);
    for tick in 1_u8..=60 {
        frame.update += 1;
        models.advance(&mut frame, BattleClock::Running, false)?;
        let expected = [(64 - tick).max(61), tick.min(60), tick.min(58), 255];
        assert_eq!(frame.models[0].tint, expected);
        models.advance(&mut frame, BattleClock::Running, false)?;
        assert_eq!(frame.models[0].tint, expected);
    }
    assert!(models.models[0].as_ref().unwrap().tint_flash.is_none());
    Ok(())
}

#[test]
fn hit_stop_resumes_actor_before_model_playback() -> Result<()> {
    let step = |battle: &mut crate::Battle| -> Result<crate::BattleFrame> {
        let mut frame = battle.step(BattleInput::default())?;
        battle.advance_models(&mut frame, BattleClock::Running, false)?;
        Ok(frame)
    };
    let mut battle = PreparedBattle::new(
        vec![
            (actor(Side::Party), Default::default()),
            (actor(Side::Enemy), Default::default()),
        ],
        Default::default(),
        1,
    )?
    .finish()?;
    battle.set_models(Models::new(
        battle.actors(),
        vec![
            Some(Arc::new(crate::tests::model_definition(7, [(0, 100.)]))),
            None,
        ],
        BTreeMap::new(),
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    battle.request_pose(
        ActorId(0),
        Some(MotionBinding { model: 7, clip: 0 }),
        Pose {
            frame: 8.5,
            blend: 0,
            ..Default::default()
        },
    );
    assert_eq!(step(&mut battle)?.models[0].frame, 8.5);
    assert_eq!(step(&mut battle)?.models[0].frame, 9.);
    battle.actors[0].hit_stop = 2;
    // Recorded half-frame playback: two actor holds, then one model resume hold.
    for expected in [9., 9., 9., 9.5] {
        let mut frame = step(&mut battle)?;
        assert_eq!(frame.models[0].frame, expected);
        battle.advance_models(&mut frame, BattleClock::Running, false)?;
        assert_eq!(frame.models[0].frame, expected);
    }
    battle.actors[0].time_stop = 2;
    for expected in [9.5, 10.] {
        assert_eq!(step(&mut battle)?.models[0].frame, expected);
    }
    Ok(())
}

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
            attachment: Some(0),
            effect_interval: None,
            initially_visible: true,
            layers: vec![crate::WeaponLayerDefinition {
                texture_animation: None,
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
    let temporary = part(0, 12);
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

    let mut frame = battle.publish(Vec::new());
    models.advance(&mut frame, BattleClock::Held, false)?;
    models.models[0]
        .as_mut()
        .unwrap()
        .weapon_visible(0, false)?;
    let ordinary = models.models[0].as_ref().unwrap().weapon_frames();
    models.begin_unison_weapons();
    models.override_unison_weapons(owner, std::slice::from_ref(&temporary))?;
    let shown = models.models[0].as_ref().unwrap().weapon_frames();
    assert_eq!(shown[0].resource, 12);
    assert!(shown[0].visible);
    assert_eq!(shown[1], ordinary[1]);
    models.restore_unison_weapons(owner);
    assert_eq!(models.models[0].as_ref().unwrap().weapon_frames(), ordinary);

    models.override_unison_weapons(owner, &[temporary])?;
    battle.replace_equipment_batch(vec![EquipmentReplacement {
        actor: owner,
        attributes: battle.actors[0].equipment.clone(),
        conditions: battle.actors[0].conditions.clone(),
        equipment: Some([0, 2]),
    }])?;
    let mut frame = battle.publish(Vec::new());
    models.advance(&mut frame, BattleClock::Held, false)?;
    assert_eq!(
        frame
            .weapons
            .iter()
            .map(|weapon| weapon.resource)
            .collect::<Vec<_>>(),
        [12, 11]
    );
    models.end_unison_weapons();
    assert_eq!(
        models.models[0].as_ref().unwrap().weapon_frames(),
        vec![ordinary[1].clone()]
    );
    models.end_unison_weapons();
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
    definition.idle_motions[0] = Some(0);
    definition.weapons.push(Arc::new(WeaponDefinition {
        slot: 0,
        attachment: Some(0),
        effect_interval: None,
        initially_visible: true,
        layers: vec![WeaponLayerDefinition {
            texture_animation: None,
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
            blend: 0,
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
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Rescue(owner), false)?;
    assert_eq!(frame.models[0].frame, 2.5);
    assert_eq!(frame.models[1], held[1]);
    frame.actors[1].availability = ActorAvailability::Active;
    frame.update += 1;
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

    models.models[0].as_mut().unwrap().play(
        MotionBinding { model: 7, clip: 1 },
        100.,
        0.5,
        false,
        0,
    )?;
    frame.actors[0].activity = crate::Activity::Recovering;
    for _ in 0..3 {
        frame.update += 1;
        models.advance(&mut frame, BattleClock::Running, true)?;
        assert_eq!(frame.models[0].clip, 1, "common recovery retains its pose");
    }
    frame.actions.push((crate::ActionId(1), owner, 0));
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, true)?;
    assert_eq!(
        frame.models[0].clip, 0,
        "authored action recovery can settle"
    );

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
            attachment: Some(99),
            effect_interval: None,
            initially_visible: true,
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
    definition.reactions.stagger_get_up = Some(2);
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
    assert!(
        frame.models[0].depth_write,
        "authored translucency must still occlude the back-facing outline"
    );
    assert_eq!(frame.models[0].light, Some([10., 0., 20.]));
    frame.actors[0].activity = crate::Activity::Idle;
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, true)?;
    assert_eq!(frame.models[0].light, None);
    frame.actors[0].availability = ActorAvailability::Dead;
    frame.actors[0].activity = crate::Activity::Defeated;
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert_eq!(
        frame.models[0].clip, 1,
        "death settles during the ending too"
    );
    assert_eq!(frame.models[0].tint, [0, 0, 0, 236]);
    assert!(!frame.models[0].depth_write, "defeat fading releases depth");
    let held = frame.models[0].clone();
    for _ in 0..30 {
        models.advance(&mut frame, BattleClock::Held, false)?;
        assert_eq!(frame.models[0], held);
    }
    frame.paused_actors.push(ActorId(0));
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, false)?;
    assert_eq!(frame.models[0].tint, [0, 0, 0, 224]);
    assert_eq!(frame.models[0].frame, held.frame);
    frame.paused_actors.clear();
    for _ in 0..30 {
        frame.update += 1;
        models.advance(&mut frame, BattleClock::Running, false)?;
    }
    assert_eq!(frame.models[0].tint[3], 0);
    assert!(!frame.models[0].visible);
    frame.actors[0].availability = ActorAvailability::Active;
    frame.actors[0].activity = crate::Activity::GettingUp;
    frame.update += 1;
    models.advance(&mut frame, BattleClock::Running, true)?;
    assert_eq!(frame.models[0].clip, 2);
    assert_eq!(frame.models[0].tint, [40, 50, 60, 173]);
    assert!(frame.models[0].visible);
    assert!(frame.models[0].depth_write, "revival restores body depth");
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
