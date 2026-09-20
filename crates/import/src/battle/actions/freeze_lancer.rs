//! Recover the casting ring, independent lance headings and complete stored lifetime.
use super::*;
#[cfg(test)]
use crate::battle::effect_program::{MagicArchive, magic_member};
use resonance_content::battle::actions::freeze_lancer::FreezeLancerRecipe;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    lifetime: u16,
    presentation: StoredSpellPresentation,
    effect_scale: f32,
    forward_distance: f32,
    minimum_height: f32,
    radius: f32,
    sector_angle: f32,
    order: [u8; 6],
    first_tick: u16,
    interval: u16,
    forward_threshold: f32,
    direction_threshold: f32,
    vertical_limit: f32,
    sound: u16,
}

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<FreezeLancerRecipe> {
    ensure!(
        definition.native_id == 222 && definition.flags == 0x0044018b,
        "unexpected Freeze Lancer binding"
    );
    let bundle = tables.bundle(222)?;
    ensure!(
        bundle.phase(0)?.duration == 190
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected Freeze Lancer action phases or rules"
    );
    let p = &tables.elemental.freeze_lancer;
    let recipe = FreezeLancerRecipe {
        lifetime: p.lifetime,
        presentation: p.presentation,
        effect_scale: p.effect_scale,
        forward_distance: p.forward_distance,
        minimum_height: p.minimum_height,
        radius: p.radius,
        sector_angle: p.sector_angle,
        order: p.order,
        first_tick: p.first_tick,
        interval: p.interval,
        forward_threshold: p.forward_threshold,
        direction_threshold: p.direction_threshold,
        vertical_limit: p.vertical_limit,
        sound: p.sound,
        rule: bundle.phase_rule(0, 0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 22 * 4)?;
    for (phase, handler) in [0x735f8, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Freeze Lancer dispatch"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x73344)),
        "missing Freeze Lancer callback"
    );
    for (offset, size, digest) in [
        (
            0x73344,
            0x2b4,
            "a69ab3113ce098b70fe1984c82f978d4bf07d7ff06b546aec827632d91605e7d",
        ),
        (
            0x735f8,
            0x128,
            "5d290a202d4355eb75d4a27518b533d8bf757a1913aadf1fe7f7ce6742b1f5dd",
        ),
        (
            0x37b10,
            0x194,
            "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
        ),
        (
            0x205ac,
            0x158,
            "552cf2dc842171c40e9b3d0ac01bf778c1a025d8d145bb181eec9f657c39693f",
        ),
        (
            0x4da50,
            0xa4,
            "55c714ef1017f923308ec610177f12d73d2b6ad04c32265f671c41c3c0c84dfc",
        ),
        (
            0x4daf4,
            0x58,
            "6570940cb3be69fb10e6f3d10dc0ba30340d3c2a221fbd5b2cd1713faa001678",
        ),
        (
            0x42494,
            0x158,
            "e6a07a6353a62a359ce551da0081e07658ed6be0c7130e9801259dc079b1b079",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Freeze Lancer callback")?
            ) == digest,
            "unrecovered Freeze Lancer operation at {offset:#x}"
        );
    }
    let settings = rel.at((4, 0x4b50))?;
    ensure!(
        float(settings, 0x28)? == 0. && float(settings, 0x38)? == -float(settings, 0x34)?,
        "unexpected Freeze Lancer plane or vertical clamp"
    );
    let mut order = [0; 6];
    for (index, value) in order.iter_mut().enumerate() {
        *value = u8::try_from(word(settings, 4 + index * 4)?)?;
    }
    Ok(Parameters {
        lifetime: 210,
        presentation: StoredSpellPresentation {
            color: settings[..4].try_into()?,
            camera_distance: float(settings, 0x48)?,
            camera_elevation: float(settings, 0x4c)?,
        },
        effect_scale: float(settings, 0x58)?,
        forward_distance: float(settings, 0x50)?,
        minimum_height: float(settings, 0x54)?,
        radius: float(settings, 0x24)?,
        sector_angle: float(settings, 0x1c)? * float(settings, 0x20)?,
        order,
        first_tick: 20,
        interval: 8,
        forward_threshold: float(settings, 0x30)?,
        direction_threshold: float(rel.at((4, 0x2800))?, 0)?,
        vertical_limit: float(settings, 0x34)?,
        sound: 95,
    })
}

#[cfg(test)]
mod tests;
