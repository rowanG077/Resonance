use super::*;

fn empty_rel() -> Rel {
    Rel {
        bytes: vec![],
        sections: vec![],
        pointers: Default::default(),
        local_targets: Default::default(),
    }
}

#[test]
#[ignore = "reads original disc 1 and model/audio records from RESONANCE_WORLD_ASSETS without publication"]
fn original_cinematic_tracks_bind_prepared_models_and_audio() -> Result<()> {
    let output = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_WORLD_ASSETS").context("set RESONANCE_WORLD_ASSETS")?,
    );
    let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/extracted/disc1/files/US_r_Top2field.rel");
    let (scenes, _) = read(&Rel::read(&original)?)?;
    // This read-only preflight uses only unchanged model records. Full acceptance
    // separately decodes the complete current schema after cook-all.
    let package: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join(PACKAGE_PATH))?)?;
    let mut audio: resonance_content::field_audio::FieldAudio =
        serde_json::from_slice(&std::fs::read(output.join("worlds/audio.json"))?)?;
    audio.validate()?;
    for (id, scene) in &scenes {
        let actors: BTreeMap<u8, Vec<resonance_content::ScenePart>> = serde_json::from_value(
            package["visuals"]["cinematics"][id.to_string()]["actors"].clone(),
        )?;
        scene.presentation.validate(&actors)?;
        scene.presentation.validate_audio(&audio)?;
        for line in &scene.dialogue {
            assert!(audio.voices.contains_key(&line.voice));
        }
        if let Some(voices) = &scene.voices {
            let files = original.parent().unwrap();
            let source = crate::field_resources::resolve_path(files, &voices.source)?;
            let bytes = crate::compression::payload(std::fs::read(files.join(source))?)?;
            let members = crate::field::sections(&bytes)?;
            let archive = &bytes[members[voices.member]
                .clone()
                .context("missing voice archive")?];
            let entries = crate::field::sections(archive)?;
            for (index, range) in entries.iter().enumerate() {
                let encoded = &archive[range.clone().context("missing source voice")?];
                let id = voices.first_resource + u32::try_from(index)?;
                assert_eq!(
                    audio.voices[&id].asset.path,
                    format!("audio/voices/{}.wav", crate::digest(encoded))
                );
            }
            assert!(
                scene
                    .dialogue
                    .iter()
                    .all(|line| line.voice >= voices.first_resource
                        && line.voice < voices.first_resource + entries.len() as u32)
            );
        }
        let mut missing = actors.clone();
        missing.remove(&1);
        assert!(scene.presentation.validate(&missing).is_err());
    }
    audio.sounds.remove(&24);
    assert!(scenes[&516].presentation.validate_audio(&audio).is_err());
    Ok(())
}

#[test]
fn translated_tracks_support_other_periods_delays_and_held_endpoints() -> Result<()> {
    let rel = empty_rel();
    let track = UvRecipe {
        texture: 3,
        clock: CinematicClock::Scene,
        delay_ticks: 2,
        motion: UvMotion::Scroll { axis: 0, period: 7 },
    }
    .prepare(&rel, &BTreeMap::new())?;
    assert_eq!(track.animation.texture, 3);
    assert_eq!(track.offset(2, 100), [0., 0.]);
    assert_eq!(track.offset(8, 100), [6. / 7., 0.]);
    assert_eq!(track.offset(9, 100), [0., 0.]);
    let track = UvRecipe {
        texture: 0,
        clock: CinematicClock::Scene,
        delay_ticks: 1,
        motion: UvMotion::Linear {
            axis: 1,
            hold: 5,
            pieces: vec![
                LinearPiece {
                    start: 0,
                    rate: Scalar::Value(0.25),
                    offset: Scalar::Value(0.),
                },
                LinearPiece {
                    start: 3,
                    rate: Scalar::Value(-0.5),
                    offset: Scalar::Value(1.),
                },
            ],
        },
    }
    .prepare(&rel, &BTreeMap::new())?;
    assert_eq!(
        [0, 1, 3, 4, 5, 6, 50].map(|tick| track.offset(tick, 0)[1]),
        [0., 0., 0.5, 1., 0.5, 0., 0.]
    );
    assert!(
        UvRecipe {
            texture: 0,
            clock: CinematicClock::Scene,
            delay_ticks: 0,
            motion: UvMotion::Scroll { axis: 1, period: 0 }
        }
        .prepare(&rel, &BTreeMap::new())
        .is_err()
    );
    assert!(
        UvRecipe {
            texture: 0,
            clock: CinematicClock::Scene,
            delay_ticks: 0,
            motion: UvMotion::Atlas {
                axis: 1,
                sequence: "missing".into(),
                window_table: None
            }
        }
        .prepare(&rel, &BTreeMap::new())
        .is_err()
    );
    Ok(())
}

#[test]
#[ignore = "reads both original field RELs; does not publish assets"]
fn original_cinematic_presentation_tables_and_native_controllers() -> Result<()> {
    let mut previous = None;
    for disc in ["disc1", "disc2"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../local/extracted/{disc}/files/US_r_Top2field.rel"
        ));
        let rel = Rel::read(&path)?;
        let (scenes, sounds) = read(&rel)?;
        assert_eq!(
            sounds,
            [
                24, 25, 26, 133, 160, 177, 182, 183, 193, 217, 282, 437, 438, 443, 445
            ]
        );
        let value = serde_json::to_value(
            scenes
                .iter()
                .map(|(id, s)| (*id, (&s.presentation, &s.dialogue)))
                .collect::<BTreeMap<_, _>>(),
        )?;
        if let Some(previous) = &previous {
            assert_eq!(&value, previous);
        }
        previous = Some(value);
        let dialogue = &scenes[&517].dialogue;
        assert_eq!(
            dialogue
                .iter()
                .map(|line| (line.tick, line.duration, line.speaker))
                .collect::<Vec<_>>(),
            [
                (540, 120, 1),
                (680, 120, 5),
                (820, 140, 4),
                (980, 240, 5),
                (1240, 120, 5),
                (1380, 80, 3),
                (1480, 120, 5),
                (1620, 60, 1),
                (1740, 60, 3)
            ]
        );
        assert_eq!(scenes[&517].voices.as_ref().unwrap().first_resource, 917923);
        assert_eq!(
            dialogue.iter().map(|line| line.voice).collect::<Vec<_>>(),
            (917923..=917931).collect::<Vec<_>>()
        );
        assert!(
            scenes
                .iter()
                .filter(|(id, _)| **id != 517)
                .all(|(_, s)| s.dialogue.is_empty() && s.voices.is_none())
        );
        // Compare the cooked events with each original sound row, including duplicates.
        for (scene, table, count) in [
            (516, 0x344, 5),
            (517, 0x358, 7),
            (518, 0x374, 4),
            (519, 0x384, 2),
            (520, 0x38c, 3),
            (521, 0x398, 1),
            (522, 0x39c, 5),
            (523, 0x39c, 5),
        ] {
            let bytes = rel.at((5, table))?;
            let mut expected = (0..count)
                .map(|i| {
                    Ok((
                        u32::try_from(i16::read(bytes, i * 4)?)?,
                        i16::read(bytes, i * 4 + 2)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            expected.sort_by_key(|&(tick, _)| tick);
            let actual = scenes[&scene]
                .presentation
                .audio
                .iter()
                .filter(|e| !e.before_dialogue)
                .map(|e| (e.tick, e.action.sound().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "scene {scene}");
        }
        assert_eq!(
            scenes[&519]
                .presentation
                .audio
                .iter()
                .map(|e| (e.tick, e.action.sound()))
                .collect::<Vec<_>>(),
            [(50, Some(282)), (50, Some(282))]
        );
        let audio = &scenes[&517].presentation.audio;
        assert!(matches!(
            audio.iter().find(|e| e.tick == 1620).unwrap(),
            CinematicAudioEvent {
                before_dialogue: true,
                action: CinematicAudio::Sound { id: 437, .. },
                ..
            }
        ));
        // Independently evaluate the native atlas controllers against extracted ranges.
        for scene in 522..=524 {
            for actor in 1..=7 {
                let (table, stride, index) = if actor <= 3 {
                    (0x27c, 3, actor - 1)
                } else {
                    (0x2c4, 4, actor - 4)
                };
                let bytes = rel.at((5, table + ((scene - 522) * stride + index) as usize * 8))?;
                let begin = u32::read(bytes, 0)?;
                let end = u32::read(bytes, 4)?;
                let uv = &scenes[&scene].presentation.actors[&(actor as u8)].uv[0];
                for tick in 0..=400 {
                    let expected = if tick < begin || tick > end {
                        0.
                    } else if actor <= 3 {
                        ((tick - begin + 1) / 6 + 1) as f32 / 8.
                    } else {
                        let phase = (tick - begin) / 2;
                        (if phase < 3 {
                            1
                        } else if phase < 9 {
                            2
                        } else if phase < 16 {
                            3
                        } else if phase < 24 {
                            4
                        } else if phase < 29 {
                            3
                        } else if phase < 34 {
                            2
                        } else if phase < 43 {
                            1
                        } else {
                            0
                        }) as f32
                            / 4.
                    };
                    assert_eq!(
                        uv.offset(tick, 137)[1],
                        expected,
                        "scene {scene} actor {actor} tick {tick}"
                    );
                }
            }
        }
        let ship = &scenes[&516].presentation.actors[&9];
        let constant = |offset| f32::read(rel.at((5, offset))?, 0);
        let fast = constant(0xa7c)?;
        let slow = constant(0xa84)?;
        for tick in 0..=600u32 {
            let age = tick.saturating_sub(1).min(240);
            let expected = if age < 60 {
                fast * age as f32
            } else if age < 210 {
                slow.mul_add((age - 60) as f32, constant(0xa80)?)
            } else {
                fast.mul_add((age - 210) as f32, constant(0xa34)?)
            };
            assert_eq!(
                ship.uv[0].offset(tick, 999)[1],
                expected,
                "ship tick {tick}"
            );
            assert_eq!(
                ship.uv[1].offset(tick, 999)[1],
                (1. / constant(0xc70)?) * tick.saturating_sub(1).min(40) as f32
            );
            assert_eq!(
                scenes[&513].presentation.actors[&1].uv[0].offset(tick, 999)[1],
                ((tick % 32) >> 3) as f32 / 4.
            );
        }
        for (scene, actor, axis, period) in [
            (516, 5, 1, 128),
            (517, 8, 1, 128),
            (518, 6, 1, 128),
            (521, 2, 0, 512),
            (525, 1, 1, 32),
        ] {
            let uv = &scenes[&scene].presentation.actors[&actor].uv[0];
            for clock in [0, 1, 31, 32, 127, 128, 511, 512, 1023] {
                assert_eq!(
                    uv.offset(17, clock)[axis],
                    (clock % period) as f32 / period as f32
                );
            }
        }
        for actor in 1..=8 {
            let bytes = rel.at((5, 0x324 + (actor - 1) * 4))?;
            let begin = u32::from(u16::read(bytes, 0)?);
            let end = u32::from(u16::read(bytes, 2)?);
            let opacity = scenes[&516].presentation.actors[&(actor as u8)]
                .opacity
                .unwrap();
            for tick in 0..=260 {
                let expected = if tick >= end {
                    0
                } else if tick > begin {
                    ((end - 1 - tick) * 255 / (end - begin)) as u8
                } else {
                    255
                };
                assert_eq!(opacity.alpha(tick), expected);
            }
        }
    }
    Ok(())
}
