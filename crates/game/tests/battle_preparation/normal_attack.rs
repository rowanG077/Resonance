use super::*;
use resonance_battle::{
    Activity, CombatStats, ContactSource, Control, Cue, GuardResult, MotionBinding, Playback,
    PreparedBattle, SoundBinding, VoiceLine,
};
use resonance_content::{battle_action, battle_model};

struct Original<'a> {
    files: &'a Files,
    groups: Vec<Vec<u16>>,
}

impl BattleResources for Original<'_> {
    fn voice(&mut self, path: &str) -> Result<Vec<Option<VoiceLine>>> {
        use battle::{model::ModelSource, voice::Sound};
        let line = match path {
            "battle/voices/absolute/1" => 1,
            "battle/voices/absolute/2" => 2,
            "battle/voices/absolute/3" => 3,
            _ => bail!("unexpected normal voice {path}"),
        };
        battle::voice::absolute(
            self.files,
            &[ModelSource::Party(1), ModelSource::Enemy(49)],
            line,
            |sound| {
                Ok(match sound {
                    Sound::Cue(index) => SoundBinding { resource: 1, index },
                    Sound::Stream(index) => SoundBinding { resource: 2, index },
                })
            },
        )
    }

    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        battle::normal::lloyd_melee(path, &self.groups)
    }
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        let clip = match path {
            "battle/motions/lloyd/30" => 30,
            "battle/motions/lloyd/31" => 31,
            "battle/motions/lloyd/32" => 32,
            "battle/motions/lloyd/33" => 33,
            "battle/motions/lloyd/38" => 38,
            "battle/motions/lloyd/40" => 40,
            "battle/motions/lloyd/41" => 41,
            "battle/motions/lloyd/16" => 16,
            _ => bail!("unexpected normal motion {path}"),
        };
        Ok(MotionBinding { model: 1, clip })
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        assert_eq!(path, "battle/sounds/common/60");
        Ok(SoundBinding {
            resource: 1,
            index: 60,
        })
    }
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("normal attack requested casting")
    }
    fn effect(&mut self, _: &str) -> Result<EffectResource> {
        bail!("normal attack requested an effect")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("normal attack requested a particle")
    }
    fn projectile(&mut self, _: &str) -> Result<ProjectileResource> {
        bail!("normal attack requested a projectile")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("normal attack requested a spell")
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

fn stats(row: &serde_json::Value) -> CombatStats {
    let s = row["stats"].as_array().unwrap();
    CombatStats {
        slash: s[0].as_i64().unwrap() as i16,
        thrust: s[1].as_i64().unwrap() as i16,
        defense: s[2].as_i64().unwrap() as i16,
        intelligence: s[3].as_i64().unwrap() as i16,
        accuracy: s[4].as_i64().unwrap() as i16,
        evasion: s[5].as_i64().unwrap() as i16,
        level: s[6].as_u64().unwrap() as u8,
    }
}

fn prepare(
    files: &Files,
    observed: &serde_json::Value,
    selection: u8,
    hit_stop: u8,
) -> Result<Arc<PreparedBattle>> {
    let binding = battle::normal::lloyd_bindings(files, [1; 7])?.remove(usize::from(selection));
    prepare_actions(files, observed, &[binding], hit_stop, selection == 4)
}

fn prepare_actions(
    files: &Files,
    observed: &serde_json::Value,
    bindings: &[ActionBinding],
    hit_stop: u8,
    armor: bool,
) -> Result<Arc<PreparedBattle>> {
    let mut owner = actor();
    let source = &observed["owner"];
    owner.stats = stats(source);
    owner.hp = source["hp"].as_i64().unwrap() as i32;
    owner.max_hp = source["max_hp"].as_i64().unwrap() as i32;
    owner.tp = source["tp"].as_u64().unwrap() as u16;
    owner.max_tp = source["max_tp"].as_u64().unwrap() as u16;
    owner.luck = source["luck"].as_u64().unwrap() as u8;
    owner.control = Control::SemiAuto;
    owner.movement.direction = [0., 0., 1.];
    owner.movement.braking = 0.41250002;
    owner.movement.gravity = -1.;
    owner.hit_stop = hit_stop;
    let (owner, mut model) = battle::model::party(files, 1, owner, setup(1))?;
    let body: battle_model::Party = files.json(&battle_model::party_path(1))?;
    let weapon = battle::model::weapon(files, 135)?;
    let mut groups = vec![];
    for (&slot, part) in &weapon.parts {
        groups.extend(battle::model::rigid_weapon(
            Arc::make_mut(&mut model),
            part,
            body.body.attachments[&slot],
        )?);
    }
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2]);
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let (mut target, enemy_model) =
        battle::model::enemy(files, &menu.monsters.records[49], 1, setup(2))?;
    assert_eq!(target.stats, stats(&observed["target"]));
    target.hp = observed["target"]["hp"].as_i64().unwrap() as i32;
    // Stationary placement is an integration case, not the original AI replay.
    // Its action is outside the observed [1,5) automatic-guard window.
    target.position = [0., 30., 60.];
    target.activity = Activity::Action {
        clock: 0,
        guard_window: [1, 4],
    };
    if armor {
        // Keep both original sword windows in range while exercising their
        // shared per-target cache and rearm, without an enemy decision loop.
        target.reaction.armor.threshold = 2;
    }
    battle::prepare(
        &mut PreparationCache::default(),
        files,
        bindings,
        vec![owner, target],
        observed["random_before"].as_u64().unwrap() as u32,
        &mut Original { files, groups },
        vec![Some(model), Some(enemy_model)],
    )
}

#[test]
#[ignore = "requires current Lloyd, weapon and normal-action publications; no devices"]
fn cold_lloyd_normals_run_verified_motion_weapon_contacts_and_recovery() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Party(1),
            battle::model::ModelSource::Weapon(135),
            battle::model::ModelSource::Enemy(49),
        ],
        &mut cache,
        || false,
    )?;
    assert_eq!(
        files.read("scripts/battle/normal_lloyd.sym")?.as_ref(),
        include_bytes!("../../../../scripts/battle/normal_lloyd.sym")
    );
    let original: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-contact-tp.json"
    ))?;
    let observed = &original["observations"][4];
    assert_eq!(observed["combat_tick"], 421);
    let stun: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-stun-rolls.json"
    ))?;
    let stun = stun["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["combat_tick"] == 421)
        .unwrap();
    for (selection, duration, recovery, hits_at, sounds_at) in [
        (0, 30, 10, vec![12], vec![12]),
        (4, 40, 8, vec![14, 22], vec![12, 16]),
    ] {
        for stop in [0, 3] {
            let prepared = prepare(&files, observed, selection, stop)?;
            let ids: Vec<_> = prepared.actor_ids().collect();
            let mut active = Battle::new(prepared.clone());
            let mut replay = Battle::new(prepared);
            let mut hits = vec![];
            let mut sounds = vec![];
            let mut voices = vec![];
            let end = u32::from(stop) + 4 + duration + recovery + 1;
            for update in 0..=end {
                if update == 6 {
                    let before = replay.actors().to_vec();
                    let random = replay.random_state();
                    for _ in 0..3 {
                        let held = replay.step(BattleInput {
                            menu_open: true,
                            ..Default::default()
                        })?;
                        assert_eq!(held.actors, before);
                        assert!(held.cues.is_empty());
                        assert_eq!(replay.random_state(), random);
                    }
                }
                let input = || BattleInput {
                    actions: if update == 0 {
                        vec![ActionRequest {
                            actor: ids[0],
                            target: ids[1],
                            action: 1,
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
                for cue in &frame.cues {
                    match cue {
                        Cue::Hit {
                            source,
                            actor,
                            result,
                            ..
                        } => {
                            assert_eq!(*actor, ids[1]);
                            assert!(
                                matches!(source, ContactSource::Melee { actor, .. } if *actor == ids[0])
                            );
                            assert_eq!(result.guard, GuardResult::None);
                            assert_eq!(result.armored, selection == 4);
                            assert!(result.amount > 0);
                            hits.push(update - u32::from(stop));
                            if selection == 0 {
                                // This contact uses the captured damage/TP inputs;
                                // the geometry and action timeline above are native.
                                assert_eq!(
                                    result.amount,
                                    observed["amount"].as_i64().unwrap() as i32
                                );
                                assert_eq!(
                                    frame.actors[1].hp,
                                    observed["hp_after"].as_i64().unwrap() as i32
                                );
                                assert_eq!(
                                    frame.actors[0].tp,
                                    observed["tp_after"].as_u64().unwrap() as u16
                                );
                                assert_eq!(
                                    active.random_state(),
                                    stun["random_after"].as_u64().unwrap() as u32
                                );
                            }
                        }
                        Cue::Sound {
                            sound, priority, ..
                        } => {
                            assert_eq!((sound.index, *priority), (60, 1));
                            sounds.push(update - u32::from(stop));
                        }
                        Cue::Voice { actor, sound, .. } => {
                            assert_eq!(*actor, ids[0]);
                            voices.push((update - u32::from(stop), sound.index));
                        }
                        _ => {}
                    }
                }
                assert_eq!(frame.actions.is_empty(), update == end);
                assert_eq!(frame.actors[0].tp, 32 + hits.len() as u16);
                assert!(frame.outcome.is_none());
            }
            assert_eq!(hits, hits_at, "selection {selection}, stop {stop}");
            assert_eq!(sounds, sounds_at);
            assert_eq!(voices, [(8, if selection == 0 { 502 } else { 504 })]);
            assert_eq!(active.actors()[0].activity, Activity::Idle);
        }
        for interruption in [6, 10] {
            let prepared = prepare(&files, observed, selection, 0)?;
            let ids: Vec<_> = prepared.actor_ids().collect();
            let mut active = Battle::new(prepared);
            let first = active.step(BattleInput {
                actions: vec![ActionRequest {
                    actor: ids[0],
                    target: ids[1],
                    action: 1,
                }],
                ..Default::default()
            })?;
            let action = first.actions[0].0;
            let mut voices = vec![];
            for update in 1..60 {
                let frame = active.step(BattleInput {
                    interrupt: if update == interruption {
                        vec![action]
                    } else {
                        vec![]
                    },
                    ..Default::default()
                })?;
                assert!(
                    !frame
                        .cues
                        .iter()
                        .any(|cue| matches!(cue, Cue::Hit { .. } | Cue::Sound { .. }))
                );
                for cue in &frame.cues {
                    if let Cue::Voice { sound, .. } = cue {
                        voices.push((update, sound.index));
                    }
                    assert!(!matches!(cue, Cue::VoiceStopped { .. }));
                }
                if update >= interruption {
                    assert!(frame.actions.is_empty());
                }
            }
            assert_eq!(
                voices,
                if interruption == 6 {
                    vec![]
                } else {
                    vec![(8, if selection == 0 { 502 } else { 504 })]
                }
            );
            assert_eq!(active.actors()[0].tp, 32);
            assert_eq!(active.actors()[1].hp, 160);
            assert_eq!(
                active.random_state(),
                observed["random_before"].as_u64().unwrap() as u32
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires current Lloyd, weapon and normal-action publications; no devices"]
fn all_lloyd_selectors_share_one_preparation_and_match_source_commands_and_motions() -> Result<()> {
    use resonance_content::battle_action::CommandRecord;
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Party(1),
            battle::model::ModelSource::Weapon(135),
            battle::model::ModelSource::Enemy(49),
        ],
        &mut cache,
        || false,
    )?;
    let original: serde_json::Value = serde_json::from_str(include_str!(
        "../../../battle/tests/fixtures/opening-contact-tp.json"
    ))?;
    let bindings = battle::normal::lloyd_bindings(&files, [1, 2, 3, 4, 5, 6, 7])?;
    let prepared = prepare_actions(&files, &original["observations"][4], &bindings, 0, true)?;
    let ids: Vec<_> = prepared.actor_ids().collect();
    let table: battle_action::NormalTable = files.json(battle_action::NORMAL_PATH)?;
    let group = &table.groups[0];
    for (selection, binding) in bindings.iter().enumerate() {
        let source = &group.actions[usize::from(group.selectors[selection].action)];
        let descriptor = &group.descriptors[source.descriptor as usize];
        let mut expected_playback = Vec::new();
        for command in group
            .commands
            .iter()
            .skip_while(|row| row.word_index < source.command)
        {
            match &command.record {
                CommandRecord::End => break,
                CommandRecord::Command {
                    time,
                    opcode: opcode @ (27 | 28),
                    operands,
                } => {
                    expected_playback.push((*time as u32, *opcode == 27, operands[0]));
                }
                _ => {}
            }
        }
        // 2C8B0 command 28 plays through 9E38 immediately; command 27 only
        // queues 71E78's pending actor voice. Common actor update 2503C then
        // dispatches it through 71674 after the command stream. Rising has
        // both at age 2, so its sound precedes voice playback despite the
        // opposite command-table order. Keep the order within each phase.
        expected_playback.sort_by_key(|&(age, voice, _)| (age, voice));
        let expected_clips: Vec<_> = group.animations[source.animation as usize..]
            .iter()
            .take_while(|row| row.time >= 0)
            .map(|row| u16::from(row.clip))
            .collect();
        let mut active = Battle::new(prepared.clone());
        let mut playback = Vec::new();
        let mut clips = Vec::new();
        let mut age = 0;
        let mut recovery_updates = 0;
        let mut complete = false;
        for update in 0..128 {
            let frame = active.step(BattleInput {
                actions: if update == 0 {
                    vec![ActionRequest {
                        actor: ids[0],
                        target: ids[1],
                        action: binding.id,
                    }]
                } else {
                    vec![]
                },
                ..Default::default()
            })?;
            for cue in &frame.cues {
                match cue {
                    Cue::Sound {
                        sound, priority, ..
                    } => {
                        assert_eq!(*priority, 1);
                        playback.push((age, false, sound.index));
                    }
                    Cue::Voice { sound, .. } => playback.push((age, true, sound.index - 501)),
                    Cue::Rejected { .. } => panic!("rejected normal selector {selection}"),
                    _ => {}
                }
            }
            let clip = frame.models[0].clip;
            if clip != 0 && clips.last() != Some(&clip) {
                clips.push(clip);
            }
            if frame.actors[0].activity == Activity::Recovering {
                recovery_updates += 1;
                assert_eq!(frame.actions[0].2, u32::from(descriptor.duration));
            }
            if frame.actions.is_empty() {
                complete = true;
                break;
            }
            age = frame.actions[0].2;
        }
        assert!(complete, "normal selector {selection} did not finish");
        assert_eq!(
            playback, expected_playback,
            "normal selector {selection} playback dispatch"
        );
        assert_eq!(
            clips, expected_clips,
            "normal selector {selection} animation stream"
        );
        assert_eq!(recovery_updates, descriptor.recovery_ticks + 1);
        assert_eq!(active.actors()[0].activity, Activity::Idle);
    }
    Ok(())
}
