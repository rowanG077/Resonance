use super::*;
use battle::party::Character;
use resonance_battle::{ActorAvailability, BattleResult, Playback};

#[test]
#[ignore = "requires current party models and victory packages; no devices"]
fn every_character_can_play_a_prepared_celebration() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let characters = Character::ALL;
    let sources: Vec<_> = characters
        .iter()
        .map(|&character| battle::model::ModelSource::Party(character))
        .collect();
    let files = battle::model::load_files(&root, field, &sources, &mut cache, || false)?;
    let bodies = characters
        .iter()
        .map(|&character| {
            let body: resonance_content::battle_model::Party = files.json(
                &resonance_content::battle_model::party_path(character as u8),
            )?;
            Ok((character, body.body_sha256))
        })
        .collect::<Result<Vec<_>>>()?;
    let selected: Vec<_> = bodies
        .iter()
        .map(|(character, hash)| (*character as u8, hash.as_str()))
        .collect();
    let (files, source) = battle::victory::load(&root, files, &selected, &mut cache, || false)?;
    for character in characters {
        let member = single_party_fixture(
            &files,
            character as u8,
            0,
            battle::party::Setup {
                position: [0.; 3],
                heading: 0.,
                model: Some(battle::model::ModelSetup {
                    resource: resonance_battle::ResourceId(character as u32),
                    initial: Playback {
                        clip: 0,
                        frame: 0.,
                        rate: 0.5,
                        repeat: true,
                    },
                    suppress_root_translation: [true; 3],
                }),
            },
        )?;
        let body = files.json(&resonance_content::battle_model::party_path(
            character as u8,
        ))?;
        let (model, performances) = battle::victory::prepare(
            &files,
            &source,
            &body,
            character,
            member.model.as_deref().unwrap(),
        )?;
        let posture = battle::victory::prepare_posture(&files, character, Some(&model))?;
        assert_eq!(performances.len(), 5);
        for selected in &performances {
            let timeline = selected
                .timeline
                .as_ref()
                .context("missing victory timeline")?;
            let mut enemy = actor();
            enemy.side = Side::Enemy;
            enemy.hp = 0;
            enemy.availability = ActorAvailability::Dead;
            let prepared = resonance_battle::PreparedBattle::new(
                vec![
                    (member.actor.clone(), Default::default()),
                    (enemy, Default::default()),
                ],
                Default::default(),
                1,
            )?;
            let owner = prepared.actor_ids().next().unwrap();
            let mut live = prepared.finish()?;
            let mut models = resonance_battle::Models::new(
                live.actors(),
                vec![Some(model.clone()), None],
                Default::default(),
                resonance_content::diagnostics::Diagnostics::new(true),
            )?;
            assert_eq!(live.recognize_result(), Some(BattleResult::Victory));
            live.retire_combat()?;
            battle::victory::construct_result_actors(
                &mut live,
                owner,
                &[(owner, character)],
                &[posture],
                battle::lifecycle::victory_selection::ColetteState::Normal,
            )?;
            let mut playing = battle::victory::Playing::start(&mut live, owner, selected)?;
            let mut opening = live.publish(Vec::new());
            models.advance(&mut opening, resonance_battle::BattleClock::Running, false)?;
            let elapsed = timeline.last().context("empty victory timeline")?.0 + 30;
            for _ in 0..elapsed {
                playing.advance(&mut live)?;
                let mut frame = live.step(BattleInput::default())?;
                models.advance(&mut frame, resonance_battle::BattleClock::Running, false)?;
            }
            let pose = models
                .main_motion_observation(owner)
                .context("missing celebration pose")?;
            let idle = matches!(
                timeline.last().unwrap().1,
                resonance_content::battle_victory::Event::Idle { .. }
            );
            assert_eq!(pose.clip, if idle { 0 } else { selected.motion.clip });
            assert!(pose.frame.is_finite());
            assert!(!live.is_diagnostic());
        }
    }
    Ok(())
}
