//! Field texture motion profiles. Speeds are UV units per simulation tick.
use RenderValue::{Fixed, Setting, SettingOffset};
use resonance_content::field::{
    FieldTextureAnimation, FieldTextureWave, RenderValue, TextureClock, TextureMotion,
};
use std::collections::BTreeMap;

const BACKGROUND: RenderValue = Fixed(999_996);
const WATER: RenderValue = Fixed(999_997);
const DETAILS: RenderValue = Fixed(999_998);
const THODA_OUTSIDE: u32 = 6;
const THODA_ENTRANCE: u32 = 7;
const THODA_STAIRS: u32 = 8;
const THODA_PUZZLE: u32 = 9;
const THODA_SEAL: u32 = 10;
const ASGARD_CONVEYOR: u32 = 213;
const MARTEL_SEAL: u32 = 307;
const MANA_LAMPS: u32 = 362;
const MANA_BRIDGES: u32 = 366;
const BALACRUF_WIND: u32 = 510;

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
fn water() -> FieldTextureAnimation {
    FieldTextureAnimation {
        actor: WATER,
        texture: Setting(0),
        clock: TextureClock::Effect,
        motion: TextureMotion::Scroll {
            velocity: [-0.00125; 2],
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

pub(crate) fn profiles() -> BTreeMap<u32, Vec<FieldTextureAnimation>> {
    let mut profiles = BTreeMap::from([
        (
            ASGARD_CONVEYOR,
            vec![scroll(BACKGROUND, Setting(0), [-1. / 60., 0.])],
        ),
        (
            BALACRUF_WIND,
            vec![scroll(Setting(0), Fixed(0), [0., -0.01])],
        ),
        (MARTEL_SEAL, vec![atlas(DETAILS, Setting(0), 8, 7, false)]),
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
    ]);
    for map in [
        THODA_OUTSIDE,
        THODA_ENTRANCE,
        THODA_STAIRS,
        THODA_PUZZLE,
        THODA_SEAL,
    ] {
        let mut tracks = vec![water()];
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
        assert_eq!(profiles[&MANA_BRIDGES][0].offset(1000, 0), [0., -2.]);
        let lamps = &profiles[&MANA_LAMPS][0];
        assert_ne!(lamps.offset(7, 0), lamps.offset(0, 0));
        assert_eq!(lamps.offset(35, 0), lamps.offset(0, 0));
        assert_ne!(profiles[&THODA_OUTSIDE][0].offset(0, 100), [0.; 2]);
        Ok(())
    }
}
