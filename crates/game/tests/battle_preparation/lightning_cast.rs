use super::*;
use resonance_battle::{ActionPhase, Control, Cue, ModelDefinition, Playback};
use resonance_content::{battle_action, battle_effect, battle_projectile};

// These tests bind prepared presentation IDs without opening render/audio devices.
struct Original<'a>(Resources, &'a Files);

impl BattleResources for Original<'_> {
    fn casting(&mut self, path: &str) -> Result<battle::casting::CastingResource> {
        self.0.casting(path)
    }
    fn voice(&mut self, path: &str) -> Result<Vec<Option<resonance_battle::VoiceLine>>> {
        use battle::{
            model::ModelSource,
            voice::{Phase, Sound},
        };
        let phase = match path {
            "battle/voices/techniques/genis/216/chant" => Phase::Chant,
            "battle/voices/techniques/genis/216/fallback" => Phase::Fallback,
            "battle/voices/techniques/genis/216/release" => Phase::Release,
            _ => bail!("unexpected Lightning voice resource {path}"),
        };
        battle::voice::technique(
            self.1,
            &[
                ModelSource::Party(3),
                ModelSource::Enemy(49),
                ModelSource::Enemy(49),
            ],
            216,
            phase,
            |sound| {
                Ok(match sound {
                    Sound::Cue(index) => resonance_battle::SoundBinding { resource: 1, index },
                    Sound::Stream(index) => resonance_battle::SoundBinding { resource: 2, index },
                })
            },
        )
    }
    fn sound(&mut self, path: &str) -> Result<resonance_battle::SoundBinding> {
        self.0.paths.push(path.into());
        Ok(resonance_battle::SoundBinding {
            resource: 1,
            index: path.rsplit('/').next().context("sound index")?.parse()?,
        })
    }
    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        self.0.effect(path)
    }
    fn particle(&mut self, path: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        self.0.particle(path)
    }
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        self.0.melee(path)
    }
    fn motion(&mut self, path: &str) -> Result<resonance_battle::MotionBinding> {
        self.0.motion(path)
    }
    fn spell(&mut self, path: &str) -> Result<u16> {
        self.0.spell(path)
    }
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        assert_eq!(path, "battle/projectiles/techniques/4");
        self.0.paths.push(path.into());
        Ok(ProjectileResource {
            impact: None,
            trail: None,
            ground: None,
            source: battle_projectile::PATH.into(),
            member: 4,
            hit: HitResource {
                source: battle_action::SPELL_PATH.into(),
                selection: battle::HitSelection::Technique {
                    member: 16,
                    phase: 0,
                },
                rule: 0,
            },
            birth: Some(EffectResource {
                models: Default::default(),
                scene: None,
                source: battle_effect::TECHNIQUES_PATH.into(),
                resource: 38,
                members: vec![28],
            }),
            clash: None,
        })
    }
}

fn prepare(
    files: &Files,
    model: Arc<ModelDefinition>,
    owner: Actor,
) -> Result<(Battle, ActionRequest)> {
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let spells: battle_action::Table = files.json(battle_action::SPELL_PATH)?;
    let duration = spells.records[16]
        .as_ref()
        .context("Lightning action")?
        .phases[0]
        .duration;
    assert_eq!(duration, 90);
    let mut enemies = vec![];
    let mut models = vec![Some(model)];
    for (index, x) in [200., 205.].into_iter().enumerate() {
        let (mut enemy, model) = battle::model::enemy(
            files,
            &menu.monsters.records[49],
            1,
            setup(files, index as u32 + 2)?,
        )?;
        enemy.position = [x, 0., 0.];
        enemies.push(enemy);
        models.push(Some(model));
    }
    let mut resources = Original(
        Resources {
            paths: vec![],
            fail: false,
        },
        files,
    );
    let prepared = battle::prepare(
        &mut PreparationCache::default(),
        files,
        &[
            ActionBinding {
                id: 1,
                phase: ActionPhase::Casting,
                module: "battle::genis_lightning".into(),
                entry: "run".into(),
                duration: 0,
                tp_cost: 9,
            },
            ActionBinding {
                id: 100,
                phase: ActionPhase::Resident,
                module: "battle::lightning".into(),
                entry: "release".into(),
                duration,
                tp_cost: 0,
            },
        ],
        std::iter::once(owner).chain(enemies).collect(),
        1,
        &mut resources,
        models,
    )?;
    assert!(
        resources
            .0
            .paths
            .iter()
            .any(|p| p == "battle/sounds/common/92")
    );
    let ids: Vec<_> = prepared.actor_ids().collect();
    Ok((
        Battle::new(prepared),
        ActionRequest {
            actor: ids[0],
            target: ids[1],
            action: 1,
        },
    ))
}

pub(super) fn setup(files: &Files, resource: u32) -> Result<battle::model::ModelSetup> {
    let bank: battle_effect::SourceBank = files.json(battle_effect::COMMON_PATH)?;
    Ok(battle::model::ModelSetup {
        resource,
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        suppress_root_translation: [false; 3],
        stun: Some(battle::model::StunResources {
            particle: Arc::new(resonance_battle::ParticleDefinition {
                model: None,
                resource: 37,
                member: 19,
                data: bank.particle(19)?,
            }),
            sound: resonance_battle::SoundBinding {
                resource: 1,
                index: 117,
            },
        }),
    })
}

#[test]
#[ignore = "requires current Genis model, casting and Lightning publications; no devices"]
fn cold_genis_lightning_runs_cast_release_contacts_and_surviving_effects() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Party(3),
            battle::model::ModelSource::Enemy(49),
        ],
        &mut cache,
        || false,
    )?;
    for (name, maintained) in [
        (
            "genis_lightning",
            include_bytes!("../../../../scripts/battle/genis_lightning.sym").as_slice(),
        ),
        (
            "lightning",
            include_bytes!("../../../../scripts/battle/lightning.sym").as_slice(),
        ),
        (
            "casting",
            include_bytes!("../../../../scripts/battle/casting.sym").as_slice(),
        ),
    ] {
        assert_eq!(
            files.read(&format!("scripts/battle/{name}.sym"))?.as_ref(),
            maintained
        );
    }
    let mut owner = actor();
    owner.control = Control::Auto;
    owner.stats.intelligence = 100;
    let (owner, model) = battle::model::party(&files, 3, owner, setup(&files, 1)?)?;
    // Interrupted casting pays no cost. Interruption after release leaves the
    // resident, its projectile and the independent effect alive.
    for interruption in [None, Some(10), Some(160)] {
        let (mut active, action) = prepare(&files, model.clone(), owner.clone())?;
        let mut frame = active.step(BattleInput {
            actions: vec![action],
            ..Default::default()
        })?;
        let casting = frame.actions[0].0;
        let mut releases = vec![];
        let mut resident = None;
        let mut bolts = vec![];
        let mut hits = vec![];
        let mut sounds = vec![];
        let mut voices = vec![];
        let mut payment = None;
        let mut particle_tail = false;
        for update in 1..=240 {
            frame = active.step(BattleInput {
                interrupt: if interruption == Some(update) {
                    vec![casting]
                } else {
                    vec![]
                },
                ..Default::default()
            })?;
            if interruption == Some(update) {
                assert!(frame.cues.contains(&Cue::Interrupted { action: casting }));
            }
            if frame.actors[0].tp != 40 {
                assert_eq!(frame.actors[0].tp, 31);
                payment.get_or_insert(update);
            }
            for cue in &frame.cues {
                match cue {
                    Cue::Released { action, .. } => {
                        releases.push(update);
                        resident = Some(*action);
                    }
                    Cue::ProjectileStarted { .. } => bolts.push(update),
                    Cue::Hit { actor, result, .. } => {
                        assert!(result.amount > 0);
                        hits.push((update, actor.index()));
                    }
                    Cue::Sound { sound, .. } if sound.index == 92 => sounds.push(update),
                    Cue::Voice { actor, sound, .. } => {
                        assert_eq!(actor.index(), 0);
                        voices.push((update, sound.index));
                    }
                    _ => {}
                }
            }
            if frame.projectiles.is_empty() && frame.particles.iter().any(|p| p.resource == 38) {
                particle_tail = true;
            }
            if let Some(resident) = resident {
                // Script completion after emission does not free the spell slot.
                // The original action descriptor retains it through callback age 90.
                assert_eq!(active.action_age(resident).is_some(), update < 233);
                if update == 233 {
                    assert!(frame.cues.contains(&Cue::Completed { action: resident }));
                }
            }
        }
        if interruption == Some(10) {
            assert!(
                releases.is_empty() && bolts.is_empty() && hits.is_empty() && sounds.is_empty()
            );
            assert_eq!(payment, None);
            assert!(voices.is_empty());
        } else {
            assert_eq!(payment, Some(141));
            assert_eq!(releases, [142]);
            assert_eq!(bolts, [164]);
            assert_eq!(sounds, bolts);
            assert_eq!(voices, [(84, 248), (141, 312)]);
            // Birth performs age-zero motion without contact (14C24). Each
            // submitted volume selects one eligible recipient in roster order.
            assert_eq!(hits, [(165, 1), (166, 2)]);
            assert!(particle_tail);
        }
        assert!(frame.projectiles.is_empty());
        assert!(frame.particles.iter().all(|p| p.resource != 38));
    }
    let mut poor = owner;
    poor.tp = 8;
    let (mut active, action) = prepare(&files, model, poor)?;
    let frame = active.step(BattleInput {
        actions: vec![action],
        ..Default::default()
    })?;
    assert_eq!(frame.actors[0].tp, 8);
    assert!(frame.actions.is_empty());
    assert_eq!(frame.cues.len(), 1);
    assert!(matches!(frame.cues[0], Cue::Rejected { .. }));
    Ok(())
}
