use super::*;
use crate::prepare_party_fixture;
use resonance_battle::{Activity, ControlInput, Cue, NormalAttack, Playback};
use resonance_content::session::SessionData;

#[test]
#[ignore = "requires current party and weapon publications; no devices"]
fn all_nine_characters_normal_attacks_hit_and_finish() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let weapons = [135, 159, 175, 190, 205, 228, 242, 258, 228];
    let mut models: Vec<_> = battle::party::Character::ALL
        .into_iter()
        .map(battle::model::ModelSource::Party)
        .collect();
    models.extend(weapons.map(battle::model::ModelSource::Weapon));
    models.push(battle::model::ModelSource::Enemy(36));
    let files = battle::model::load_files(&root, files, &models, &mut cache, || false)?;
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let session: SessionData = files.json("game/session-data.json")?;
    let mut party = resonance_events::party::Party::new(&session, Default::default())
        .map_err(anyhow::Error::msg)?;
    let mut missed_contacts = Vec::new();
    for character in 1..=9 {
        let index = usize::from(character - 1);
        let name = format!("{:?}", battle::party::Character::try_from(character)?);
        party.formation = vec![character];
        party.settings.battle_controls[0] = 0;
        let member = &mut party.members[index];
        member.equipment = [0; 6];
        member.equipment[0] = weapons[index];
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.hp = 1;
        member.tp = 0;
        let prepared_owner = prepare_party_fixture(
            &files,
            &menu,
            &party,
            vec![battle::party::Setup {
                position: [0.; 3],
                heading: 0.,
                model: Some(battle::model::ModelSetup {
                    resource: resonance_battle::ResourceId(1),
                    initial: Playback {
                        clip: 0,
                        frame: 0.,
                        rate: 0.5,
                        repeat: true,
                    },
                    suppress_root_translation: [true; 3],
                }),
            }],
        )
        .with_context(|| format!("prepare {name} normal timing owner"))?
        .remove(0);
        let (mut owner, model) = (prepared_owner.actor, prepared_owner.model.unwrap());
        owner.hp = owner.equipment.max_hp;
        owner.movement.direction = [0., 0., 1.];
        owner.movement.target_direction = owner.movement.direction;
        owner.facing_direction = owner.movement.direction;
        owner.movement.gravity = -1.;
        if character == 5 {
            owner.equipment.base_element = Some(resonance_battle::Element::Fire);
        }
        let (bindings, _) = super::normal_attack::resources(&files, character, &model)?;
        // Contacts require a recipient with complete reaction resources.
        let enemy = prepare_enemy_fixture(&files, menu.monsters()?.records[36].id, 0, 2)?;
        let mut target = enemy.actor;
        target.position = [0., 0., 100.];
        target.hp = 10000;
        target.equipment.max_hp = 10000;
        let control = super::normal_attack::control(&files, character, &model)?;
        let mut playable = std::collections::BTreeSet::new();
        for airborne in [false, true] {
            for x in [-127, 0, 127] {
                for y in [-127, 0, 127] {
                    playable.insert(NormalAttack::from_input([x, y], airborne) as usize);
                }
            }
        }
        loop {
            let previous = playable.clone();
            for &selection in &previous {
                let continuation = control.normals[selection].continuation;
                playable.extend(continuation.fallback.map(|attack| attack as usize));
                playable.extend(
                    (0..bindings.len()).filter(|&i| continuation.directions & (1 << i) != 0),
                );
            }
            if previous == playable {
                break;
            }
        }
        let prepare = |selection: usize| -> Result<_> {
            let owner = owner.clone();
            let mut target = target.clone();
            if NormalAttack::ALL[selection].airborne() || character == 3 && selection == 1 {
                // Upward and aerial strikes have an airborne recipient.
                target.position[1] = owner.body_top();
                target.movement.flying = true;
                target.movement.hover_height = target.position[1];
                target.movement.gravity = 0.;
            }
            resonance_battle::PreparedBattle::new(
                vec![
                    (
                        owner,
                        resonance_battle::ActorSetup {
                            control: Some(control.clone()),
                            ..Default::default()
                        },
                    ),
                    (target, Default::default()),
                ],
                bindings.to_vec().into(),
                0,
            )
            .with_context(|| format!("prepare {name} normal timing actions"))
        };
        for selection in playable {
            let prepared = prepare(selection)?;
            let ids: Vec<_> = prepared.actor_ids().collect();
            let mut active = prepared.finish()?;
            let models = resonance_battle::Models::new(
                active.actors(),
                vec![Some(model.clone()), enemy.model.clone()],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?;
            active.set_models(models);
            active.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(true));
            active.step(BattleInput::default())?;
            if NormalAttack::ALL[selection].airborne() {
                for _ in 0..120 {
                    active.step(BattleInput::from(ControlInput {
                        stick: [0, 127],
                        ..ControlInput::neutral(ids[0])
                    }))?;
                    if active.actors()[0].position[1] > 0. {
                        break;
                    }
                }
                assert!(active.actors()[0].position[1] > 0., "{name} did not jump");
            }
            let (mut voiced, mut sounded, mut recovered, mut completed) =
                (false, false, false, false);
            let (mut effect, mut hidden_cards, mut hit) = (false, false, false);
            for update in 0..180 {
                let mut frame = active.step(BattleInput {
                    actions: if update == 0 {
                        vec![ActionRequest {
                            actor: ids[0],
                            target: ids[1],
                            action: resonance_battle::ActionKey(selection),
                        }]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })?;
                let clock = frame.clock;
                active.advance_models(&mut frame, clock, true)?;
                for cue in &frame.cues {
                    match cue {
                        Cue::Voice { .. } => voiced = true,
                        Cue::Sound { .. } => sounded = true,
                        Cue::Hit { actor, result, .. }
                            if *actor == ids[1] && result.is_damage() =>
                        {
                            hit = true
                        }
                        Cue::Rejected { .. } => panic!("{name} selector {selection} rejected"),
                        _ => {}
                    }
                }
                effect |= crate::effects(&frame).next().is_some();
                hidden_cards |= frame.weapons.iter().any(|weapon| !weapon.visible);
                recovered |= frame.actors[0].activity == Activity::Recovering;
                if frame.actions.is_empty() {
                    completed = true;
                    break;
                }
            }
            assert!(
                completed && recovered && voiced && sounded,
                "{name} selector {selection} did not complete with attack feedback"
            );
            if !hit {
                missed_contacts.push(format!("{name} selector {selection}"));
            }
            if character == battle::party::Character::Sheena as u8 {
                assert!(
                    hidden_cards,
                    "Sheena selector {selection} did not select a card hand"
                );
                assert_eq!(
                    effect,
                    !matches!(selection, 1 | 4),
                    "Sheena selector {selection} effect"
                );
            }
        }
    }
    assert!(
        missed_contacts.is_empty(),
        "normal attacks did not hit the nearby target: {}",
        missed_contacts.join(", ")
    );
    Ok(())
}
