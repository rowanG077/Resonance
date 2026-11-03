//! Added native Unison319–322 callbacks, lowered into ordinary shared events.
use crate::{battle_scenes, battle_voices, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_normal::Event,
    battle_unison::{Callback, Combination, Execution, Origin, PairVoice, Role, RolePlacement},
};

pub(super) fn read(
    source: &Source,
    image: &pe::Image<'_>,
    combination: &Combination,
) -> Result<Option<Execution>> {
    let native = combination.native_id;
    if !(319..=322).contains(&native) {
        return Ok(None);
    }
    ensure!(
        pe::half(
            image.slice(0x8de450 + u32::from(combination.id) * 64, 64)?,
            4
        )? == native,
        "added Unison row/native identity changed"
    );
    verify(image, native)?;
    let scalar = |at| -> Result<f32> {
        let value = f32::from_bits(pe::word(image.slice(at, 4)?, 0)?);
        ensure!(value.is_finite(), "non-finite Unison placement");
        Ok(value)
    };
    let angle = |at| -> Result<f32> {
        let value = f64::from_le_bytes(image.slice(at, 8)?.try_into()?) as f32;
        ensure!(value.is_finite(), "non-finite Unison angle");
        Ok(value)
    };
    let mut roles = match native {
        319 => [
            role(&[("presea", 0)], 0., scalar(0x882d94)?),
            role(&[("sheena", 1)], angle(0x884078)?, scalar(0x882d94)?),
        ],
        320 => [
            role(&[("presea", 0)], 0., scalar(0x882d94)?),
            role(
                &[("genis", 1), ("zelos", 1), ("kratos", 1)],
                angle(0x884088)?,
                scalar(0x7a36d4)?,
            ),
        ],
        // Both source branches select a phase using role0's character. The
        // three source tracks are equal after following their local offsets;
        // validate that fact before choosing the common phase0 for either role.
        321 => {
            equivalent_rock_tracks(source, image)?;
            let phases = &[("genis", 0), ("zelos", 0), ("kratos", 0)];
            [
                role(phases, 0., scalar(0x884084)?),
                role(phases, angle(0x884070)?, scalar(0x884084)?),
            ]
        }
        322 => [
            role(&[("regal", 0)], 0., scalar(0x882d94)?),
            role(&[("sheena", 1)], angle(0x884070)?, scalar(0x882d94)?),
        ],
        _ => unreachable!(),
    };
    source_tracks(source, image, native, &roles)?;
    let mut voices = Vec::new();
    match native {
        319 => {
            roles[0].events.extend([
                (0, effect(4, Origin::TargetAim, true)),
                (52, effect(3, Origin::TargetAim, false)),
            ]);
            // Native319 has two named combinations sharing the same scene.
            roles[1].events.push((
                0,
                voice(
                    image,
                    if combination.id == 20 {
                        0x422acf
                    } else {
                        0x422af4
                    },
                )?,
            ));
        }
        320 => {
            roles[0].events.extend([
                (0, voice(image, 0x422d09)?),
                (230, projectile(1, 0, Origin::TargetRoot)),
            ]);
            // Bank0/member7 is resident artwork, not local scene member7.
            roles[1].events.push((
                210,
                Callback::CommonEffect {
                    member: 7,
                    origin: Origin::ActorAim,
                    scale: None,
                    follow: true,
                },
            ));
        }
        321 => {
            let points = battle_scenes::unison_points(image, native)?;
            ensure!(points.len() == 10, "Rock Pillar strike-point count changed");
            for (index, offset) in points.into_iter().enumerate() {
                let origin = Origin::TargetRootOffset {
                    offset,
                    rotated: true,
                };
                roles[0].events.extend([
                    (30 + 12 * index as u16, effect(2, origin, false)),
                    (60 + 12 * index as u16, effect(3, origin, false)),
                    (60 + 12 * index as u16, projectile(1, 0, origin)),
                ]);
            }
            roles[0].events.extend([
                (170, effect(4, Origin::TargetRoot, false)),
                (200, effect(5, Origin::TargetRoot, false)),
                (200, projectile(2, 1, Origin::TargetRoot)),
            ]);
            // The actor occupying role0 speaks the line for this unordered pair.
            for (first, second, operand) in [
                ("genis", "zelos", 0x42310e),
                ("genis", "kratos", 0x4230e9),
                ("zelos", "kratos", 0x4230ae),
            ] {
                let sound = battle_voices::sound(pe::word(image.slice(operand, 4)?, 0)?)?
                    .context("Rock Pillar pair voice is disabled")?;
                for characters in [
                    [first.to_owned(), second.to_owned()],
                    [second.to_owned(), first.to_owned()],
                ] {
                    voices.push(PairVoice {
                        characters,
                        speaker: 0,
                        sound,
                    });
                }
            }
        }
        322 => {
            roles[0]
                .events
                .push((0, effect(6, Origin::TargetAim, true)));
            roles[1].events.extend([
                (
                    0,
                    voice(
                        image,
                        if combination.id == 24 {
                            0x4239ba
                        } else {
                            0x4239df
                        },
                    )?,
                ),
                (18, projectile(1, 0, Origin::TargetAim)),
            ]);
        }
        _ => unreachable!(),
    }
    let (at, duration, limit) = match native {
        319 => (50, 60, 12),
        320 => (218, 86, 8),
        321 => (190, 100, 12),
        322 => (60, 90, 8),
        _ => unreachable!(),
    };
    // Native feedback compares every participating actor's callback clock.
    for role in &mut roles {
        role.events
            .push((at, Callback::Feedback { duration, limit }));
        role.events.sort_by_key(|(at, _)| *at);
    }
    let color_at = match native {
        319 => 0x42292e,
        320 => 0x422c48,
        321 => 0x422f8d,
        322 => 0x423809,
        _ => unreachable!(),
    };
    let execution = Execution {
        roles,
        color: Some(image.slice(color_at, 3)?.try_into()?),
        voices,
    };
    execution.validate()?;
    Ok(Some(execution))
}

fn role(phases: &[(&str, u8)], angle: f32, distance: f32) -> Role {
    let (sin, cos) = angle.sin_cos();
    Role {
        phases: phases
            .iter()
            .map(|&(name, variant)| (name.to_owned(), variant))
            .collect(),
        placement: Some(RolePlacement {
            direction: [cos, 0., sin],
            distance,
        }),
        visible: true,
        temporary_weapons: false,
        events: Vec::new(),
    }
}
fn effect(member: u16, origin: Origin, follow: bool) -> Callback {
    Callback::Effect {
        member,
        origin,
        scale: Some(1.),
        follow,
    }
}
fn projectile(member: u8, hit_variant: u8, origin: Origin) -> Callback {
    Callback::Projectile {
        member,
        hit_variant,
        origin,
        heading: None,
        height: None,
    }
}
fn voice(image: &pe::Image<'_>, operand: u32) -> Result<Callback> {
    Ok(Callback::Ordinary(Event::Voice(battle_voices::sound(
        pe::word(image.slice(operand, 4)?, 0)?,
    )?)))
}
fn package_parameters(source: &Source, image: &pe::Image<'_>, native: u16) -> Result<Vec<u8>> {
    let name = image.text(pe::word(
        image.slice(0x8e98a8 + u32::from(native) * 4, 4)?,
        0,
    )?)?;
    let package = source.require(name)?;
    Ok(battle_scenes::member(&package, 0x190)?
        .context("missing added combination participant tracks")?
        .to_vec())
}
fn source_tracks(
    source: &Source,
    image: &pe::Image<'_>,
    native: u16,
    roles: &[Role; 2],
) -> Result<()> {
    let parameters = package_parameters(source, image, native)?;
    for variant in roles.iter().flat_map(|role| role.phases.values()) {
        ensure!(
            pe::half(&parameters, 16 + usize::from(*variant) * 28)? > 0,
            "combination{native} selects empty participant track{variant}"
        );
    }
    Ok(())
}
fn equivalent_rock_tracks(source: &Source, image: &pe::Image<'_>) -> Result<()> {
    let parameters = package_parameters(source, image, 321)?;
    equivalent_rock_parameters(&parameters)
}
fn equivalent_rock_parameters(parameters: &[u8]) -> Result<()> {
    let base = &parameters[16..44];
    let contacts = pe::word(&parameters, 4)? as usize;
    let poses = pe::word(&parameters, 8)? as usize;
    let commands = pe::word(&parameters, 12)? as usize;
    let first_pose = poses + pe::word(base, 20)? as usize * 12;
    let expected_pose = parameters
        .get(first_pose..first_pose + 12)
        .context("missing common Rock Pillar cast pose")?;
    for index in 0..3 {
        let row = parameters
            .get(16 + index * 28..44 + index * 28)
            .context("missing Rock Pillar variant")?;
        let pose = poses + pe::word(row, 20)? as usize * 12;
        ensure!(
            row[..16] == base[..16]
                && parameters.get(pose..pose + 12) == Some(expected_pose)
                && pe::half(&parameters, pose + 12)? == 0xfffe
                && pe::half(&parameters, contacts + pe::word(row, 16)? as usize * 32)? == 0xffff
                && pe::half(&parameters, commands + pe::word(row, 24)? as usize * 2)? == 0xffff,
            "Rock Pillar participant tracks no longer equivalent"
        );
    }
    Ok(())
}

fn verify(image: &pe::Image<'_>, native: u16) -> Result<()> {
    let (initializer, callback_write, callback) = match native {
        319 => (0x422900, 0x422b56, 0x422b70),
        320 => (0x422c20, 0x422df4, 0x422e00),
        321 => (0x422f60, 0x42326c, 0x423280),
        322 => (0x4237e0, 0x423a41, 0x423a50),
        _ => unreachable!(),
    };
    let descriptor = pe::word(image.slice(0x8e9688 + u32::from(native) * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == initializer
            && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x427f20
            && image.slice(callback_write, 3)? == [0xc7, 0x43, 0x18]
            && pe::word(image.slice(callback_write + 3, 4)?, 0)? == callback,
        "combination{native} callback producer changed"
    );
    let guards: &[(u32, &[u8])] = match native {
        319 => &[
            (0x4229b7, &[0x32, 0xd2]),
            (0x422a92, &[0xb2, 0x01]),
            (0x4229df, &[0x8d, 0x81, 0x38, 0x39, 0x01, 0x00]),
            (0x4229f7, &[0x6a, 0x04]),
            (0x422aab, &[0x80, 0x3e, 0x14, 0x75]),
            (
                0x422ac9,
                &[0xc7, 0x83, 0x94, 0x3a, 0x01, 0x00, 0xf9, 0x04, 0x00, 0x00],
            ),
            (
                0x422aee,
                &[0xc7, 0x83, 0x94, 0x3a, 0x01, 0x00, 0xfa, 0x04, 0x00, 0x00],
            ),
            (0x422b77, &[0x66, 0x83, 0xb8, 0xbe, 0x01, 0x00, 0x00, 0x32]),
            (0x422b8e, &[0xba, 0x3c, 0x00, 0x00, 0x00]),
            (0x422b9a, &[0xc6, 0x81, 0x27, 0x9a, 0x00, 0x00, 0x0c]),
            (0x422bb1, &[0x80, 0xf9, 0x07]),
            (0x422bb6, &[0x66, 0x83, 0xb8, 0xbe, 0x01, 0x00, 0x00, 0x34]),
            (0x422bf0, &[0x6a, 0x03]),
        ],
        320 => &[
            (0x422cd1, &[0x32, 0xd2]),
            (0x422de1, &[0xb2, 0x01]),
            (
                0x422d03,
                &[0xc7, 0x83, 0x94, 0x3a, 0x01, 0x00, 0x21, 0x05, 0x00, 0x00],
            ),
            (0x422e10, &[0xb9, 0xda, 0x00, 0x00, 0x00]),
            (0x422e30, &[0xb9, 0x56, 0x00, 0x00, 0x00]),
            (0x422e3c, &[0xc6, 0x82, 0x27, 0x9a, 0x00, 0x00, 0x08]),
            (0x422e53, &[0x80, 0xf9, 0x07]),
            (0x422e71, &[0xbe, 0xe6, 0x00, 0x00, 0x00]),
            (0x422e94, &[0x8b, 0xbe, 0x9c, 0x38, 0x01, 0x00]),
            (0x422ef7, &[0xb9, 0xd2, 0x00, 0x00, 0x00]),
            (0x422f32, &[0x6a, 0x07, 0x6a, 0x00]),
        ],
        321 => &[
            (0x423027, &[0x3c, 0x03]),
            (0x42302f, &[0x3c, 0x06]),
            (0x42303a, &[0x3c, 0x09]),
            (0x4231e7, &[0x8b, 0x86, 0xe0, 0x92, 0x00, 0x00]),
            (0x4231f8, &[0x3c, 0x03]),
            (0x423200, &[0x3c, 0x06]),
            (0x42320b, &[0x3c, 0x09]),
            (0x4232c4, &[0xb9, 0xbe, 0x00, 0x00, 0x00]),
            (0x423397, &[0xba, 0x64, 0x00, 0x00, 0x00]),
            (0x4233a3, &[0xc6, 0x80, 0x27, 0x9a, 0x00, 0x00, 0x0c]),
            (0x4233b1, &[0x3b, 0xb0, 0xe0, 0x92, 0x00, 0x00]),
            (0x4233c4, &[0x66, 0x83, 0xf8, 0x1e]),
            (0x4233ce, &[0xb9, 0x96, 0x00, 0x00, 0x00]),
            (0x4233e1, &[0xb9, 0x0c, 0x00, 0x00, 0x00]),
            (0x4233fc, &[0x8b, 0x91, 0x9c, 0x38, 0x01, 0x00]),
            (0x4234b7, &[0x6a, 0x02]),
            (0x4234e0, &[0x66, 0x83, 0xf8, 0x3c]),
            (0x4234ea, &[0xb9, 0xb4, 0x00, 0x00, 0x00]),
            (0x4234fd, &[0xb9, 0x0c, 0x00, 0x00, 0x00]),
            (0x4235d8, &[0x6a, 0x03]),
            (0x42363a, &[0x8d, 0x82, 0x90, 0x01, 0x00, 0x00]),
            (0x423662, &[0xba, 0xaa, 0x00, 0x00, 0x00]),
            (0x4236a1, &[0x6a, 0x04]),
            (0x4236cd, &[0xb9, 0xc8, 0x00, 0x00, 0x00]),
            (0x42372a, &[0x6a, 0x05]),
            (0x423796, &[0x8d, 0x82, 0x20, 0x03, 0x00, 0x00]),
            (0x4237ac, &[0x83, 0xc1, 0x1c]),
        ],
        322 => &[
            (0x423892, &[0x32, 0xd2]),
            (0x423979, &[0xb2, 0x01]),
            (0x4238c6, &[0x8d, 0x81, 0x38, 0x39, 0x01, 0x00]),
            (0x4238de, &[0x6a, 0x06]),
            (0x423992, &[0x80, 0xbe, 0xf0, 0x92, 0x00, 0x00, 0x18]),
            (
                0x4239b4,
                &[0xc7, 0x83, 0x94, 0x3a, 0x01, 0x00, 0xf9, 0x04, 0x00, 0x00],
            ),
            (
                0x4239d9,
                &[0xc7, 0x83, 0x94, 0x3a, 0x01, 0x00, 0xfa, 0x04, 0x00, 0x00],
            ),
            (0x423a57, &[0x66, 0x83, 0xb8, 0xbe, 0x01, 0x00, 0x00, 0x3c]),
            (0x423a7a, &[0xb9, 0x5a, 0x00, 0x00, 0x00]),
            (0x423a86, &[0xc6, 0x82, 0x27, 0x9a, 0x00, 0x00, 0x08]),
            (0x423a9d, &[0x80, 0xf9, 0x05]),
            (0x423aa2, &[0x66, 0x83, 0xb8, 0xbe, 0x01, 0x00, 0x00, 0x12]),
            (0x423ac1, &[0x8b, 0xbe, 0x38, 0x39, 0x01, 0x00]),
            (0x423af3, &[0x8d, 0x82, 0x90, 0x01, 0x00, 0x00]),
        ],
        _ => unreachable!(),
    };
    for &(address, expected) in guards {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "combination{native} callback changed at{address:#x}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combination(id: u8, native_id: u16) -> Combination {
        Combination {
            id,
            name: "source test row".into(),
            native_id,
            participants: 2,
            recipes: Vec::new(),
            duration: 1,
            camera_pitch: 0.,
            execution: None,
        }
    }

    #[test]
    #[ignore = "requires native Steam; reads only Unison319–322 callbacks/packages"]
    fn installed_lower_unison_keeps_alias_voices_origins_and_all_pillar_hits() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        for (first, second, native) in [(20, 21, 319), (24, 25, 322)] {
            let a = read(&source, &image, &combination(first, native))?.unwrap();
            let b = read(&source, &image, &combination(second, native))?.unwrap();
            assert_ne!(
                a.roles[1].events, b.roles[1].events,
                "shared scene must retain its two source notice/voice variants"
            );
            assert!(a.roles[0].events.iter().any(|(at, event)| *at == 0
                && matches!(
                    event,
                    Callback::Effect {
                        origin: Origin::TargetAim,
                        follow: true,
                        ..
                    }
                )));
        }
        let critical = read(&source, &image, &combination(22, 320))?.unwrap();
        assert!(critical.roles[1].events.iter().any(|(at, event)| *at == 210
            && matches!(
                event,
                Callback::CommonEffect {
                    member: 7,
                    origin: Origin::ActorAim,
                    ..
                }
            )));
        assert!(critical.roles[0].events.iter().any(|(at, event)| *at == 230
            && matches!(
                event,
                Callback::Projectile {
                    member: 1,
                    hit_variant: 0,
                    origin: Origin::TargetRoot,
                    ..
                }
            )));
        let rock = read(&source, &image, &combination(23, 321))?.unwrap();
        let projectile_ticks = rock.roles[0]
            .events
            .iter()
            .filter_map(|(at, event)| matches!(event, Callback::Projectile { .. }).then_some(*at))
            .collect::<Vec<_>>();
        assert_eq!(
            projectile_ticks,
            (0..10)
                .map(|i| 60 + i * 12)
                .chain([200])
                .collect::<Vec<_>>()
        );
        assert_eq!(rock.voices.len(), 6);
        assert!(rock.voices.iter().all(|voice| voice.speaker == 0));
        assert!(
            rock.roles
                .iter()
                .flat_map(|role| role.phases.values())
                .all(|phase| *phase == 0)
        );
        assert!(rock.roles[0].events.iter().any(|(at, event)| *at == 30
            && matches!(
                event,
                Callback::Effect {
                    origin: Origin::TargetRootOffset {
                        offset: [-150., 0., 0.],
                        rotated: true
                    },
                    member: 2,
                    ..
                }
            )));
        // A changed native cast variant must stop the phase0 equivalence lowering.
        let mut parameters = package_parameters(&source, &image, 321)?;
        let second = 44;
        let pose =
            pe::word(&parameters, 8)? as usize + pe::word(&parameters, second + 20)? as usize * 12;
        parameters[pose + 2] ^= 1;
        assert!(equivalent_rock_parameters(&parameters).is_err());
        Ok(())
    }
}
