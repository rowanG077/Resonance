use super::*;
use resonance_battle::{ActorAvailability, BattleResult, Playback};

#[test]
#[ignore = "requires current party models and victory packages; no devices"]
fn every_character_can_play_a_prepared_celebration() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let characters: Vec<_> = (1..=9).collect();
    let sources: Vec<_> = characters
        .iter()
        .map(|&id| battle::model::ModelSource::Party(id))
        .collect();
    let files = battle::model::load_files(&root, field, &sources, &mut cache, || false)?;
    let (files, source) = battle::victory::load(&root, files, &characters, &mut cache, || false)?;
    for character in characters {
        let member = single_party_fixture(
            &files,
            character,
            0,
            battle::party::Setup {
                position: [0.; 3],
                heading: 0.,
                model: Some(battle::model::ModelSetup {
                    resource: u32::from(character),
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
        let body = files.json(&resonance_content::battle_model::party_path(character))?;
        let (model, performances) = battle::victory::prepare(
            &files,
            &source,
            &body,
            character,
            member.model.as_deref().unwrap(),
        )?;
        let posture = battle::victory::prepare_posture(&files, character, Some(&model))?;
        let selected = performances
            .first()
            .context("missing prepared celebration")?;
        let mut enemy = actor();
        enemy.side = Side::Enemy;
        enemy.hp = 0;
        enemy.availability = ActorAvailability::Dead;
        let prepared = resonance_battle::PreparedBattle::new(
            vec![
                (member.actor, Default::default()),
                (enemy, Default::default()),
            ],
            Default::default(),
            1,
        )?;
        let owner = prepared.actor_ids().next().unwrap();
        let mut live = prepared.finish()?;
        let mut models = resonance_battle::Models::new(
            live.actors(),
            vec![Some(model), None],
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
            0,
        )?;
        live.play_victory_pose(owner, selected.motion)?;
        for _ in 0..30 {
            let mut frame = live.step(BattleInput::default())?;
            models.advance(&mut frame, resonance_battle::BattleClock::Running, false)?;
        }
        let pose = models
            .main_motion_observation(owner)
            .context("missing celebration pose")?;
        assert_eq!(pose.clip, selected.motion.clip);
        assert!(pose.frame.is_finite());
        assert!(!live.is_diagnostic());
    }
    Ok(())
}
