use super::*;
use crate::prepare_party_fixture;
use resonance_battle::{Activity, Cue, Playback};
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
    let mut models: Vec<_> = (1..=9).map(battle::model::ModelSource::Party).collect();
    models.extend(weapons.map(battle::model::ModelSource::Weapon));
    models.push(battle::model::ModelSource::Enemy(36));
    let files = battle::model::load_files(&root, files, &models, &mut cache, || false)?;
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let session: SessionData = files.json("game/session-data.json")?;
    let mut party = resonance_events::party::Party::new(&session, Default::default())
        .map_err(anyhow::Error::msg)?;
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
                    resource: 1,
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
        let prepare = |aerial| -> Result<_> {
            let mut owner = owner.clone();
            if aerial {
                // Start during ascent so slower aerial windups can reach contact before landing.
                owner.position[1] = 80.;
                owner.movement.vertical = 8.;
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
                    (target.clone(), Default::default()),
                ],
                bindings.to_vec().into(),
                0,
            )
            .with_context(|| format!("prepare {name} normal timing actions"))
        };
        for selection in 0..bindings.len() {
            let prepared = prepare(selection >= 5)?;
            let ids: Vec<_> = prepared.actor_ids().collect();
            let mut active = prepared.finish()?;
            let mut models = resonance_battle::Models::new(
                active.actors(),
                vec![Some(model.clone()), None],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?;
            active.set_diagnostics(resonance_content::diagnostics::Diagnostics::new(true));
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
                models.advance(&mut frame, resonance_battle::BattleClock::Running, true)?;
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
            assert!(
                hit,
                "{name} selector {selection} did not hit the nearby target"
            );
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
    Ok(())
}
