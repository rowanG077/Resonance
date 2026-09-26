use super::*;
use resonance_battle::{Cue, MotionBinding, Playback, SoundBinding, VoiceLine};
use resonance_content::{battle_action, battle_effect};

struct Original<'a> {
    files: &'a Files,
    character: u8,
    bank: EffectResource,
}

impl BattleResources for Original<'_> {
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("martial cast")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("martial particle")
    }
    fn melee(&mut self, _: &str) -> Result<MeleeResource> {
        bail!("martial melee")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("martial spell")
    }
    fn voice(&mut self, path: &str) -> Result<Vec<Option<VoiceLine>>> {
        use battle::{model::ModelSource, voice::Sound};
        let line = path
            .strip_prefix("battle/voices/absolute/")
            .context("martial voice")?
            .parse()?;
        battle::voice::absolute(
            self.files,
            &[ModelSource::Party(self.character), ModelSource::Enemy(49)],
            line,
            |sound| {
                Ok(match sound {
                    Sound::Cue(index) => SoundBinding { resource: 1, index },
                    Sound::Stream(index) => SoundBinding { resource: 2, index },
                })
            },
        )
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        Ok(SoundBinding {
            resource: 1,
            index: path
                .strip_prefix("battle/sounds/common/")
                .context("martial sound")?
                .parse()?,
        })
    }
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        let (character, clip) = battle::martial::motion(path).context("martial motion")?;
        assert_eq!(character, self.character);
        Ok(MotionBinding { model: 1, clip })
    }
    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        battle::martial::effect(path, &self.bank)
    }
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        battle::martial::projectile_for_path(self.files, path, &self.bank)
    }
}

fn setup(resource: u32) -> battle::model::ModelSetup {
    battle::model::ModelSetup {
        resource,
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        suppress_root_translation: [true; 3],
        stun: None,
    }
}

#[test]
#[ignore = "requires current opening martial, actor, weapon and effect publications; no devices"]
fn opening_martials_debit_original_cost_and_run_source_clocks_projectiles_and_models() -> Result<()>
{
    use battle::model::{self, ModelSource};
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = model::load_files(
        &root,
        files,
        &[
            ModelSource::Party(1),
            ModelSource::Party(2),
            ModelSource::Weapon(159),
            ModelSource::Enemy(49),
        ],
        &mut cache,
        || false,
    )?;
    let table: battle_action::Table = files.json(battle_action::MARTIAL_PATH)?;
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    for (technique, character, cost, emission, duration, voice, sound) in
        [(1, 1, 4, 12, 44, 59, 60), (35, 2, 5, 24, 95, 182, 61)]
    {
        let binding = battle::martial::binding(&files, technique, technique)?;
        assert_eq!((binding.tp_cost, binding.duration), (cost, duration));
        let source = table.records[usize::from(technique)].as_ref().unwrap();
        assert_eq!(
            source.hits[source.phases[0].indices[1] as usize].start,
            emission
        );
        let mut bank = EffectResource {
            source: battle_effect::TECHNIQUES_PATH.into(),
            resource: 38,
            members: vec![],
            scene: None,
            models: Default::default(),
        };
        if technique == 35 {
            let weapon = model::weapon(&files, 159)?;
            bank.models
                .insert(0, model::effect(&files, &weapon.parts[&0], 10)?);
        }
        let (mut owner, owner_model) = model::party(&files, character, actor(), setup(1))?;
        let (mut target, target_model) =
            model::enemy(&files, &menu.monsters.records[49], 1, setup(2))?;
        // This timing witness keeps contacts out of reach; reaction integration
        // has separate fixtures. No stun resource is needed by these recipients.
        owner.reaction.stun.immune = true;
        target.reaction.stun.immune = true;
        target.position = [0., 0., 3000.];
        let mut resources = Original {
            files: &files,
            character,
            bank,
        };
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &[binding],
            vec![owner, target],
            1,
            &mut resources,
            vec![Some(owner_model), Some(target_model)],
        )?;
        let actors: Vec<_> = prepared.actor_ids().collect();
        let mut battle = Battle::new(prepared);
        let first = battle.step(BattleInput {
            actions: vec![ActionRequest {
                actor: actors[0],
                action: technique,
                target: actors[1],
            }],
            ..Default::default()
        })?;
        assert_eq!(first.actors[0].tp, 40 - cost);
        let action = first.actions[0].0;
        let mut emitted = Vec::new();
        let mut sounds = Vec::new();
        let mut voices = Vec::new();
        let mut saw_model = false;
        let mut completed = false;
        let mut recovery = 0;
        for _ in 0..180 {
            let before = battle.action_age(action);
            let frame = battle.step(BattleInput::default())?;
            for cue in &frame.cues {
                match cue {
                    Cue::ProjectileStarted { action: source, .. } if *source == action => {
                        emitted.push(before.unwrap())
                    }
                    Cue::Sound { sound, .. } => sounds.push((before, sound.index)),
                    Cue::Voice { sound, .. } => voices.push(sound.index),
                    Cue::Completed { action: source } if *source == action => completed = true,
                    _ => {}
                }
            }
            if frame.actors[0].activity == resonance_battle::Activity::Recovering {
                recovery += 1;
            }
            saw_model |= frame.particles.iter().any(|particle| {
                particle.member == 47
                    && particle
                        .model
                        .as_ref()
                        .is_some_and(|model| model.resource == 10 && model.clip.is_none())
            });
            assert_eq!(frame.actors[0].tp, 40 - cost);
        }
        assert_eq!(emitted, [emission as u32]);
        assert!(sounds.contains(&(Some(if technique == 1 { 4 } else { 20 }), sound)));
        assert!(voices.contains(&voice));
        assert_eq!(recovery, usize::from(source.phases[0].recovery_ticks) + 1);
        assert!(completed);
        assert_eq!(saw_model, technique == 35);
    }
    Ok(())
}
