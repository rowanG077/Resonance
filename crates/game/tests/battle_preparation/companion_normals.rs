use super::*;
use resonance_battle::{Activity, Cue, MotionBinding, Playback, SoundBinding, VoiceLine};
use resonance_content::{
    battle_action::{self, CommandRecord},
    battle_model,
};

struct Resources {
    name: &'static str,
    character: u8,
    groups: Vec<Vec<u16>>,
}

impl BattleResources for Resources {
    fn voice(&mut self, path: &str) -> Result<Vec<Option<VoiceLine>>> {
        let index = path
            .strip_prefix("battle/voices/absolute/")
            .context("unexpected voice path")?
            .parse()?;
        Ok(vec![
            Some(VoiceLine {
                sound: SoundBinding { resource: 2, index },
                duration: 1,
            }),
            None,
        ])
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        let index = path
            .strip_prefix("battle/sounds/common/")
            .context("unexpected sound path")?
            .parse()?;
        Ok(SoundBinding { resource: 1, index })
    }
    fn motion(&mut self, path: &str) -> Result<MotionBinding> {
        let clip = path
            .strip_prefix(&format!("battle/motions/{}/", self.name))
            .context("unexpected motion path")?
            .parse()?;
        Ok(MotionBinding { model: 1, clip })
    }
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        match self.character {
            2 => battle::normal::colette_melee(path, &self.groups),
            3 => battle::normal::genis_melee(path, &self.groups),
            _ => unreachable!(),
        }
    }
    fn weapon_flight(&mut self, path: &str) -> Result<battle::WeaponFlightResource> {
        battle::normal::colette_flight(path)
    }
    fn casting(&mut self, _: &str) -> Result<battle::casting::CastingResource> {
        bail!("unexpected casting")
    }
    fn effect(&mut self, _: &str) -> Result<EffectResource> {
        bail!("unexpected effect")
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("unexpected particle")
    }
    fn projectile(&mut self, _: &str) -> Result<ProjectileResource> {
        bail!("unexpected projectile")
    }
    fn spell(&mut self, _: &str) -> Result<u16> {
        bail!("unexpected spell")
    }
}

#[test]
#[ignore = "requires current Colette, Genis, weapon and normal publications; no devices"]
fn companion_selectors_execute_original_commands_motions_and_recovery() -> Result<()> {
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[
            battle::model::ModelSource::Party(2),
            battle::model::ModelSource::Party(3),
            battle::model::ModelSource::Enemy(36),
            battle::model::ModelSource::Weapon(159),
            battle::model::ModelSource::Weapon(175),
        ],
        &mut cache,
        || false,
    )?;
    let table: battle_action::NormalTable = files.json(battle_action::NORMAL_PATH)?;
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    assert_eq!(
        table.source_sha256,
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    for (character, name, item, source) in [
        (
            2,
            "colette",
            159,
            include_bytes!("../../../../scripts/battle/normal_colette.sym").as_slice(),
        ),
        (
            3,
            "genis",
            175,
            include_bytes!("../../../../scripts/battle/normal_genis.sym").as_slice(),
        ),
    ] {
        assert_eq!(
            files
                .read(&format!("scripts/battle/normal_{name}.sym"))?
                .as_ref(),
            source
        );
        let mut owner = actor();
        owner.movement.direction = [0., 0., 1.];
        owner.movement.gravity = -1.;
        let (owner, mut model) = battle::model::party(
            &files,
            character,
            owner,
            battle::model::ModelSetup {
                resource: 1,
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
        let body: battle_model::Party = files.json(&battle_model::party_path(character))?;
        let weapon = battle::model::weapon(&files, item)?;
        let mut groups = vec![];
        for (&slot, part) in &weapon.parts {
            groups.extend(battle::weapon::attach(
                &files,
                Arc::make_mut(&mut model),
                part,
                slot,
                body.body.attachments[&slot],
                10 + u32::from(slot),
                if character == 3 {
                    battle::weapon::owner_linked()
                } else {
                    resonance_battle::WeaponPlayback::Rigid
                },
            )?);
        }
        let bindings = if character == 2 {
            battle::normal::colette_bindings(&files, [1, 2, 3, 4, 5, 6, 7])?
        } else {
            battle::normal::genis_bindings(&files, [1, 2, 3, 4, 5, 6, 7])?
        };
        // Both original companion hit rules carry stagger 2. Even this distant
        // timing recipient needs the real knockdown/recovery model resources.
        let (mut target, target_model) = battle::model::enemy(
            &files,
            &menu.monsters.records[36],
            0,
            battle::model::ModelSetup {
                resource: 2,
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
        assert_eq!(
            target_model
                .knockdown
                .map(|binding| (binding.down_motion, binding.recovery_motion)),
            Some((7, Some(9)))
        );
        target.position = [10000., 0., 0.];
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &bindings,
            vec![owner, target],
            0,
            &mut Resources {
                name,
                character,
                groups,
            },
            vec![Some(model), Some(target_model)],
        )?;
        let ids: Vec<_> = prepared.actor_ids().collect();
        let group = &table.groups[usize::from(character - 1)];
        for (selection, binding) in bindings.iter().enumerate() {
            let action = &group.actions[usize::from(group.selectors[selection].action)];
            let descriptor = &group.descriptors[action.descriptor as usize];
            let mut expected_audio = vec![];
            for command in group
                .commands
                .iter()
                .skip_while(|row| row.word_index < action.command)
            {
                match &command.record {
                    CommandRecord::End => break,
                    CommandRecord::Command {
                        time,
                        opcode: opcode @ (27 | 28),
                        operands,
                    } => {
                        expected_audio.push((*time as u32, *opcode, operands[0]));
                    }
                    _ => {}
                }
            }
            // Shared 2C8B0 dispatch: command 28 plays immediately; command 27
            // queues its voice until common update 2503C calls 71674.
            expected_audio.sort_by_key(|&(age, opcode, _)| (age, opcode == 27));
            let mut expected_clips: Vec<_> = group.animations[action.animation as usize..]
                .iter()
                .take_while(|row| row.time >= 0)
                .map(|row| u16::from(row.clip))
                .collect();
            if descriptor.recovery_clip != 0 {
                expected_clips.push(u16::from(descriptor.recovery_clip));
            }
            let mut active = Battle::new(prepared.clone());
            let (mut audio, mut clips) = (vec![], vec![]);
            let (mut age, mut recovery, mut completed) = (0, 0, false);
            for update in 0..180 {
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
                        Cue::Voice { sound, .. } => audio.push((age, 27, sound.index)),
                        Cue::Sound { sound, .. } => audio.push((age, 28, sound.index)),
                        Cue::Rejected { .. } => panic!("{name} selector {selection} rejected"),
                        _ => {}
                    }
                }
                let clip = frame.models[0].clip;
                if clip != 0 && clips.last() != Some(&clip) {
                    clips.push(clip);
                }
                if frame.actors[0].activity == Activity::Recovering {
                    recovery += 1;
                }
                if frame.actions.is_empty() {
                    completed = true;
                    break;
                }
                age = frame.actions[0].2;
            }
            assert!(completed, "{name} selector {selection} did not finish");
            assert_eq!(
                audio, expected_audio,
                "{name} selector {selection} command stream"
            );
            assert_eq!(
                clips, expected_clips,
                "{name} selector {selection} motion stream"
            );
            assert_eq!(
                recovery,
                descriptor.recovery_ticks + 1,
                "{name} selector {selection} recovery"
            );
        }
    }
    Ok(())
}
