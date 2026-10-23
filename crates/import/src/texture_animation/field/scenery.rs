//! Water, scenery and event-prop texture motion.
use super::*;
use resonance_content::field::TextureControl;

fn sampled(
    actor: RenderValue,
    texture: RenderValue,
    offsets: Vec<[f32; 2]>,
    loop_start: usize,
) -> FieldTextureAnimation {
    track(
        actor,
        texture,
        TextureMotion::Sampled {
            animation: resonance_content::TextureAnimation {
                texture: (),
                delay_ticks: 0,
                loop_start,
                offsets,
            },
        },
    )
}

fn stepped_water(texture: RenderValue) -> FieldTextureAnimation {
    FieldTextureAnimation {
        clock: TextureClock::Effect,
        ..atlas(WATER, texture, 256, 1, true)
    }
}

pub(super) fn extend(profiles: &mut BTreeMap<u32, Vec<FieldTextureAnimation>>) {
    profiles.extend([
        (
            137,
            vec![
                FieldTextureAnimation {
                    control: Some(TextureControl::ClearWhenDisabled),
                    ..sampled(
                        BACKGROUND,
                        Setting(0),
                        (0..=217)
                            .map(|tick| [0., (tick / 7) as f32 / 32.])
                            .collect(),
                        217,
                    )
                },
                FieldTextureAnimation {
                    clock: TextureClock::UnconditionalActor,
                    ..atlas(Setting(1), Fixed(0), 8, 9, false)
                },
            ],
        ),
        (
            143,
            vec![
                atlas(DETAILS, Setting(0), 32, 10, false),
                atlas(WATER_DETAILS, Setting(1), 8, 3, false),
                atlas(WATER_DETAILS, Setting(2), 16, 4, false),
            ],
        ),
        (
            232,
            vec![FieldTextureAnimation {
                actor: Setting(0),
                ..water(Fixed(0))
            }],
        ),
        (
            233,
            vec![
                FieldTextureAnimation {
                    clock: TextureClock::EffectPulse {
                        interval: 2,
                        phase: 1,
                        unconditional: true,
                    },
                    ..looping_scroll(DETAILS, Fixed(0), [-1. / 300., 0.], 301)
                },
                FieldTextureAnimation {
                    clock: TextureClock::EffectPulse {
                        interval: 4,
                        phase: 0,
                        unconditional: true,
                    },
                    ..looping_scroll(DETAILS, Fixed(1), [-1. / 512., 0.], 513)
                },
            ],
        ),
        (
            234,
            (0..20)
                .flat_map(|offset| {
                    let actor = SettingOffset { slot: 0, offset };
                    [
                        FieldTextureAnimation {
                            clock: TextureClock::Effect,
                            ..scroll(actor.clone(), Setting(1), [-0.01, 0.])
                        },
                        atlas(actor, Setting(1), 4, 4, false),
                    ]
                })
                .chain([
                    looping_scroll(Setting(3), Fixed(1), [0., -1. / 110.], 111),
                    looping_scroll(Setting(4), Fixed(0), [0.0025, -0.0025], 401),
                    looping_scroll(Setting(4), Fixed(1), [0.0025, -0.0025], 401),
                ])
                .collect(),
        ),
        (
            300,
            vec![looping_scroll(
                WATER_DETAILS,
                Setting(0),
                [0.00005, -0.00005],
                20_001,
            )],
        ),
        (
            301,
            vec![looping_scroll(
                DETAILS,
                Setting(0),
                [0.00005, -0.00005],
                20_001,
            )],
        ),
        (
            308,
            vec![looping_scroll(DETAILS, Setting(0), [0.0003, 0.], 3334)],
        ),
        (
            309,
            vec![looping_scroll(DETAILS, Setting(0), [0., 0.01], 101)],
        ),
        (
            326,
            vec![
                stepped_water(Setting(0)),
                atlas(WATER, Setting(1), 8, 16, false),
            ],
        ),
        (
            329,
            vec![
                looping_scroll(WATER, Setting(0), [-0.005, 0.], 201),
                atlas(WATER_DETAILS, Setting(1), 4, 8, false),
                atlas(WATER_DETAILS, Setting(2), 8, 8, false),
                atlas(WATER_DETAILS, Setting(3), 8, 10, false),
            ],
        ),
        (
            341,
            vec![
                atlas(WATER, Setting(0), 4, 12, false),
                atlas(WATER_DETAILS, Setting(1), 32, 6, false),
                atlas(WATER_DETAILS, Setting(2), 16, 6, false),
            ],
        ),
        (
            342,
            vec![
                atlas(WATER_DETAILS, Setting(1), 32, 6, false),
                atlas(WATER_DETAILS, Setting(2), 16, 6, false),
            ],
        ),
        (343, vec![atlas(WATER_DETAILS, Setting(0), 32, 6, false)]),
        (
            352,
            vec![FieldTextureAnimation {
                clock: TextureClock::UnconditionalEffect,
                ..atlas(BACKGROUND, Fixed(7), 256, 1, false)
            }],
        ),
        (
            374,
            [6, 7]
                .map(|actor| {
                    sampled(
                        Setting(actor),
                        Fixed(1),
                        [0., 0.25, 0.5, 0.75, 0.5, 0.25]
                            .into_iter()
                            .flat_map(|v| std::iter::repeat_n([0., v], 4))
                            .collect(),
                        0,
                    )
                })
                .into(),
        ),
        (
            380,
            vec![
                looping_scroll(WATER, Setting(0), [0., -0.0004], 2500),
                atlas(WATER_DETAILS, Setting(1), 4, 15, true),
            ],
        ),
        (
            419,
            vec![
                atlas(WATER_DETAILS, Setting(0), 16, 10, false),
                water(Setting(1)),
            ],
        ),
        (
            435,
            vec![
                atlas(WATER_DETAILS, Setting(0), 4, 11, false),
                looping_scroll(WATER, Setting(1), [0., -1. / 300.], 301),
            ],
        ),
        (
            523,
            vec![FieldTextureAnimation {
                control: Some(TextureControl::FinishWhen {
                    setting: 1,
                    tick: 90,
                }),
                ..sampled(
                    DETAILS,
                    Setting(0),
                    (0..=90).map(|tick| [0., tick as f32 / 180.]).collect(),
                    90,
                )
            }],
        ),
        (
            524,
            vec![
                looping_scroll(WATER, Setting(0), [0., -0.002], 501),
                looping_scroll(WATER, Setting(1), [0.002, 0.], 501),
            ],
        ),
        (
            534,
            vec![
                looping_scroll(Setting(4), Fixed(2), [0., 0.08], 13),
                looping_scroll(Setting(5), Fixed(0), [0., -0.012], 84),
            ],
        ),
    ]);
    for map in 156..=158 {
        profiles.insert(
            map,
            vec![looping_scroll(BACKGROUND, Setting(0), [0., -0.002], 501)],
        );
    }
    for map in [235, 237, 280] {
        profiles.insert(
            map,
            vec![FieldTextureAnimation {
                actor: DETAILS,
                ..water(Setting(0))
            }],
        );
    }
    for map in [330, 344] {
        profiles.insert(map, vec![atlas(WATER, Setting(0), 4, 12, false)]);
    }
    for map in [370, 372, 373] {
        profiles.insert(
            map,
            vec![FieldTextureAnimation {
                clock: TextureClock::UnconditionalEffect,
                ..stepped_water(Fixed(0))
            }],
        );
    }
    profiles
        .entry(TOIZE_WATERFALL)
        .or_default()
        .push(looping_scroll(WATER_DETAILS, Setting(0), [0., -0.005], 201));
}
