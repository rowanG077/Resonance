use super::*;
use resonance_battle::{Activity, Cue, MotionBinding, Playback, SoundBinding};
use resonance_content::battle_model;

struct Original {
    groups: Vec<Vec<u16>>,
}

impl BattleResources for Original {
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        battle::enemy::zombie_melee(path, &self.groups)
    }
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        let clip = path
            .strip_prefix("battle/motions/enemies/036/")
            .context("unexpected enemy motion")?
            .parse()?;
        Ok(MotionBinding { model: 36, clip })
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        anyhow::ensure!(path == "battle/sounds/common/60", "unexpected enemy sound");
        Ok(SoundBinding {
            resource: 1,
            index: 60,
        })
    }
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("ordinary attack requested casting")
    }
    fn effect(&mut self, _: &str) -> Result<EffectResource> {
        bail!("ordinary attack requested an effect")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("ordinary attack requested a particle")
    }
    fn projectile(&mut self, _: &str) -> Result<ProjectileResource> {
        bail!("ordinary attack requested a projectile")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("ordinary attack requested a spell")
    }
}

fn setup(files: &Files, resource: u32) -> Result<battle::model::ModelSetup> {
    let common: resonance_content::battle_effect::SourceBank =
        files.json(resonance_content::battle_effect::COMMON_PATH)?;
    Ok(battle::model::ModelSetup {
        resource,
        initial: Playback {
            clip: 0,
            frame: 0.,
            rate: 0.5,
            repeat: true,
        },
        suppress_root_translation: [true; 3],
        stun: Some(battle::model::StunResources {
            particle: Arc::new(resonance_battle::ParticleDefinition {
                model: None,
                resource: 37,
                member: 19,
                data: common.particle(19)?,
            }),
            sound: SoundBinding {
                resource: 1,
                index: 117,
            },
        }),
    })
}

#[test]
#[ignore = "requires current enemy action and model publications; no devices"]
fn cold_zombie_rows_bind_contacts_and_run_maintained_source() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Enemy(36),
            battle::model::ModelSource::Party(1),
        ],
        &mut cache,
        || false,
    )?;
    assert_eq!(
        files.read("scripts/battle/enemy_zombie.sym")?.as_ref(),
        include_bytes!("../../../../scripts/battle/enemy_zombie.sym")
    );
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(36))?;
    assert_eq!(source.actions.rows.len(), 5);
    let groups: Vec<_> = (0..=source.body.attack_groups.keys().copied().max().unwrap_or(0))
        .map(|index| {
            source
                .body
                .attack_groups
                .get(&index)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    for (action, row, group, reaction) in [(0, 0, 1, 1), (1, 0, 0, 6), (2, 0, 1, 1), (2, 1, 0, 1)] {
        let request = MeleeResource {
            impact: None,
            source: battle_model::enemy_path(36),
            selection: battle::MeleeSelection::Enemy { action },
            row,
            anchor_groups: groups.clone(),
        };
        let contact = battle::melee::load(&files, &request)?;
        assert_eq!(contact.anchors, groups[group]);
        assert_eq!(
            (contact.radius, contact.height, contact.cooldown),
            (30., 30., 30)
        );
        let hit = &source.actions.hits
            [usize::from(source.actions.rows[usize::from(action)].hit) + usize::from(row)];
        assert_eq!(hit.reaction, reaction);
    }
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let bindings = battle::enemy::zombie_bindings(&files, [1, 2, 3, 4, 5])?;
    for (selection, expected_clips, expected_sounds) in [
        (0, vec![30, 31, 32, 0], 1),
        (1, vec![33, 34, 35, 0], 1),
        (2, vec![30, 31, 34, 35, 0], 2),
        (3, vec![30, 31, 34, 36, 31, 32, 0], 3),
        (4, vec![38, 0], 1),
    ] {
        let (mut owner, owner_model) =
            battle::model::enemy(&files, &menu.monsters.records[36], 0, setup(&files, 36)?)?;
        // Source route01 C338..340 retains walk speed1.3, then normal3DF34
        // sets braking0.41250002 before the first two movement visits.
        owner.movement.forward = 1.3;
        owner.movement.braking = 0.55;
        let initial_tp = owner.tp;
        let mut target = actor();
        target.position = [10000., 0., 0.];
        let (target, target_model) = battle::model::party(&files, 1, target, setup(&files, 1)?)?;
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &bindings,
            vec![owner, target],
            0x12345678,
            &mut Original {
                groups: groups.clone(),
            },
            vec![Some(owner_model), Some(target_model)],
        )?;
        let ids: Vec<_> = prepared.actor_ids().collect();
        if selection == 0 {
            let decision = battle::ai::definition(&files, 36, ids[0], &[1, 2, 3, 4, 5], 0)?;
            assert_eq!(
                decision
                    .choices
                    .iter()
                    .map(|row| row.guard_chance)
                    .collect::<Vec<_>>(),
                [0, 15, 33, 50, 0],
                "selected-row guard metadata must exist before any authored attack starts"
            );
        }
        let mut active = Battle::new(prepared.clone());
        let mut replay = Battle::new(prepared);
        let mut clips = vec![];
        let mut sounds = 0;
        let mut completed = false;
        // This is preparation/scheduling coverage, not an original frame oracle.
        // Multiple animation rows require separately verified source clocks.
        for update in 0..250 {
            if update == 20 {
                let before = replay.actors().to_vec();
                let paused = replay.step(BattleInput {
                    menu_open: true,
                    ..Default::default()
                })?;
                assert_eq!(paused.actors, before);
                assert!(paused.cues.is_empty());
            }
            let input = || BattleInput {
                actions: if update == 0 {
                    vec![ActionRequest {
                        actor: ids[0],
                        target: ids[1],
                        action: bindings[selection].id,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            let frame = active.step(input())?;
            let again = replay.step(input())?;
            assert_eq!(frame.actors, again.actors);
            assert_eq!(frame.models, again.models);
            assert_eq!(frame.actions, again.actions);
            assert_eq!(frame.cues, again.cues);
            assert_eq!(active.random_state(), replay.random_state());
            assert_eq!(frame.actors[0].tp, initial_tp);
            if update == 0 {
                assert_eq!(frame.actors[0].movement.braking.to_bits(), 0x3ed3_3334);
                assert_eq!(frame.actors[0].movement.forward, 1.3_f32 - 0.41250002);
            }
            if update == 1 {
                assert_eq!(frame.actors[0].movement.forward, 0.);
            }
            if let Activity::Action { guard_window, .. } = frame.actors[0].activity {
                assert_eq!(guard_window, [0, 12]);
                assert_eq!(
                    frame.actors[0].guard.enemy_chance,
                    source.actions.rows[selection].guard_chance as i8
                );
            }
            // Model evaluation precedes admission, so update0 still presents
            // the pre-action idle pose. The next sample shows the bound clip.
            if update != 0
                && let Some(model) = frame.models.first()
                && clips.last() != Some(&model.clip)
            {
                clips.push(model.clip);
            }
            for cue in &frame.cues {
                match cue {
                    Cue::Sound {
                        sound, priority, ..
                    } => {
                        assert_eq!((sound.index, *priority), (60, 1));
                        sounds += 1;
                    }
                    Cue::Completed { .. } => completed = true,
                    Cue::Rejected { .. } | Cue::Hit { .. } => panic!("unexpected cue {cue:?}"),
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(completed, "Zombie row {selection} did not recover");
        assert_eq!(clips, expected_clips);
        assert_eq!(sounds, expected_sounds);
        assert!(matches!(active.actors()[0].activity, Activity::Idle));
    }
    Ok(())
}

struct Ghost {
    groups: Vec<Vec<u16>>,
    birth: EffectResource,
    clash: EffectResource,
}

impl BattleResources for Ghost {
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        battle::enemy::ghost_melee(path, &self.groups)
    }
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        let clip = path
            .strip_prefix("battle/motions/enemies/049/")
            .context("unexpected Ghost motion")?
            .parse()?;
        Ok(MotionBinding { model: 49, clip })
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        let index = path
            .strip_prefix("battle/sounds/common/")
            .context("unexpected Ghost sound")?
            .parse()?;
        anyhow::ensure!(matches!(index, 60 | 61), "unexpected Ghost sound index");
        Ok(SoundBinding { resource: 1, index })
    }
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        battle::enemy::ghost_projectile(path, self.birth.clone(), self.clash.clone())
    }
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("Ghost requested casting")
    }
    fn effect(&mut self, _: &str) -> Result<EffectResource> {
        bail!("Ghost requested direct effect")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("Ghost requested direct particle")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("Ghost requested spell")
    }
}

#[test]
#[ignore = "requires current enemy action, local effect and carried model publications; no devices"]
fn cold_ghost_source_prepares_both_rows_and_spits_its_local_model_projectile() -> Result<()> {
    use resonance_content::battle_effect;
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Enemy(49),
            battle::model::ModelSource::Party(1),
        ],
        &mut cache,
        || false,
    )?;
    assert_eq!(
        files.read("scripts/battle/enemy_ghost.sym")?.as_ref(),
        include_bytes!("../../../../scripts/battle/enemy_ghost.sym")
    );
    let source: battle_model::Enemy = files.json(&battle_model::enemy_path(49))?;
    let bank: battle_effect::SourceBank = files.json(&battle_model::enemy_effects_path(49))?;
    let art = bank.art.as_ref().context("missing Ghost effect artwork")?;
    assert_eq!(source.actions.rows.len(), 2);
    assert_eq!(
        (source.profile.idle_ticks, source.profile.idle_variation),
        (75, 25)
    );
    assert_eq!(art.models.len(), 1);
    assert!(art.textures.contains_key(&2));
    let effect_model = battle::model::effect(&files, &art.models[&0], 490)?;
    let birth = EffectResource {
        source: battle_model::enemy_effects_path(49),
        resource: 49,
        members: vec![1],
        scene: None,
        models: [(0, effect_model)].into(),
    };
    let clash = EffectResource {
        source: battle_effect::COMMON_PATH.into(),
        resource: 0,
        members: vec![11],
        scene: None,
        models: Default::default(),
    };
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let bindings = battle::enemy::ghost_bindings(&files, [1, 2])?;
    for selection in 0..2 {
        let (owner, mut owner_model) =
            battle::model::enemy(&files, &menu.monsters.records[49], 1, setup(&files, 49)?)?;
        let mut groups: Vec<_> = (0..12)
            .map(|slot| {
                source
                    .body
                    .attack_groups
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        let part = source
            .attachments
            .get(&0)
            .context("missing Ghost carried model")?;
        let appended = battle::weapon::attach(
            &files,
            Arc::make_mut(&mut owner_model),
            part,
            0,
            source.body.attachments[&0],
            491,
            resonance_battle::WeaponPlayback::Rigid,
        )?;
        for (slot, anchors) in appended.into_iter().enumerate() {
            groups[slot].extend(anchors);
        }
        let mut target = actor();
        target.position = [10000., 0., 0.];
        let (target, target_model) = battle::model::party(&files, 1, target, setup(&files, 1)?)?;
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &bindings,
            vec![owner, target],
            0x12345678,
            &mut Ghost {
                groups,
                birth: birth.clone(),
                clash: clash.clone(),
            },
            vec![Some(owner_model), Some(target_model)],
        )?;
        let ids: Vec<_> = prepared.actor_ids().collect();
        let mut active = Battle::new(prepared);
        let mut sound = None;
        let mut projectile_seen = false;
        let mut hidden = false;
        let mut shown_again = false;
        let mut complete = false;
        for update in 0..200 {
            let frame = active.step(BattleInput {
                actions: if update == 0 {
                    vec![ActionRequest {
                        actor: ids[0],
                        target: ids[1],
                        action: bindings[selection].id,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            })?;
            for cue in &frame.cues {
                match cue {
                    Cue::Sound {
                        sound: emitted,
                        priority,
                        ..
                    } => {
                        assert_eq!(*priority, 1);
                        sound = Some(emitted.index);
                    }
                    Cue::Completed { .. } => complete = true,
                    Cue::Rejected { .. } | Cue::Hit { .. } => {
                        panic!("unexpected Ghost cue {cue:?}")
                    }
                    _ => {}
                }
            }
            projectile_seen |= !frame.projectiles.is_empty();
            let carried = frame
                .weapons
                .iter()
                .find(|weapon| weapon.owner == ids[0] && weapon.slot == 0)
                .context("missing Ghost carried pose")?;
            hidden |= !carried.visible;
            shown_again |= hidden && carried.visible;
            if complete {
                break;
            }
        }
        assert!(complete, "Ghost row did not recover");
        assert_eq!(sound, Some(if selection == 0 { 60 } else { 61 }));
        assert_eq!(projectile_seen, selection == 1);
        assert_eq!(
            (hidden, shown_again),
            if selection == 0 {
                (false, false)
            } else {
                (true, true)
            }
        );
    }
    Ok(())
}
