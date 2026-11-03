//! Native Wyrm Wrath through Purgatory Seal, expressed as shared choreography.
use crate::{battle_scenes, battle_voices, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_normal::Event,
    battle_unison::{Callback, Execution, Origin, PairVoice, Role, RolePlacement},
};

pub(super) fn read(
    source: &Source,
    image: &pe::Image<'_>,
    native: u16,
) -> Result<Option<Execution>> {
    if !(323..=326).contains(&native) {
        return Ok(None);
    }
    verify(image, native)?;
    let scalar = |at| -> Result<f32> { Ok(f32::from_bits(pe::word(image.slice(at, 4)?, 0)?)) };
    let angle =
        |at| -> Result<f32> { Ok(f64::from_le_bytes(image.slice(at, 8)?.try_into()?) as f32) };
    let mut roles = match native {
        323 => [
            role(&[("lloyd", 0)], 0., scalar(0x884080)?),
            role(&[("regal", 1)], 0., scalar(0x8835c0)?),
        ],
        324 => [
            role(
                &[("colette", 1), ("zelos", 1), ("kratos", 1)],
                angle(0x884070)?,
                scalar(0x882d94)?,
            ),
            role(&[("raine", 0)], 0., scalar(0x882d94)?),
        ],
        325 => [
            role(
                &[("lloyd", 0), ("zelos", 1), ("kratos", 2)],
                0.,
                scalar(0x882d94)?,
            ),
            role(&[("raine", 3)], angle(0x884078)?, scalar(0x882d94)?),
        ],
        326 => [
            role(&[("sheena", 0)], angle(0x884070)?, scalar(0x7a36d4)?),
            role(&[("raine", 1)], 0., scalar(0x7a36d4)?),
        ],
        _ => unreachable!(),
    };
    source_tracks(source, image, native, &roles)?;
    let mut voices = Vec::new();
    match native {
        323 => {
            roles[0].events.push((0, voice(image, 0x423c19)?));
            roles[1].events.push((
                0,
                Callback::PositionFromTarget {
                    height: scalar(0x8838a4)?,
                    retreat: scalar(0x8835c0)?,
                },
            ));
            roles[0]
                .events
                .push((100, projectile(2, 1, Origin::TargetRoot)));
            roles[1]
                .events
                .push((24, projectile(1, 0, Origin::TargetRoot)));
        }
        324 => {
            roles[0].events.push((0, effect(1, Origin::World([0.; 3]))));
            roles[1].events.push((30, voice(image, 0x424365)?));
            for at in (30..150).step_by(12) {
                roles[1]
                    .events
                    .push((at, projectile(1, 0, Origin::TargetAim)));
            }
            let points = battle_scenes::unison_points(image, native)?;
            ensure!(points.len() == 20, "Gospel strike-point count changed");
            for (index, offset) in points.into_iter().enumerate() {
                roles[1].events.push((
                    30 + 6 * index as u16,
                    effect(
                        2,
                        Origin::TargetAimOffset {
                            offset,
                            rotated: true,
                        },
                    ),
                ));
            }
            roles[1].events.extend([
                (170, effect(3, Origin::TargetAim)),
                (170, projectile(1, 1, Origin::TargetAim)),
            ]);
        }
        325 => {
            roles[0].events.extend([
                (0, effect(1, Origin::World([0.; 3]))),
                (104, effect(3, Origin::TargetAim)),
                (104, projectile(1, 0, Origin::TargetAim)),
            ]);
            for (character, address) in [
                ("lloyd", 0x424844),
                ("zelos", 0x42486e),
                ("kratos", 0x424894),
            ] {
                voices.push(PairVoice {
                    characters: [character.to_owned(), "raine".to_owned()],
                    speaker: 1,
                    sound: battle_voices::sound(pe::word(image.slice(address, 4)?, 0)?)?
                        .context("Gungnir pair voice is disabled")?,
                });
            }
        }
        326 => {
            roles[0].events.extend([
                (0, effect(2, Origin::TargetAim)),
                (0, voice(image, 0x424b6b)?),
            ]);
            roles[1].events.extend([
                (0, effect(1, Origin::World([0.; 3]))),
                (30, projectile(1, 0, Origin::TargetRoot)),
                (70, voice(image, 0x424f13)?),
                (84, projectile(2, 1, Origin::TargetRoot)),
            ]);
            // Both callback branches explicitly test character4 (Raine).
            // The seal owner initializes program2 but does not own these pulses.
            let points = battle_scenes::unison_points(image, native)?;
            ensure!(
                points.len() == 12,
                "Purgatory Seal strike-point count changed"
            );
            for (index, offset) in points.into_iter().enumerate() {
                let at = 120 + 6 * index as u16;
                roles[1].events.extend([
                    (
                        at,
                        effect(
                            3,
                            Origin::WorldOffset {
                                offset,
                                rotated: true,
                            },
                        ),
                    ),
                    (at, projectile(1, 2, Origin::TargetRoot)),
                ]);
            }
            roles[1].events.extend([
                (150, voice(image, 0x425205)?),
                (220, projectile(3, 3, Origin::TargetRoot)),
            ]);
        }
        _ => unreachable!(),
    }
    let (at, duration, limit) = match native {
        323 => (90, 120, 12),
        324 => (160, 50, 12),
        325 => (96, 70, 8),
        326 => (210, 60, 12),
        _ => unreachable!(),
    };
    for role in &mut roles {
        role.events
            .push((at, Callback::Feedback { duration, limit }));
        role.events.sort_by_key(|(at, _)| *at);
    }
    let color_at = match native {
        323 => 0x423b58,
        324 => 0x423eb6,
        325 => 0x424677,
        326 => 0x424a96,
        _ => unreachable!(),
    };
    let color = image.slice(color_at, 3)?.try_into()?;
    let execution = Execution {
        roles,
        color: Some(color),
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
fn effect(member: u16, origin: Origin) -> Callback {
    Callback::Effect {
        member,
        origin,
        scale: Some(1.),
        follow: false,
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

fn source_tracks(
    source: &Source,
    image: &pe::Image<'_>,
    native: u16,
    roles: &[Role; 2],
) -> Result<()> {
    let name = image.text(pe::word(
        image.slice(0x8e98a8 + u32::from(native) * 4, 4)?,
        0,
    )?)?;
    let package = source.require(name)?;
    let parameters =
        battle_scenes::member(&package, 0x190)?.context("missing combination tracks")?;
    for variant in roles.iter().flat_map(|role| role.phases.values()) {
        ensure!(
            pe::half(parameters, 16 + usize::from(*variant) * 28)? > 0,
            "combination{native} selects an empty participant track{variant}"
        );
    }
    Ok(())
}

fn verify(image: &pe::Image<'_>, native: u16) -> Result<()> {
    let (initializer, callback_write, callback) = match native {
        323 => (0x423b30, 0x423d0b, 0x423d20),
        324 => (0x423e80, 0x42407a, 0x424090),
        325 => (0x424640, 0x4248f7, 0x424910),
        326 => (0x424a60, 0x424d26, 0x424d40),
        _ => unreachable!(),
    };
    let descriptor = pe::word(image.slice(0x8e9688 + u32::from(native) * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == initializer
            && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x427f20
            && image.slice(callback_write, 3)? == [0xc7, 0x43, 0x18]
            && pe::word(image.slice(callback_write + 3, 4)?, 0)? == callback,
        "combination{native} native callbacks changed"
    );
    let guards: &[(u32, &[u8])] = match native {
        323 => &[
            (0x423b54, &[0xc7, 0x44, 0x24, 0x10]),
            (0x423c13, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 0xb4, 5, 0, 0]),
            (0x423be1, &[0x32, 0xd2]),
            (0x423cf5, &[0xb2, 1]),
            (0x423d2b, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 90]),
            (0x423d4c, &[0xba, 120, 0, 0, 0]),
            (0x423d58, &[0xc6, 0x87, 0x27, 0x9a, 0, 0, 12]),
            (0x423d72, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 24]),
            (0x423dcc, &[5, 0x90, 1, 0, 0]),
            (0x423dfa, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 100]),
            (0x423e4e, &[0x8d, 0x87, 0x20, 3, 0, 0]),
            (0x423e63, &[0x83, 0xc1, 0x1c]),
        ],
        324 => &[
            (0x423eb2, &[0xc7, 0x44, 0x24, 0x0c]),
            (0x423f43, &[0xb2, 1]),
            (0x424028, &[0x32, 0xd2]),
            (0x423f8c, &[0x6a, 1]),
            (0x42425b, &[0xba, 160, 0, 0, 0]),
            (0x4242fe, &[0xb8, 50, 0, 0, 0]),
            (0x42430a, &[0xc6, 0x87, 0x27, 0x9a, 0, 0, 12]),
            (0x424320, &[0x80, 0xf9, 4]),
            (0x424330, &[0x66, 0x83, 0xf8, 30]),
            (0x424384, &[0xb9, 150, 0, 0, 0]),
            (0x424397, &[0xb9, 12, 0, 0, 0]),
            (0x4243ee, &[0x8d, 0x82, 0x90, 1, 0, 0]),
            (0x42443a, &[0xb9, 6, 0, 0, 0]),
            (0x424511, &[0x6a, 2]),
            (0x424533, &[0xba, 170, 0, 0, 0]),
            (0x424590, &[0x6a, 3]),
            (0x424612, &[0x83, 0xc1, 0x1c]),
        ],
        325 => &[
            (0x424673, &[0xc7, 0x44, 0x24, 0x14]),
            (0x424710, &[0x3c, 1]),
            (0x424718, &[0x3c, 6]),
            (0x424722, &[0x3c, 9]),
            (0x4247f9, &[0xb2, 3]),
            (0x424762, &[0x6a, 1]),
            (0x42491b, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 96]),
            (0x42493b, &[0xba, 70, 0, 0, 0]),
            (0x424965, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 104]),
            (0x424947, &[0xc6, 0x80, 0x27, 0x9a, 0, 0, 8]),
            (0x4249be, &[0x6a, 3]),
            (0x424a26, &[0x8d, 0x82, 0x90, 1, 0, 0]),
        ],
        326 => &[
            (0x424a92, &[0xc7, 0x44, 0x24, 0x0c]),
            (0x424b27, &[0x32, 0xd2]),
            (0x424c82, &[0xb2, 1]),
            (0x424bb1, &[0x6a, 2]),
            (0x424cbb, &[0x6a, 1]),
            (0x424d7f, &[0xba, 210, 0, 0, 0]),
            (0x424eab, &[0xb9, 60, 0, 0, 0]),
            (0x424eb7, &[0xc6, 0x80, 0x27, 0x9a, 0, 0, 12]),
            (
                0x424ec5,
                &[
                    0x8a, 0x96, 0x22, 0x13, 0, 0, 0x80, 0xe2, 0x0f, 0x80, 0xfa, 4,
                ],
            ),
            (0x424ede, &[0x66, 0x83, 0xf8, 70]),
            (0x424f28, &[0x66, 0x83, 0xf8, 120]),
            (0x424f32, &[0xb9, 192, 0, 0, 0]),
            (0x424f45, &[0xb9, 6, 0, 0, 0]),
            (0x424ff7, &[0x6a, 3]),
            (0x425062, &[0x8d, 0x81, 0x90, 1, 0, 0]),
            (0x425078, &[0x83, 0xc1, 0x38]),
            (0x42508d, &[0x66, 0x83, 0xbe, 0xbe, 1, 0, 0, 84]),
            (0x4250f4, &[5, 0x20, 3, 0, 0]),
            (0x42511e, &[0xba, 220, 0, 0, 0]),
            (0x42518a, &[5, 0xb0, 4, 0, 0]),
            (0x42519f, &[0x83, 0xc1, 0x54]),
            (
                0x4251b4,
                &[
                    0x8a, 0x96, 0x22, 0x13, 0, 0, 0x80, 0xe2, 0x0f, 0x80, 0xfa, 4,
                ],
            ),
            (0x42521a, &[0x66, 0x83, 0xf9, 30]),
            (0x42527e, &[5, 0x90, 1, 0, 0]),
        ],
        _ => unreachable!(),
    };
    for &(at, bytes) in guards {
        ensure!(
            image.slice(at, bytes.len())? == bytes,
            "combination{native} controller changed at{at:#x}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires native Steam; reads only the four upper Unison controllers"]
    fn installed_upper_unison_keeps_every_native_hit_and_effect() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        for (native, projectiles, effects) in
            [(323, 2, 0), (324, 11, 22), (325, 1, 2), (326, 15, 14)]
        {
            let execution = read(&source, &image, native)?.context("missing upper choreography")?;
            let events = || execution.roles.iter().flat_map(|role| &role.events);
            ensure!(
                events()
                    .filter(|(_, event)| matches!(event, Callback::Projectile { .. }))
                    .count()
                    == projectiles,
                "combination{native} lost a native hit"
            );
            ensure!(
                events()
                    .filter(|(_, event)| matches!(event, Callback::Effect { .. }))
                    .count()
                    == effects,
                "combination{native} lost native visual choreography"
            );
            ensure!(
                events()
                    .filter(|(_, event)| matches!(event, Callback::Feedback { .. }))
                    .count()
                    == 2,
                "combination{native} lost participant feedback"
            );
            let roundtrip: Execution = serde_json::from_slice(&serde_json::to_vec(&execution)?)?;
            ensure!(
                serde_json::to_value(&roundtrip)? == serde_json::to_value(&execution)?,
                "combination{native} shared recipe did not roundtrip"
            );
            if native == 326 {
                ensure!(
                    !execution.roles[0]
                        .events
                        .iter()
                        .any(|(_, event)| matches!(event, Callback::Projectile { .. })),
                    "Purgatory pulses incorrectly assigned to its seal owner"
                );
                ensure!(
                    execution.roles[1].phases.get("raine") == Some(&1),
                    "Purgatory native actor gate changed"
                );
            }
            if native == 325 {
                ensure!(
                    execution.voices.len() == 3
                        && execution.voices.iter().all(|voice| voice.speaker == 1),
                    "Gungnir lost its partner-dependent Raine recording"
                );
            }
        }
        Ok(())
    }
}
