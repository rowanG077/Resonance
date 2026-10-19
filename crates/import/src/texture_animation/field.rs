//! Field texture motion profiles. Speeds are UV units per simulation tick.
use RenderValue::{Fixed, Setting, SettingOffset};
use resonance_content::field::{
    FieldTextureAnimation, FieldTextureWave, RenderValue, TextureClock, TextureMotion,
};
use std::collections::BTreeMap;

const BACKGROUND: RenderValue = Fixed(999_996);
const WATER: RenderValue = Fixed(999_997);
const DETAILS: RenderValue = Fixed(999_998);
const WATER_DETAILS: RenderValue = Fixed(999_980);
const THODA_OUTSIDE: u32 = 6;
const THODA_ENTRANCE: u32 = 7;
const THODA_STAIRS: u32 = 8;
const THODA_PUZZLE: u32 = 9;
const THODA_SEAL: u32 = 10;
const ASGARD_DAIS: u32 = 31;
const PALMACOSTA_ENTRANCE: u32 = 54;
const PALMACOSTA_HARBOR: u32 = 55;
const PALMACOSTA_SQUARE: u32 = 58;
const TETHEALLA_BRIDGES: std::ops::RangeInclusive<u32> = 75..=78;
const THODA_DOCK: u32 = 82;
const ASGARD_CONVEYOR: u32 = 213;
const TRIET_SEAL: u32 = 221;
const SYLVARANT_BASE_HALL: u32 = 270;
const MARTEL_SEAL: u32 = 307;
const IZOOLD_VILLAGE: u32 = 353;
const IZOOLD_HARBOR: u32 = 354;
const MANA_LAMPS: u32 = 362;
const MANA_BRIDGES: u32 = 366;
const MANA_SEAL: u32 = 369;
const TOIZE_WATERFALL: u32 = 414;
const SYBAK_RESEARCH_ROOM: u32 = 439;
const MELTOKIO_SEWERS: std::ops::RangeInclusive<u32> = 466..=468;
const BALACRUF_WIND: u32 = 510;
const TOWER_OF_SALVATION: u32 = 535;

fn track(actor: RenderValue, texture: RenderValue, kind: TextureMotion) -> FieldTextureAnimation {
    FieldTextureAnimation {
        actor,
        texture,
        motion: kind,
        clock: TextureClock::Field,
    }
}
fn scroll(actor: RenderValue, texture: RenderValue, velocity: [f32; 2]) -> FieldTextureAnimation {
    track(
        actor,
        texture,
        TextureMotion::Scroll {
            velocity,
            vertical_wave: None,
            restart_ticks: None,
        },
    )
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
        motion: TextureMotion::Scroll {
            velocity: [-0.00125; 2],
            restart_ticks: None,
            vertical_wave: Some(FieldTextureWave {
                degrees_per_tick: 2.,
                amplitude: -0.025,
            }),
        },
    }
}
fn flowing(actor: RenderValue, texture: u8, speed: f32) -> FieldTextureAnimation {
    FieldTextureAnimation {
        clock: TextureClock::Effect,
        ..scroll(actor, Setting(texture), [0., -speed])
    }
}

fn fountain_flow(texture: u8, speed: f32, period: u32) -> FieldTextureAnimation {
    track(
        WATER_DETAILS,
        Setting(texture),
        TextureMotion::Scroll {
            velocity: [0., speed],
            vertical_wave: None,
            restart_ticks: Some(period),
        },
    )
}

pub(crate) fn profiles() -> BTreeMap<u32, Vec<FieldTextureAnimation>> {
    let mut profiles = BTreeMap::from([
        (
            ASGARD_DAIS,
            vec![
                scroll(DETAILS, Setting(0), [0., -0.001]),
                scroll(DETAILS, Setting(1), [0., -0.001]),
                scroll(Setting(2), Fixed(0), [0., 0.01]),
            ],
        ),
        (
            TOWER_OF_SALVATION,
            [(4, 2, 0.08, 13), (5, 0, -0.012, 84)]
                .map(|(actor, texture, speed, period)| {
                    track(
                        Setting(actor),
                        Fixed(texture),
                        TextureMotion::Scroll {
                            velocity: [0., speed],
                            vertical_wave: None,
                            restart_ticks: Some(period),
                        },
                    )
                })
                .into(),
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
                    vertical_wave: None,
                    restart_ticks: Some(143),
                },
            )],
        ),
        (
            ASGARD_CONVEYOR,
            vec![scroll(BACKGROUND, Setting(0), [-1. / 60., 0.])],
        ),
        (
            TRIET_SEAL,
            (0..=1)
                .map(|texture| scroll(Setting(2), Setting(texture), [0., -0.002]))
                .collect(),
        ),
        (
            BALACRUF_WIND,
            vec![scroll(Setting(0), Fixed(0), [0., -0.01])],
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
                            vertical_wave: None,
                            restart_ticks: Some(201),
                        },
                    )
                })
                .into(),
        ),
        (MARTEL_SEAL, vec![atlas(DETAILS, Setting(0), 8, 7, false)]),
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
                    (0..=1)
                        .map(move |texture| scroll(Setting(actor), Setting(texture), [0., -0.002]))
                })
                .collect(),
        ),
        (
            MANA_SEAL,
            // Both seal-cylinder textures scroll together.
            (0..=1)
                .map(|texture| scroll(Setting(2), Setting(texture), [0., -0.002]))
                .collect(),
        ),
    ]);
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
                        vertical_wave: None,
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
        assert_eq!(profiles[&MANA_BRIDGES][0].offset(1000, 0), [0., -2.]);
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
