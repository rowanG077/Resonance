use super::*;
use crate::{
    battle::{actions::member, all::physical_ranges, pose},
    compression,
    field::sections,
    read::{u16 as half, u32 as word},
};

#[test]
#[ignore = "requires both original discs and all-assets; validates shared effects without conversion"]
fn original_common_and_enemy_effect_library_binds_all_254_records() -> Result<()> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = project.join("local/all-assets");
    let effects = BattleEffectPrograms {
        programs: vec![],
        actors: vec![],
        materials: vec![],
    };
    for disc in 1..=2 {
        let extracted = project.join(format!("local/extracted/disc{disc}"));
        let sources = Sources::read(&extracted)?;
        let files = extracted.join("files");
        let usual = fs::read(files.join(&sources.usual))?;
        let enemy = fs::read(files.join(&sources.enemy))?;
        let mut expected = BTreeMap::new();
        for (index, range) in sections(member(&usual, 6)?)?.iter().enumerate() {
            if range.is_some() {
                let index = u8::try_from(index + 1)?;
                expected.insert(
                    ModelRef::Common { index },
                    (format!("common-model-{index}"), 1, None),
                );
            }
        }
        let offsets = member(&usual, 10)?
            .chunks_exact(4)
            .map(|row| word(row, 0))
            .collect::<Result<Vec<_>>>()?;
        for (monster, range) in physical_ranges(&offsets, 0, enemy.len() as u64)? {
            let monster = u8::try_from(monster)?;
            let bytes = compression::decode(&enemy[range])?;
            ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
            let count = bytes[usize::from(half(&bytes, 4)?) + 0x1e8];
            ensure!(count <= 6, "enemy effect count exceeds pointer slots");
            for index in 0..6 {
                let model = word(&bytes, 0x180 + usize::from(index) * 4)? as usize;
                if model == 0 {
                    continue;
                }
                ensure!(index < count, "authored model exceeds declared enemy count");
                let parts = 1 + usize::from(word(&bytes, 0x198 + usize::from(index) * 4)? != 0);
                let animation = |slot| -> Result<Option<f32>> {
                    let offset = word(&bytes, 0x1b0 + usize::from(slot) * 4)? as usize;
                    (offset != 0)
                        .then(|| {
                            pose::motion(&bytes[offset..], &bytes[model..]).map(|motion| {
                                motion.duration_frames / resonance_content::battle::pose::FRAME_HZ
                            })
                        })
                        .transpose()
                };
                let name = format!("enemy-{monster}-model-{index}");
                expected.insert(
                    ModelRef::Enemy { monster, index },
                    (name.clone(), parts, animation(index)?),
                );
                for animation_model in 0..count {
                    if let Some(duration) = animation(animation_model)? {
                        expected.insert(
                            ModelRef::EnemyAnimated {
                                monster,
                                index,
                                animation_model,
                            },
                            (
                                format!("{name}-animation-{animation_model}"),
                                parts,
                                Some(duration),
                            ),
                        );
                    }
                }
            }
        }
        let bound: BTreeMap<_, _> = models(
            &root,
            disc,
            &sources,
            expected.keys().copied().chain([ModelRef::ColetteWeapon]),
            &effects,
        )?
        .into_iter()
        .map(|model| (model.binding, model))
        .collect();
        assert_eq!(bound.len(), 254, "disc {disc}");
        let common = Directory::open(&root, disc, &sources.usual)?;
        let enemy = Directory::open(&root, disc, &sources.enemy)?;
        let mut counts = [0; 4];
        for (&binding, (name, parts, duration)) in &expected {
            let model = &bound[&binding];
            let directory = if matches!(binding, ModelRef::Common { .. }) {
                &common
            } else {
                &enemy
            };
            super::super::tests::assert_bound(
                directory,
                name,
                &Visual::EffectModel(model.clone()),
            )?;
            assert_eq!(model.model.parts.len(), *parts);
            for part in &model.model.parts {
                let clips: Vec<_> = part
                    .scene
                    .clips
                    .iter()
                    .map(|clip| (clip.resource_slot, clip.duration_seconds))
                    .collect();
                assert_eq!(
                    clips,
                    duration
                        .map(|duration| (0, duration))
                        .into_iter()
                        .collect::<Vec<_>>(),
                    "{name}"
                );
            }
            match binding {
                ModelRef::Common { .. } => counts[0] += 1,
                ModelRef::Enemy { .. } => counts[1] += 1,
                ModelRef::EnemyAnimated {
                    monster,
                    index,
                    animation_model,
                } => {
                    counts[2 + usize::from(index != animation_model)] += 1;
                    if index == animation_model {
                        assert_eq!(
                            serde_json::to_value(&model.model)?,
                            serde_json::to_value(
                                &bound[&ModelRef::Enemy { monster, index }].model
                            )?,
                            "same-slot animation must share its base geometry and clip files"
                        );
                    }
                }
                _ => unreachable!(),
            }
        }
        assert_eq!(counts, [13, 144, 38, 59]);
        for index in 0..3 {
            let inherited = &bound[&ModelRef::EnemyAnimated {
                monster: 183,
                index,
                animation_model: 2,
            }];
            assert_eq!(
                inherited.model.parts[0].scene.clips[0].duration_seconds,
                0.8
            );
            if index != 2 {
                assert_ne!(
                    inherited.model.parts[0].scene.mesh,
                    bound[&ModelRef::Enemy {
                        monster: 183,
                        index
                    }]
                        .model
                        .parts[0]
                        .scene
                        .mesh,
                    "Amphitra geometry changes must preserve the template's animation"
                );
            }
        }
        assert!(
            models(
                &root,
                disc,
                &sources,
                [ModelRef::Common { index: 0 }],
                &effects
            )
            .is_err()
        );
    }
    Ok(())
}
