//! Field texture motion profiles. Speeds are UV units per simulation tick.
use RenderValue::{Fixed, Setting, SettingOffset};
use resonance_content::field::{
    FieldTextureAnimation, FieldTextureWave, RenderValue, TextureClock, TextureMotion,
};
use std::collections::BTreeMap;

mod scenery;
mod water_and_machinery;

const BACKGROUND: RenderValue = Fixed(999_996);
const WATER: RenderValue = Fixed(999_997);
const DETAILS: RenderValue = Fixed(999_998);
const WATER_DETAILS: RenderValue = Fixed(999_980);
const THODA_OUTSIDE: u32 = 6;
const THODA_ENTRANCE: u32 = 7;
const THODA_STAIRS: u32 = 8;
const THODA_PUZZLE: u32 = 9;
const THODA_SEAL: u32 = 10;
const ALTAMIRA_DAY: u32 = 11;
const ALTAMIRA_NIGHT: u32 = 12;
const ALTAMIRA_CASINO: u32 = 13;
const ALTAMIRA_BEACH: u32 = 14;
const ALTAMIRA_PARK: u32 = 15;
const ALTAMIRA_HOTEL_LOBBY: u32 = 16;
const LEZARENO_LOBBY: u32 = 24;
const LEZARENO_SKY_TERRACE: u32 = 26;
const ASGARD_DAIS: u32 = 31;
const PALMACOSTA_ENTRANCE: u32 = 54;
const PALMACOSTA_HARBOR: u32 = 55;
const PALMACOSTA_SQUARE: u32 = 58;
const TETHEALLA_BRIDGES: std::ops::RangeInclusive<u32> = 75..=78;
const THODA_DOCK: u32 = 82;
const DERRIS_KHARLAN: [u32; 8] = [128, 129, 130, 131, 132, 135, 136, 530];
const TOWER_ALTAR: u32 = 152;
const TOWER_TRAPS: u32 = 182;
const TOWER_FINAL_CHAMBER: u32 = 184;
const EXIRE_OUTDOORS: std::ops::RangeInclusive<u32> = 189..=190;
const ASGARD_CONVEYOR: u32 = 213;
const TRIET_SEAL: u32 = 221;
const RODYLE_RANCH_ENTRANCE: u32 = 222;
const SYLVARANT_BASE_HALL: u32 = 270;
const HEIMDALL_VILLAGE: std::ops::RangeInclusive<u32> = 282..=284;
const HEIMDALL_RUINS: std::ops::RangeInclusive<u32> = 291..=292;
const HIMA_INN_ROOMS: [u32; 2] = [304, 305];
const MARTEL_SEAL: u32 = 307;
const ICE_PUZZLE: u32 = 324;
const IZOOLD_VILLAGE: u32 = 353;
const IZOOLD_HARBOR: u32 = 354;
const MANA_LAMPS: u32 = 362;
const MANA_BRIDGES: u32 = 366;
const MANA_SEAL: u32 = 369;
const TOIZE_WATERFALL: u32 = 414;
const OZETTE_ATTACK: u32 = 432;
const SYBAK_RESEARCH_ROOM: u32 = 439;
const MELTOKIO_SEWERS: std::ops::RangeInclusive<u32> = 466..=468;
const TEMPLE_OF_LIGHTNING: u32 = 475;
const TORENT_STREAM: u32 = 480;
const TORENT_POND: u32 = 481;
const TORENT_CASCADE: u32 = 482;
const ORIGIN_ALTAR: u32 = 484;
const LATHEON_GORGE: std::ops::RangeInclusive<u32> = 492..=498;
const LATHEON_TWIN_WATERFALLS: u32 = 497;
const WELGAIA_OUTDOORS: std::ops::RangeInclusive<u32> = 500..=503;
const BALACRUF_WIND: u32 = 510;
const TOWER_OF_SALVATION: u32 = 535;
const TOWER_CHAMBERS: [u32; 3] = [150, TOWER_OF_SALVATION, 536];

fn track(actor: RenderValue, texture: RenderValue, kind: TextureMotion) -> FieldTextureAnimation {
    FieldTextureAnimation {
        actor,
        texture,
        motion: kind,
        clock: TextureClock::Field,
        control: None,
    }
}
fn scroll(actor: RenderValue, texture: RenderValue, velocity: [f32; 2]) -> FieldTextureAnimation {
    track(
        actor,
        texture,
        TextureMotion::Scroll {
            velocity,
            wave: None,
            restart_ticks: None,
        },
    )
}

fn actor_clock(mut animation: FieldTextureAnimation) -> FieldTextureAnimation {
    animation.clock = TextureClock::Actor;
    animation
}
fn atlas(
    actor: RenderValue,
    texture: RenderValue,
    frames: u32,
    interval: u32,
    horizontal: bool,
) -> FieldTextureAnimation {
    let step = 1. / frames as f32;
    track(
        actor,
        texture,
        TextureMotion::Atlas {
            frames,
            interval,
            step: if horizontal { [step, 0.] } else { [0., step] },
        },
    )
}
fn water(texture: RenderValue) -> FieldTextureAnimation {
    FieldTextureAnimation {
        actor: WATER,
        texture,
        clock: TextureClock::Effect,
        control: None,
        motion: TextureMotion::Scroll {
            velocity: [-0.00125; 2],
            restart_ticks: None,
            wave: Some(FieldTextureWave {
                degrees_per_tick: 2.,
                amplitude: -0.025,
                horizontal: false,
            }),
        },
    }
}
fn grid_atlas(
    texture: u8,
    columns: u32,
    rows: u32,
    frames: u32,
    interval: u32,
    phase: u32,
) -> FieldTextureAnimation {
    FieldTextureAnimation {
        clock: TextureClock::EffectPulse {
            interval,
            phase,
            unconditional: false,
        },
        ..track(
            DETAILS,
            Setting(texture),
            TextureMotion::Sampled {
                animation: resonance_content::TextureAnimation {
                    texture: (),
                    delay_ticks: 0,
                    loop_start: 0,
                    offsets: (0..frames)
                        .map(|frame| {
                            [
                                (frame / rows) as f32 / columns as f32,
                                (frame % rows) as f32 / rows as f32,
                            ]
                        })
                        .collect(),
                },
            },
        )
    }
}
fn flowing(actor: RenderValue, texture: u8, speed: f32) -> FieldTextureAnimation {
    FieldTextureAnimation {
        clock: TextureClock::Effect,
        ..scroll(actor, Setting(texture), [0., -speed])
    }
}

fn fountain_flow(texture: u8, speed: f32, period: u32) -> FieldTextureAnimation {
    looping_scroll(WATER_DETAILS, Setting(texture), [0., speed], period)
}

fn looping_scroll(
    actor: RenderValue,
    texture: RenderValue,
    velocity: [f32; 2],
    period: u32,
) -> FieldTextureAnimation {
    track(
        actor,
        texture,
        TextureMotion::Scroll {
            velocity,
            wave: None,
            restart_ticks: Some(period),
        },
    )
}

pub(crate) fn profiles() -> BTreeMap<u32, Vec<FieldTextureAnimation>> {
    let mut profiles = BTreeMap::from([
        (TORENT_STREAM, vec![flowing(WATER_DETAILS, 1, 0.01)]),
        (
            TORENT_POND,
            vec![looping_scroll(WATER, Setting(0), [0., -0.001], 1001)],
        ),
        (
            TORENT_CASCADE,
            vec![looping_scroll(WATER_DETAILS, Setting(0), [0., -0.03], 34)],
        ),
        (
            ORIGIN_ALTAR,
            vec![actor_clock(looping_scroll(
                Setting(0),
                Setting(1),
                [0., 0.01],
                101,
            ))],
        ),
        (
            TOWER_FINAL_CHAMBER,
            vec![
                looping_scroll(DETAILS, Setting(0), [1. / 150., 0.], 151),
                looping_scroll(BACKGROUND, Setting(1), [0., -0.002], 501),
                looping_scroll(BACKGROUND, Setting(2), [0.002, 0.], 501),
            ],
        ),
        (
            TOWER_TRAPS,
            (0..5)
                .flat_map(|offset| {
                    [(0, -1. / 480.), (1, 1. / 480.)].map(|(texture, speed)| {
                        looping_scroll(
                            SettingOffset { slot: 0, offset },
                            Fixed(texture),
                            [speed, 0.],
                            480,
                        )
                    })
                })
                .collect(),
        ),
        (
            ALTAMIRA_HOTEL_LOBBY,
            vec![grid_atlas(0, 4, 4, 16, 4, 0), grid_atlas(1, 8, 8, 20, 2, 1)],
        ),
        (
            LEZARENO_LOBBY,
            vec![atlas(DETAILS, Setting(0), 16, 6, false)],
        ),
        (
            LEZARENO_SKY_TERRACE,
            vec![atlas(DETAILS, Setting(0), 16, 7, false)],
        ),
        (
            OZETTE_ATTACK,
            vec![atlas(WATER_DETAILS, Setting(0), 32, 1, false)],
        ),
        (
            TEMPLE_OF_LIGHTNING,
            [(2, [0., 0.005]), (0, [0.005, 0.005])]
                .map(|(texture, velocity)| {
                    track(
                        Setting(7),
                        Fixed(texture),
                        TextureMotion::Scroll {
                            velocity,
                            wave: None,
                            restart_ticks: Some(201),
                        },
                    )
                })
                .into(),
        ),
        (
            ASGARD_DAIS,
            vec![
                looping_scroll(DETAILS, Setting(0), [0., -0.001], 1001),
                looping_scroll(DETAILS, Setting(1), [0., -0.001], 1001),
                actor_clock(looping_scroll(Setting(2), Fixed(0), [0., 0.01], 101)),
            ],
        ),
        (
            SYLVARANT_BASE_HALL,
            vec![scroll(DETAILS, Setting(0), [-1. / 64., 0.])],
        ),
        (
            TOIZE_WATERFALL,
            vec![track(
                DETAILS,
                Setting(1),
                TextureMotion::Scroll {
                    velocity: [0., -0.007],
                    wave: None,
                    restart_ticks: Some(143),
                },
            )],
        ),
        (
            ASGARD_CONVEYOR,
            vec![looping_scroll(BACKGROUND, Setting(0), [-1. / 60., 0.], 61)],
        ),
        (
            TRIET_SEAL,
            (0..=1)
                .map(|texture| {
                    actor_clock(looping_scroll(
                        Setting(2),
                        Setting(texture),
                        [0., -0.002],
                        501,
                    ))
                })
                .collect(),
        ),
        (
            BALACRUF_WIND,
            vec![looping_scroll(Setting(0), Fixed(0), [0., -0.01], 101)],
        ),
        (
            SYBAK_RESEARCH_ROOM,
            [(0, [0., 0.005]), (1, [0.005, 0.]), (2, [0., 0.005])]
                .map(|(texture, velocity)| {
                    track(
                        Setting(0),
                        Fixed(texture),
                        TextureMotion::Scroll {
                            velocity,
                            wave: None,
                            restart_ticks: Some(201),
                        },
                    )
                })
                .into(),
        ),
        (MARTEL_SEAL, vec![atlas(DETAILS, Setting(0), 8, 7, false)]),
        (
            RODYLE_RANCH_ENTRANCE,
            vec![atlas(DETAILS, Setting(0), 8, 32, false)],
        ),
        (
            ICE_PUZZLE,
            vec![
                flowing(Setting(0), 1, 0.02),
                flowing(Setting(2), 3, 0.02),
                flowing(Setting(4), 5, 0.00125),
            ],
        ),
        (
            THODA_DOCK,
            vec![
                water(Fixed(0)),
                atlas(WATER_DETAILS, Setting(0), 4, 15, false),
            ],
        ),
        (
            IZOOLD_VILLAGE,
            vec![
                atlas(DETAILS, Setting(0), 16, 12, false),
                atlas(DETAILS, Setting(1), 16, 8, false),
            ],
        ),
        (
            IZOOLD_HARBOR,
            vec![
                water(Fixed(0)),
                atlas(WATER_DETAILS, Setting(1), 4, 15, false),
                atlas(Fixed(3000), Setting(2), 4, 15, false),
            ],
        ),
        (MANA_LAMPS, vec![atlas(DETAILS, Setting(0), 5, 7, true)]),
        (
            MANA_BRIDGES,
            (2..=4)
                .flat_map(|actor| {
                    (0..=1).map(move |texture| {
                        looping_scroll(Setting(actor), Setting(texture), [0., -0.002], 501)
                    })
                })
                .collect(),
        ),
        (
            MANA_SEAL,
            // Both seal-cylinder textures scroll together.
            (0..=1)
                .map(|texture| {
                    actor_clock(looping_scroll(
                        Setting(2),
                        Setting(texture),
                        [0., -0.002],
                        501,
                    ))
                })
                .collect(),
        ),
    ]);
    for map in EXIRE_OUTDOORS {
        profiles.insert(
            map,
            vec![
                looping_scroll(BACKGROUND, Setting(0), [0., -0.0001], 10000),
                atlas(BACKGROUND, Setting(1), 12, 8, false),
                atlas(BACKGROUND, Setting(2), 8, 8, false),
                atlas(WATER_DETAILS, Setting(3), 16, 8, false),
                looping_scroll(DETAILS, Setting(4), [0., -0.0002], 5001),
            ],
        );
    }
    for map in [TOWER_ALTAR, TOWER_OF_SALVATION] {
        profiles.insert(
            map,
            [(4, 2, 0.08, 13), (5, 0, -0.012, 84)]
                .map(|(actor, texture, speed, period)| {
                    looping_scroll(Setting(actor), Fixed(texture), [0., speed], period)
                })
                .into(),
        );
    }
    for map in TOWER_CHAMBERS {
        profiles.entry(map).or_default().extend([
            looping_scroll(BACKGROUND, Setting(0), [0., 1. / 600.], 601),
            looping_scroll(BACKGROUND, Setting(1), [1. / 1500., 0.], 1500),
        ]);
    }
    profiles
        .get_mut(&TOWER_ALTAR)
        .unwrap()
        .extend((6..=7).map(|actor| {
            track(
                Setting(actor),
                Fixed(1),
                TextureMotion::Sampled {
                    animation: resonance_content::TextureAnimation {
                        texture: (),
                        delay_ticks: 0,
                        loop_start: 0,
                        offsets: [0., 0.25, 0.5, 0.75, 0.5, 0.25]
                            .into_iter()
                            .flat_map(|v| std::iter::repeat_n([0., v], 4))
                            .collect(),
                    },
                },
            )
        }));
    for map in HEIMDALL_VILLAGE.chain(HEIMDALL_RUINS) {
        let mut tracks = if map == 292 {
            vec![
                atlas(BACKGROUND, Setting(0), 16, 8, false),
                looping_scroll(WATER, Setting(1), [0.005, 0.], 201),
            ]
        } else {
            vec![
                looping_scroll(WATER, Setting(0), [0.005, 0.], 201),
                water(Setting(1)),
            ]
        };
        if map == 284 {
            tracks.extend([
                atlas(DETAILS, Setting(2), 8, 8, false),
                atlas(DETAILS, Setting(3), 16, 8, false),
            ]);
        }
        profiles.insert(map, tracks);
    }
    for map in LATHEON_GORGE {
        let actors: &[RenderValue] = if map == 497 {
            &[Setting(4), Setting(5)]
        } else {
            &[DETAILS]
        };
        let tracks = actors
            .iter()
            .flat_map(|actor| {
                [
                    (0, 0.004, 250),
                    (1, 0.002, 501),
                    (2, 0.002, 501),
                    (3, 0.008, 125),
                ]
                .into_iter()
                .filter(move |&(slot, _, _)| match map {
                    493 | 497 => slot != 0,
                    495 => slot == 0,
                    496 => false,
                    _ => true,
                })
                .map(move |(slot, speed, period)| {
                    looping_scroll(actor.clone(), Setting(slot), [0., -speed], period)
                })
            })
            .collect::<Vec<_>>();
        if !tracks.is_empty() {
            profiles.insert(map, tracks);
        }
    }
    for map in DERRIS_KHARLAN {
        let background = if map == 129 { Setting(2) } else { BACKGROUND };
        let mut tracks = vec![
            looping_scroll(background.clone(), Setting(0), [0., 0.005], 201),
            looping_scroll(background, Setting(1), [0., 0.002], 501),
        ];
        if map == 129 {
            for track in &mut tracks {
                track.clock = TextureClock::Actor;
            }
        }
        match map {
            129 => tracks.push(looping_scroll(Setting(4), Setting(5), [0., 1. / 30.], 30)),
            130 => tracks.extend([
                track(
                    DETAILS,
                    Setting(2),
                    TextureMotion::Atlas {
                        frames: 7,
                        interval: 5,
                        step: [0.2, 0.],
                    },
                ),
                looping_scroll(DETAILS, Setting(3), [0.001, 0.], 1001),
            ]),
            135 => tracks.extend([
                looping_scroll(DETAILS, Setting(2), [0., 0.005], 201),
                looping_scroll(DETAILS, Setting(3), [0., 0.002], 501),
            ]),
            136 | 530 => {
                tracks.extend([
                    looping_scroll(Setting(4), Fixed(2), [0., 0.08], 13),
                    looping_scroll(Setting(5), Fixed(0), [0., -0.012], 84),
                ]);
                tracks.extend([6, 7].map(|slot| {
                    track(
                        Setting(slot),
                        Fixed(1),
                        TextureMotion::Sampled {
                            animation: resonance_content::TextureAnimation {
                                texture: (),
                                delay_ticks: 0,
                                loop_start: 0,
                                offsets: [0., 0.25, 0.5, 0.75, 0.5, 0.25]
                                    .into_iter()
                                    .flat_map(|v| std::iter::repeat_n([0., v], 4))
                                    .collect(),
                            },
                        },
                    )
                }));
            }
            _ => {}
        }
        profiles.insert(map, tracks);
    }
    for map in WELGAIA_OUTDOORS {
        let mut tracks = match map {
            500 => Vec::new(),
            501 => vec![
                looping_scroll(DETAILS, Setting(0), [0., -1. / 60.], 61),
                actor_clock(looping_scroll(Setting(1), Setting(2), [0., -1. / 60.], 61)),
                actor_clock(looping_scroll(Setting(1), Setting(3), [0., -1. / 30.], 30)),
            ],
            502 => vec![
                looping_scroll(DETAILS, Setting(0), [0., -1. / 60.], 61),
                looping_scroll(WATER, Setting(1), [0., -1. / 180.], 180),
                looping_scroll(Setting(4), Setting(5), [0., 1. / 30.], 30),
                looping_scroll(Setting(6), Fixed(0), [0., -1. / 90.], 90),
            ],
            _ => vec![
                looping_scroll(DETAILS, Setting(0), [0., -1. / 60.], 61),
                looping_scroll(Setting(3), Setting(4), [0., 1. / 90.], 90),
            ],
        };
        match map {
            501 => {
                tracks.extend((0..3).map(|offset| {
                    atlas(SettingOffset { slot: 4, offset }, Setting(5), 4, 15, false)
                }))
            }
            502 => tracks.extend((0..4).map(|offset| {
                looping_scroll(
                    SettingOffset { slot: 2, offset },
                    Setting(3),
                    [0., 1. / 120.],
                    121,
                )
            })),
            503 => tracks.extend((0..20).map(|offset| {
                looping_scroll(
                    SettingOffset { slot: 1, offset },
                    Setting(2),
                    [0., 1. / 60.],
                    61,
                )
            })),
            _ => {}
        }
        if map != 502 {
            tracks.push(actor_clock(looping_scroll(
                Setting(7),
                Fixed(0),
                [0.01, 0.02],
                101,
            )));
        }
        profiles.insert(map, tracks);
    }
    for map in ALTAMIRA_DAY..=ALTAMIRA_PARK {
        let sea = |texture| {
            track(
                WATER,
                Setting(texture),
                TextureMotion::Scroll {
                    velocity: [0., -0.001],
                    wave: None,
                    restart_ticks: Some(1001),
                },
            )
        };
        let tracks = match map {
            ALTAMIRA_DAY => vec![
                atlas(WATER_DETAILS, Setting(0), 4, 8, false),
                atlas(WATER_DETAILS, Setting(1), 16, 10, false),
                sea(2),
                actor_clock(atlas(Setting(3), Setting(4), 4, 8, false)),
            ],
            ALTAMIRA_NIGHT => vec![
                atlas(WATER_DETAILS, Setting(0), 16, 10, false),
                sea(1),
                actor_clock(atlas(Setting(2), Setting(3), 4, 8, false)),
            ],
            ALTAMIRA_CASINO => vec![
                atlas(DETAILS, Setting(0), 32, 2, false),
                sea(1),
                actor_clock(atlas(Setting(2), Setting(3), 4, 8, false)),
            ],
            ALTAMIRA_BEACH => vec![
                FieldTextureAnimation {
                    actor: DETAILS,
                    ..water(Setting(0))
                },
                atlas(DETAILS, Setting(1), 16, 10, false),
            ],
            _ => vec![
                atlas(WATER_DETAILS, Setting(0), 4, 8, false),
                water(Setting(1)),
                atlas(Setting(2), Setting(4), 4, 8, false),
            ],
        };
        profiles.insert(map, tracks);
    }
    for map in PALMACOSTA_ENTRANCE..=PALMACOSTA_SQUARE {
        let mut tracks = vec![
            water(if map == PALMACOSTA_SQUARE {
                Setting(1)
            } else {
                Fixed(0)
            }),
            atlas(WATER_DETAILS, Setting(0), 4, 10, false),
        ];
        if map == PALMACOSTA_HARBOR {
            tracks.push(atlas(Fixed(3000), Setting(2), 4, 10, false));
        }
        profiles.insert(map, tracks);
    }
    for map in MELTOKIO_SEWERS {
        let mut tracks = [(WATER, 0, -0.0004, 2500), (DETAILS, 1, -0.02, 51)]
            .map(|(actor, texture, speed, period)| {
                track(
                    actor,
                    Setting(texture),
                    TextureMotion::Scroll {
                        velocity: [0., speed],
                        wave: None,
                        restart_ticks: Some(period),
                    },
                )
            })
            .to_vec();
        tracks.extend((2..=3).map(|texture| atlas(DETAILS, Setting(texture), 4, 15, false)));
        profiles.insert(map, tracks);
    }
    for map in TETHEALLA_BRIDGES {
        profiles.insert(
            map,
            vec![
                water(Setting(4)),
                atlas(WATER_DETAILS, Setting(0), 16, 4, false),
                atlas(WATER_DETAILS, Setting(1), 16, 4, false),
                atlas(WATER_DETAILS, Setting(2), 16, 3, false),
                atlas(WATER_DETAILS, Setting(3), 4, 10, false),
            ],
        );
    }
    for map in [
        THODA_OUTSIDE,
        THODA_ENTRANCE,
        THODA_STAIRS,
        THODA_PUZZLE,
        THODA_SEAL,
    ] {
        let mut tracks = vec![water(Setting(0))];
        let lights: &[u8] = match map {
            THODA_OUTSIDE => {
                tracks.push(flowing(Setting(2), 3, 0.0025));
                &[1, 4, 5]
            }
            THODA_PUZZLE => {
                for (slot, speed) in [(1, 0.01), (2, 0.02), (3, 1. / 120.)] {
                    tracks.push(flowing(DETAILS, slot, speed));
                }
                &[4, 5]
            }
            THODA_SEAL => {
                tracks.extend([1, 2].map(|slot| flowing(DETAILS, slot, 0.01)));
                &[3, 4]
            }
            _ => &[1, 2],
        };
        tracks.extend(
            lights
                .iter()
                .map(|&slot| atlas(DETAILS, Setting(slot), 4, 15, false)),
        );
        if map == THODA_OUTSIDE {
            tracks.extend(
                (0..4).map(|offset| {
                    atlas(SettingOffset { slot: 6, offset }, Setting(7), 4, 15, false)
                }),
            );
        }
        profiles.insert(map, tracks);
    }
    // Luin's outdoor water, fountain streams and splash atlases.
    for map in 447..=457 {
        let tracks = match map {
            448 => vec![
                water(Setting(0)),
                atlas(WATER_DETAILS, Setting(1), 4, 8, false),
                fountain_flow(2, -0.015, 67),
                atlas(WATER_DETAILS, Setting(3), 4, 6, false),
                fountain_flow(4, -0.03, 34),
                fountain_flow(5, 0.022, 46),
            ],
            453 => vec![
                water(Setting(0)),
                atlas(WATER_DETAILS, Setting(1), 4, 6, false),
                fountain_flow(2, -0.04, 25),
                fountain_flow(3, -0.03, 34),
                fountain_flow(4, -1. / 240., 241),
                fountain_flow(5, -1. / 120., 121),
                atlas(WATER_DETAILS, Setting(6), 4, 8, false),
            ],
            456 => vec![
                water(Setting(6)),
                atlas(WATER_DETAILS, Setting(1), 4, 8, false),
                fountain_flow(2, -0.015, 67),
                atlas(WATER_DETAILS, Setting(3), 4, 6, false),
                fountain_flow(4, -0.022, 46),
            ],
            _ => vec![
                water(Setting(1)),
                atlas(WATER_DETAILS, Setting(0), 4, 8, false),
            ],
        };
        profiles.insert(map, tracks);
    }
    water_and_machinery::extend(&mut profiles);
    scenery::extend(&mut profiles);
    for map in HIMA_INN_ROOMS {
        profiles.insert(
            map,
            vec![track(
                BACKGROUND,
                Setting(0),
                TextureMotion::Sampled {
                    animation: resonance_content::TextureAnimation {
                        texture: (),
                        delay_ticks: 0,
                        loop_start: 0,
                        offsets: (0..64)
                            .map(|frame| [(frame / 8) as f32 / 8., (frame % 8) as f32 / 8.])
                            .collect(),
                    },
                },
            )],
        );
    }
    for animation in profiles.get_mut(&LATHEON_TWIN_WATERFALLS).unwrap() {
        animation.clock = TextureClock::SharedActors {
            actors: vec![Setting(4), Setting(5)],
        };
    }
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_are_valid_and_animate_on_their_selected_clock() -> anyhow::Result<()> {
        let profiles = profiles();
        for tracks in profiles.values() {
            for track in tracks {
                track.validate()?;
            }
        }
        let seal_settings = BTreeMap::from([(0, 1), (1, 2), (2, 1005)]);
        let seal = &profiles[&TRIET_SEAL];
        assert_eq!(
            seal.iter()
                .map(|track| (
                    track.actor.resolve(&seal_settings),
                    track.texture.resolve(&seal_settings)
                ))
                .collect::<Vec<_>>(),
            [(1005, 1), (1005, 2)]
        );
        for track in seal {
            assert_ne!(track.offset(60, 0), track.offset(0, 0));
            assert_eq!(track.offset(60, 100), track.offset(60, 0));
        }
        let bridge = &profiles[&MANA_BRIDGES][0];
        assert!((bridge.offset(501, 0)[1] + 1.002).abs() < 1e-6);
        assert_eq!(bridge.offset(502, 0), [0., -0.002]);
        let mana_settings = BTreeMap::from([(0, 3), (1, 0), (2, 2800)]);
        assert_eq!(
            profiles[&MANA_SEAL]
                .iter()
                .map(|track| (
                    track.actor.resolve(&mana_settings),
                    track.texture.resolve(&mana_settings),
                ))
                .collect::<Vec<_>>(),
            [(2800, 3), (2800, 0)]
        );
        for track in &profiles[&MANA_SEAL] {
            assert!((track.offset(60, 0)[1] + 0.12).abs() < 1e-6);
            assert_eq!(track.offset(60, 100), track.offset(60, 0));
        }
        let lamps = &profiles[&MANA_LAMPS][0];
        assert_ne!(lamps.offset(7, 0), lamps.offset(0, 0));
        assert_eq!(lamps.offset(35, 0), lamps.offset(0, 0));
        assert_ne!(profiles[&THODA_OUTSIDE][0].offset(0, 100), [0.; 2]);
        let sword = &profiles[&TOWER_OF_SALVATION];
        assert_eq!(sword[0].offset(13, 0), [0., 1.04]);
        assert_eq!(sword[0].offset(14, 0), [0., 0.08]);
        assert_eq!(sword[1].offset(84, 0), [0., -1.008]);
        assert_eq!(sword[1].offset(85, 0), [0., -0.012]);
        Ok(())
    }
}
