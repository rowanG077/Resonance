//! Sample native field callbacks; their script-selected targets remain live.
use crate::field_catalogue::NativeCallback;
use anyhow::{Result, ensure};
use resonance_content::{
    TextureAnimation,
    field::{FieldTextureAnimation, RenderValue},
};
use std::collections::BTreeMap;

pub(crate) fn read(executable: &[u8]) -> Result<BTreeMap<u32, Vec<FieldTextureAnimation>>> {
    let value = |address| crate::read::f32(crate::dol::slice(executable, address, 4)?, 0);
    let mut animations = BTreeMap::new();
    for phase in crate::field_catalogue::read(executable)?.records {
        let Some(callback) = phase.render_before_objects else {
            continue;
        };
        if callback == NativeCallback::MANA_BRIDGES {
            // fn_8003C924 scrolls two selected textures on each of three
            // selected actors. Its accumulator is f32, but the step is f64.
            let speed =
                f64::from_be_bytes(crate::dol::slice(executable, 0x8035B378, 8)?.try_into()?);
            let offsets = scroll(speed, 1)?;
            let mut tracks = Vec::new();
            for actor in 2..=4 {
                for texture in 0..=1 {
                    tracks.push(FieldTextureAnimation {
                        actor: RenderValue::Setting(actor),
                        motion: TextureAnimation {
                            texture: RenderValue::Setting(texture),
                            delay_ticks: 0,
                            loop_start: 1,
                            offsets: offsets.clone(),
                        },
                    });
                }
            }
            animations.insert(phase.id as u32, tracks);
            continue;
        }
        let (actor, texture, offsets, loop_start) = if callback == NativeCallback::CONVEYOR {
            // fn_8003DE14: the main background's selected texture scrolls left.
            (
                RenderValue::Fixed(999_996),
                RenderValue::Setting(0),
                scroll(f64::from(value(0x8035B514)?), 0)?,
                1,
            )
        } else if callback == NativeCallback::ACTOR_SCROLL {
            // fn_8003D038: slot 0 selects an actor; its first texture scrolls up.
            (
                RenderValue::Setting(0),
                RenderValue::Fixed(0),
                scroll(f64::from(value(0x8035B3A8)?), 1)?,
                1,
            )
        } else if callback == NativeCallback::MARTEL_SEAL {
            // fn_800393F8 advances the third scenery layer's eight-frame atlas.
            let interval = value(0x8035B368)?;
            let frames = value(0x8035B350)?;
            ensure!(
                interval > 0. && interval.fract() == 0. && frames > 0. && frames.fract() == 0.,
                "invalid field atlas timing"
            );
            let period = (interval * frames) as usize;
            ensure!(period <= 36000, "field atlas period exceeds limit");
            let step = value(0x8035B36C)?;
            (
                RenderValue::Fixed(999_998),
                RenderValue::Setting(0),
                (0..period)
                    .map(|t| [0., (t / interval as usize) as f32 * step])
                    .collect(),
                0,
            )
        } else {
            continue;
        };
        let motion = TextureAnimation {
            texture,
            delay_ticks: 0,
            loop_start,
            offsets,
        };
        motion.validate()?;
        animations.insert(
            phase.id as u32,
            vec![FieldTextureAnimation { actor, motion }],
        );
    }
    Ok(animations)
}

fn scroll(speed: f64, axis: usize) -> Result<Vec<[f32; 2]>> {
    ensure!(
        speed.is_finite() && (1. / 36000. ..=1.).contains(&speed),
        "invalid field scroll speed"
    );
    let mut offsets = vec![[0.; 2]];
    let mut phase = 0.;
    loop {
        phase = (f64::from(phase) + speed) as f32;
        let mut offset = [0.; 2];
        offset[axis] = -phase;
        offsets.push(offset);
        ensure!(offsets.len() <= 36000, "field scroll period exceeds limit");
        if phase > 1. {
            break;
        }
    }
    Ok(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original executable catalogues"]
    fn mana_bridges_scroll_both_textures_on_all_three_scripted_actors() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let executable = std::fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let animations = read(&executable)?;
            let tracks = animations.get(&366).expect("Mana bridge callback");
            assert_eq!(tracks.len(), 6);
            let settings = [(0, 7), (1, 11), (2, 6000), (3, 6001), (4, 6002)].into();
            for (index, track) in tracks.iter().enumerate() {
                track.motion.validate()?;
                assert_eq!(track.actor.resolve(&settings), 6000 + index as i32 / 2);
                assert_eq!(track.motion.texture.resolve(&settings), [7, 11][index % 2]);
                assert_eq!(track.motion.offset(1), [0., -0.002]);
                assert!((track.motion.offset(250)[1] + 0.5).abs() < 0.00001);
                // The native f32 accumulator overshoots on update 501 and
                // uploads that value before resetting. It skips zero on wrap.
                assert_eq!(track.motion.offsets.len(), 502);
                assert!(track.motion.offset(501)[1] < -1.);
                assert_eq!(track.motion.offset(502), track.motion.offset(1));
            }
        }
        Ok(())
    }
}
