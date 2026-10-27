//! Native title lighting bound directly to ordinary field UV tracks.
use anyhow::{Context, Result, ensure};
use resonance_content::field::{FieldTextureAnimation, RenderValue, TextureClock, TextureMotion};
use resonance_content::{TextureAnimation, TextureInterpolation};
use std::{collections::BTreeMap, path::Path};

const PATH: &str = "embedded/title-texture-animations.json";

pub(crate) fn cook(executable: &[u8], output: &Path) -> Result<Vec<String>> {
    crate::write_atomic(
        &output.join(PATH),
        &serde_json::to_vec(&profiles(executable)?)?,
    )?;
    Ok(vec![PATH.into()])
}

pub(crate) fn profiles(executable: &[u8]) -> Result<BTreeMap<u32, Vec<FieldTextureAnimation>>> {
    let value = |address| crate::read::f32(crate::dol::slice(executable, address, 4)?, 0);
    ensure!(
        value(0x8035B25C)? == 0. && value(0x8035B29C)? == 1.,
        "unexpected title UV counter arithmetic"
    );
    let steps = value(0x8035B354)?;
    let step_size = value(0x8035B358)?;
    let atlas = |actor, slot, address| -> Result<_> {
        Ok(sampled(
            actor,
            slot,
            0,
            TextureInterpolation::Step,
            stepped(steps, step_size, value(address)?)?,
        ))
    };
    let day = RenderValue::Fixed(resonance_content::field::scenery_actor(2));
    let night = RenderValue::Setting(0);
    let mut night_tracks = vec![
        atlas(night.clone(), 1, 0x8035B350)?,
        atlas(night, 2, 0x8035B35C)?,
    ];
    for track in &mut night_tracks {
        track.clock = TextureClock::Actor;
    }
    Ok(BTreeMap::from([
        (
            474,
            vec![
                atlas(day.clone(), 0, 0x8035B360)?,
                sampled(
                    day.clone(),
                    1,
                    1,
                    TextureInterpolation::Scroll,
                    scroll(value(0x8035B364)?)?,
                ),
                atlas(day, 2, 0x8035B368)?,
            ],
        ),
        (531, night_tracks),
    ]))
}

fn sampled(
    actor: RenderValue,
    slot: u8,
    loop_start: usize,
    interpolation: TextureInterpolation,
    offsets: Vec<[f32; 2]>,
) -> FieldTextureAnimation {
    FieldTextureAnimation {
        actor,
        texture: RenderValue::Setting(slot),
        clock: TextureClock::Field,
        control: None,
        motion: TextureMotion::Sampled {
            animation: TextureAnimation {
                texture: (),
                delay_ticks: 1,
                loop_start,
                interpolation,
                offsets,
            },
        },
    }
}

fn stepped(steps: f32, step_size: f32, interval: f32) -> Result<Vec<[f32; 2]>> {
    let integer = |value: f32| -> Result<usize> {
        ensure!(
            value.is_finite() && (1.0..=1000.0).contains(&value) && value.fract() == 0.,
            "invalid UV animation step count"
        );
        Ok(value as usize)
    };
    let steps = integer(steps)?;
    let interval = integer(interval)?;
    ensure!(
        step_size.is_finite() && step_size > 0. && step_size <= 1.,
        "invalid UV animation step size"
    );
    let period = steps.checked_mul(interval).context("UV period overflow")?;
    ensure!(period <= 36000, "UV animation period exceeds limit");
    Ok((0..period)
        .map(|tick| [0., (tick / interval) as f32 * step_size])
        .collect())
}

fn scroll(speed: f32) -> Result<Vec<[f32; 2]>> {
    ensure!(
        speed.is_finite() && (1.0 / 36000.0..=1.0).contains(&speed),
        "invalid UV animation scroll speed"
    );
    let mut offsets = vec![[0., 0.]];
    let mut position = 0f32;
    loop {
        // Upload the accumulated f32 before resetting its overshooting value.
        position += speed;
        offsets.push([position, 0.]);
        ensure!(offsets.len() <= 36000, "UV scroll period exceeds limit");
        if position > 1. {
            return Ok(offsets);
        }
    }
}
