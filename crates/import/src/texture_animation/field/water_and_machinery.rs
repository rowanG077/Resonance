use super::*;
use resonance_content::field::TextureControl;

const ALTAMIRA_HOTEL_UPPER_FLOOR: u32 = 17;
const LEZARENO_OFFICE: u32 = 27;
const DOCK_ANIMATED_DETAILS: u32 = 89;
const DOCK_WATER: u32 = 91;
const DOCK_WATER_AND_SPLASHES: u32 = 92;
const SHADOW_TEMPLE_ENTRANCE: u32 = 125;
const TRIET_RUINS_FIRE: u32 = 220;
const WATERFRONT_VARIANTS: std::ops::RangeInclusive<u32> = 246..=250;
const SYLVARANT_BASE_MACHINERY: u32 = 268;
const LIGHTNING_TEMPLE_INNER_ROOM: u32 = 476;
const TRIET_WATER: u32 = 486;

pub(super) fn extend(profiles: &mut BTreeMap<u32, Vec<FieldTextureAnimation>>) {
    profiles.extend([
        (
            ALTAMIRA_HOTEL_UPPER_FLOOR,
            vec![grid_atlas(0, 8, 8, 48, 2, 0)],
        ),
        (
            LEZARENO_OFFICE,
            vec![atlas(DETAILS, Setting(0), 8, 7, false)],
        ),
        (
            DOCK_ANIMATED_DETAILS,
            vec![
                atlas(WATER_DETAILS, Setting(0), 32, 5, false),
                atlas(WATER_DETAILS, Setting(1), 4, 12, false),
            ],
        ),
        (DOCK_WATER, vec![water(Setting(0))]),
        (
            DOCK_WATER_AND_SPLASHES,
            vec![
                water(Fixed(0)),
                atlas(WATER_DETAILS, Setting(0), 4, 15, false),
            ],
        ),
        (
            SHADOW_TEMPLE_ENTRANCE,
            vec![flowing(DETAILS, 0, 1. / 1600.)],
        ),
        (
            TRIET_RUINS_FIRE,
            [0, 3, 6]
                .into_iter()
                .enumerate()
                .map(|(texture, first_frame)| phased_fire(texture as u8, first_frame))
                .collect(),
        ),
        (
            SYLVARANT_BASE_MACHINERY,
            [2, 3]
                .into_iter()
                .flat_map(|actor| {
                    [
                        atlas(Setting(actor), Fixed(0), 4, 8, false),
                        FieldTextureAnimation {
                            clock: TextureClock::Actor,
                            control: Some(TextureControl::ResetUnless { setting: actor + 2 }),
                            ..looping_scroll(Setting(actor), Fixed(1), [0.01, 0.], 101)
                        },
                    ]
                })
                .collect(),
        ),
        (
            SYLVARANT_BASE_HALL,
            vec![looping_scroll(DETAILS, Setting(0), [-0.001, 0.], 1001)],
        ),
        (
            TRIET_WATER,
            vec![atlas(WATER_DETAILS, Setting(0), 16, 7, false)],
        ),
    ]);

    for field in WATERFRONT_VARIANTS {
        profiles.insert(
            field,
            vec![
                FieldTextureAnimation {
                    clock: TextureClock::Effect,
                    ..track(
                        WATER,
                        Setting(0),
                        TextureMotion::Scroll {
                            velocity: [0.00125; 2],
                            wave: Some(FieldTextureWave {
                                degrees_per_tick: 2.,
                                amplitude: 0.025,
                                horizontal: true,
                            }),
                            restart_ticks: None,
                        },
                    )
                },
                atlas(DETAILS, Fixed(0), 4, 15, false),
            ],
        );
    }
    for field in [TEMPLE_OF_LIGHTNING, LIGHTNING_TEMPLE_INNER_ROOM] {
        profiles.entry(field).or_default().extend([
            flowing(DETAILS, 0, 1. / 800.),
            atlas(Setting(6), Setting(5), 8, 1, false),
        ]);
    }
    profiles
        .entry(TORENT_STREAM)
        .or_default()
        .push(flowing(DETAILS, 0, 1. / 180.));
}

fn phased_fire(texture: u8, first_frame: u32) -> FieldTextureAnimation {
    track(
        DETAILS,
        Setting(texture),
        TextureMotion::Sampled {
            animation: resonance_content::TextureAnimation {
                texture: (),
                delay_ticks: 0,
                loop_start: 0,
                offsets: (first_frame..10)
                    .chain(0..first_frame)
                    .flat_map(|frame| {
                        std::iter::repeat_n(
                            [0., frame as f32 / 10.],
                            if frame == 0 { 30 } else { 6 },
                        )
                    })
                    .collect(),
            },
        },
    )
}
