//! Innocent Edge's two native participant callbacks, without a family controller.
use crate::{battle_voices, pe, source::Source};
use anyhow::{Result, ensure};
use resonance_content::{
    battle_normal::Event,
    battle_unison::{Callback, Execution, Origin, Role, RolePlacement},
};

pub(super) fn read(
    _source: &Source,
    image: &pe::Image<'_>,
    native: u16,
) -> Result<Option<Execution>> {
    if native != 306 {
        return Ok(None);
    }
    let descriptor = pe::word(image.slice(0x8e9688 + 306 * 4, 4)?, 0)?;
    ensure!(
        pe::word(image.slice(descriptor, 4)?, 0)? == 0x4201d0
            && pe::word(image.slice(descriptor + 4, 4)?, 0)? == 0x427f20,
        "Innocent Edge handlers changed"
    );
    for (at, expected) in [
        (0x4201f4, &[0xc7, 0x44, 0x24, 0x10, 0x10, 4, 4, 0xff][..]),
        (0x420281, &[0x32, 0xd2][..]),
        (0x4203c7, &[0xb2, 1][..]),
        (0x4202b3, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 6, 5, 0, 0][..]),
        (0x4202d4, &[0x8b, 0x90, 0x3c, 0x39, 1, 0][..]),
        (0x4202ee, &[0xdc, 0x1d, 0x68, 0x2d, 0x88, 0][..]),
        (0x420330, &[0x6a, 1][..]),
        (0x42041f, &[0xc7, 0x43, 0x18, 0x30, 4, 0x42, 0][..]),
        (0x420437, &[0x66, 0x83, 0xb8, 0xbe, 1, 0, 0, 0x19][..]),
        (0x42045a, &[0xb9, 0x82, 0, 0, 0][..]),
        (0x420466, &[0xc6, 0x82, 0x27, 0x9a, 0, 0, 0x0c][..]),
        (0x42047d, &[0x80, 0xf9, 1][..]),
        (0x420482, &[0x66, 0x83, 0xb8, 0xbe, 1, 0, 0, 0x1e][..]),
        (0x4204a1, &[0x8b, 0xbe, 0x38, 0x39, 1, 0][..]),
        (0x4204d3, &[0x8d, 0x82, 0x90, 1, 0, 0][..]),
        (0x4204e8, &[0x8b, 0x55, 0xfc][..]),
    ] {
        ensure!(
            image.slice(at, expected.len())? == expected,
            "Innocent Edge callback changed at {at:#x}"
        );
    }
    let distance = f32::from_bits(pe::word(image.slice(0x8830a0, 4)?, 0)?);
    let minimum_height = f64::from_le_bytes(image.slice(0x882d68, 8)?.try_into()?) as f32;
    ensure!(
        distance == minimum_height && distance == 200.,
        "Innocent Edge placement/height changed"
    );
    let angle = f64::from_le_bytes(image.slice(0x884078, 8)?.try_into()?) as f32;
    let (sin, cos) = angle.sin_cos();
    let role = |name: &str, phase, direction| Role {
        phases: [(name.to_owned(), phase)].into(),
        placement: Some(RolePlacement {
            direction,
            distance,
        }),
        visible: true,
        temporary_weapons: false,
        events: vec![(
            25,
            Callback::Feedback {
                duration: 130,
                limit: 12,
            },
        )],
    };
    let mut roles = [
        role("presea", 0, [1., 0., 0.]),
        role("lloyd", 1, [cos, 0., sin]),
    ];
    roles[0].events.splice(
        0..0,
        [
            (
                0,
                Callback::Ordinary(Event::Voice(battle_voices::sound(0x506)?)),
            ),
            (
                0,
                Callback::Effect {
                    member: 1,
                    origin: Origin::TargetAimHeight { minimum_height },
                    scale: Some(1.),
                    follow: false,
                },
            ),
        ],
    );
    roles[1].events.push((
        30,
        Callback::Projectile {
            member: 1,
            hit_variant: 0,
            origin: Origin::TargetAim,
            heading: None,
            height: None,
        },
    ));
    let execution = Execution {
        roles,
        color: Some([16, 4, 4]),
        voices: Vec::new(),
    };
    execution.validate()?;
    Ok(Some(execution))
}
