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
        let (actor, texture, offsets, loop_start) = if callback == NativeCallback::CONVEYOR {
            // fn_8003DE14: the main background's selected texture scrolls left.
            (
                RenderValue::Fixed(999_996),
                RenderValue::Setting(0),
                scroll(value(0x8035B514)?, 0)?,
                1,
            )
        } else if callback == NativeCallback::ACTOR_SCROLL {
            // fn_8003D038: slot 0 selects an actor; its first texture scrolls up.
            (
                RenderValue::Setting(0),
                RenderValue::Fixed(0),
                scroll(value(0x8035B3A8)?, 1)?,
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

fn scroll(speed: f32, axis: usize) -> Result<Vec<[f32; 2]>> {
    ensure!(
        speed.is_finite() && (1. / 36000. ..=1.).contains(&speed),
        "invalid field scroll speed"
    );
    let mut offsets = vec![[0.; 2]];
    let mut phase = 0.;
    loop {
        phase += speed;
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
