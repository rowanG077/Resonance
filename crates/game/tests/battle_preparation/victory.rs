use super::*;
use resonance_battle::{ActorAvailability, BattleResult, MotionBinding, Playback, SoundBinding};
use resonance_content::battle_victory::{PATH, Performances};

struct Motions(Vec<battle::victory::Performance>);
impl BattleResources for Motions {
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        if let Some((name, clip)) = path
            .strip_prefix("battle/motions/")
            .and_then(|path| path.split_once('/'))
            && let Some(model) = ["lloyd", "colette", "genis"]
                .iter()
                .position(|&value| value == name)
        {
            return Ok(MotionBinding {
                model: model as u32 + 1,
                clip: clip.parse()?,
            });
        }
        battle::victory::motion(path, &self.0)
    }
    fn sound(&mut self, _: &str) -> Result<SoundBinding> {
        bail!("unexpected victory sound")
    }
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("unexpected victory cast")
    }
    fn effect(&mut self, _: &str) -> Result<EffectResource> {
        bail!("unexpected victory effect")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("unexpected victory particle")
    }
    fn projectile(&mut self, _: &str) -> Result<ProjectileResource> {
        bail!("unexpected victory projectile")
    }
    fn melee(&mut self, _: &str) -> Result<MeleeResource> {
        bail!("unexpected victory contact")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("unexpected victory spell")
    }
}

#[test]
#[ignore = "requires current partial victory, profile, party and script publications; no devices"]
fn cold_all_victory_selectors_prepare_before_results_and_keep_original_rows() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let field = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        field,
        &[
            battle::model::ModelSource::Party(1),
            battle::model::ModelSource::Party(2),
            battle::model::ModelSource::Party(3),
            battle::model::ModelSource::Weapon(175),
        ],
        &mut cache,
        || false,
    )?;
    let files = battle::victory::load_files(&root, files, &mut cache, || false)?;
    let source: Performances = files.json(PATH)?;
    let mut actors = Vec::new();
    let mut models = Vec::new();
    let mut performances = Vec::new();
    let mut bindings = Vec::new();
    let mut postures = Vec::new();
    for character in 1..=3 {
        let ids = std::array::from_fn(|selector| u16::from(character) * 10 + selector as u16);
        let (actor, mut model) = battle::model::party(
            &files,
            character,
            actor(),
            battle::model::ModelSetup {
                resource: u32::from(character),
                initial: Playback {
                    clip: 0,
                    frame: 0.,
                    rate: 0.5,
                    repeat: true,
                },
                suppress_root_translation: [true; 3],
                stun: None,
            },
        )?;
        if character == 3 {
            let body: resonance_content::battle_model::Party =
                files.json(&resonance_content::battle_model::party_path(character))?;
            let weapon = battle::model::weapon(&files, 175)?;
            for (&slot, part) in &weapon.parts {
                battle::weapon::attach(
                    &files,
                    Arc::make_mut(&mut model),
                    part,
                    slot,
                    body.body.attachments[&slot],
                    30 + u32::from(slot),
                    battle::weapon::owner_linked(),
                )?;
            }
        }
        let (model, selected) = battle::victory::prepare(&files, character, &model, ids)?;
        let posture_ids =
            std::array::from_fn(|index| 100 + u16::from(character) * 10 + index as u16);
        postures.push(battle::victory::prepare_posture(
            &files,
            character,
            &model,
            posture_ids,
        )?);
        bindings.extend(battle::victory::posture_bindings(character, posture_ids)?);
        assert!(battle::victory::prepare(&files, character, &model, ids).is_err());
        for weapon in &model.weapons {
            let original = weapon
                .motions
                .get(&83)
                .unwrap_or(&weapon.motions[&60])
                .encode()?;
            for selector in 0..5 {
                assert_eq!(weapon.motions[&(0x100 + selector + 60)].encode()?, original);
            }
        }
        actors.push(actor);
        models.push(Some(model));
        performances.extend(selected);
        bindings.extend(battle::victory::bindings(character, ids)?);
    }
    let mut enemy = actor();
    enemy.side = Side::Enemy;
    enemy.hp = 0;
    enemy.availability = ActorAvailability::Dead;
    actors.push(enemy);
    models.push(None);
    let prepared = battle::prepare(
        &mut PreparationCache::default(),
        &files,
        &bindings,
        actors.clone(),
        1,
        &mut Motions(performances.clone()),
        models.clone(),
    )?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    for performance in &performances {
        let mut live = Battle::new(prepared.clone());
        assert_eq!(live.recognize_result(), Some(BattleResult::Victory));
        live.retire_combat()?;
        let actor = ids[usize::from(performance.character - 1)];
        live.start_result_action(actor, performance.action)?;
        let row = source
            .ordinary
            .iter()
            .find(|row| {
                row.character == performance.character && row.selector == performance.selector
            })
            .unwrap();
        let first = live.step(BattleInput::default())?;
        let shown = first
            .models
            .iter()
            .find(|model| model.actor == actor)
            .unwrap();
        assert_eq!(shown.clip, performance.motion.clip);
        assert_eq!(
            shown.texture_layers,
            [if performance.character == 3 { 0 } else { 4 }, 0, 0, 0]
        );
        if let Some(next) = row.animations.get(2).filter(|row| row.time > 0) {
            // One first callback at counter0; exactly T further callbacks reach T.
            for _ in 1..next.time {
                live.step(BattleInput::default())?;
            }
            let frame = live.step(BattleInput::default())?;
            let shown = frame
                .models
                .iter()
                .find(|model| model.actor == actor)
                .unwrap();
            assert_eq!(shown.clip, performance.motion.clip);
            assert!(shown.frame >= f32::from(next.start) * next.rate.finite()?);
        }
    }
    let roster: Vec<_> = ids.iter().copied().zip(1..=3).collect();
    for sealed in [false, true] {
        let mut actors = actors.clone();
        actors[0].hp = 1;
        actors[0].overlimit = 1000;
        actors[0].overlimit_active = true;
        actors[1].hp = 0;
        actors[1].availability = if sealed {
            ActorAvailability::Petrified
        } else {
            ActorAvailability::Dead
        };
        actors[1].petrified = sealed;
        actors[1].activity = resonance_battle::Activity::Defeated;
        actors[2].hp = 25;
        actors[2].overlimit = 1000;
        actors[2].overlimit_active = true;
        for actor in &mut actors[..3] {
            actor.position[1] = 60.;
            actor.movement.vertical = -2.;
            actor.hit_stop = 8;
        }
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &bindings,
            actors,
            1,
            &mut Motions(performances.clone()),
            models.clone(),
        )?;
        let mut live = Battle::new(prepared);
        assert_eq!(live.recognize_result(), Some(BattleResult::Victory));
        live.retire_combat()?;
        live.start_result_action(ids[0], performances[0].action)?;
        let cues = battle::victory::construct_result_actors(
            &mut live,
            ids[0],
            &roster,
            &postures,
            if sealed { 1000 } else { 2500 },
        )?;
        assert!(
            cues.iter()
                .any(|cue| matches!(cue, resonance_battle::Cue::Interrupted { .. }))
        );
        let frame = live.step(BattleInput::default())?;
        assert_eq!(frame.models[0].clip, 0); // Low HP leader remains healthy.
        assert_eq!(frame.models[2].clip, 26);
        assert_eq!(frame.models[2].texture_layers, [10, 5, 0, 0]);
        assert_eq!(frame.models[1].clip, if sealed { 0 } else { 26 });
        assert_eq!(
            frame.models[1].texture_layers,
            if sealed { [15, 0, 0, 0] } else { [10, 5, 0, 0] }
        );
        assert_eq!(live.actors()[1].hp, 0);
        assert_eq!(
            live.actors()[1].availability,
            if sealed {
                ActorAvailability::Petrified
            } else {
                ActorAvailability::Active
            }
        );
        assert_eq!(live.actors()[0].overlimit, 0);
        assert_eq!(live.actors()[2].overlimit, 1000); // Weak branch does not clear active gauge.
        for actor in &live.actors()[..3] {
            assert_eq!(actor.position[1], 0.);
            assert_eq!(actor.activity, resonance_battle::Activity::Idle);
            assert_eq!(actor.hit_stop, 0);
        }
    }
    let mut missing = files.clone();
    missing.bytes.remove(&source.ordinary[0].motion);
    // Every published byte was already verified before the candidate above.
    assert!(missing.read(&source.ordinary[0].motion).is_err());
    Ok(())
}
