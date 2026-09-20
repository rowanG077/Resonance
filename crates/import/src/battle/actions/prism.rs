//! Recover Prism Sword's rotated impact table and independently authored finisher.
use super::*;
use resonance_content::battle::actions::{
    lightning::GroundSpellOrigin,
    prism::{PrismBurst, PrismRecipe},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    lifetime: u16,
    origin: GroundSpellOrigin,
    presentation: StoredSpellPresentation,
    heading_offset: f32,
    effect_scale: f32,
    bursts: [PrismBurst; 7],
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    ensure!(
        technique == 94 && definition.native_id as u16 == 232 && definition.flags == 0x00440193,
        "unexpected Prism Sword menu binding"
    );
    let bundle = tables.bundle(232)?;
    ensure!(
        bundle.phases.iter().all(|phase| phase.duration == 0) && bundle.rule_count() == 2,
        "unexpected Prism Sword phases or rule table"
    );
    let p = &tables.elemental.prism;
    let recipe = PrismRecipe {
        lifetime: p.lifetime,
        origin: p.origin,
        presentation: p.presentation,
        heading_offset: p.heading_offset,
        effect_scale: p.effect_scale,
        bursts: p.bursts,
        rules: [bundle.rule(0)?, bundle.rule(1)?],
    };
    recipe.validate()?;
    let casters = recovery::casters(
        catalogue,
        tables,
        technique,
        definition,
        recovery::Release::Stored,
    )?;
    let resume = stored_resume::shared(tables, &casters)?;
    Ok(TechniqueProgram::PrismSword {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 32 * 4)?;
    for (phase, handler) in [0x8755c, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Prism Sword dispatch phase {phase}"
        );
    }
    for (start, end, hash) in [
        (
            0x871c0,
            0x8755c,
            "7963248b7d7581dea92da5c226cdbec1c3dc9bf53e47e849dd275b178a571e29",
        ),
        (
            0x8755c,
            0x87658,
            "93d4d4d42098333a5b51156212f4f56a54976146f719d64f491630b485eda38b",
        ),
        (
            0x37b10,
            0x37ca4,
            "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, start))?
                    .get(..end - start)
                    .context("truncated Prism Sword controller")?
            ) == hash,
            "unreviewed Prism Sword controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x871c0))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unreviewed Prism Sword callback or origin fallback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset: usize| half(rel.at((1, offset + 2))?, 0);
    ensure!(
        scalar(0x7f90)?.to_bits() == 1f32.to_radians().to_bits(),
        "unexpected Prism Sword heading conversion"
    );
    let start = immediate(0x87214)?;
    let end = immediate(0x87224)?;
    let final_tick = immediate(0x873d0)?;
    let mut bursts = [PrismBurst {
        tick: 0,
        offset: [0.; 3],
    }; 7];
    for (i, burst) in bursts.iter_mut().enumerate() {
        let at = 0x7f3c + i * 12;
        *burst = PrismBurst {
            tick: if i == 6 {
                final_tick
            } else {
                start + i as u16 * (end - start) / 6
            },
            offset: [scalar(at)?, scalar(at + 4)?, scalar(at + 8)?],
        };
    }
    Ok(Parameters {
        lifetime: immediate(0x875ac)?,
        origin: GroundSpellOrigin {
            height: scalar(0x1c80)?,
            nudge: 1.,
            direction_threshold: scalar(0x2800)?,
        },
        presentation: StoredSpellPresentation {
            color: rel.at((4, 0x7f38))?[..4].try_into()?,
            camera_distance: scalar(0x7f98)?,
            camera_elevation: scalar(0x7f9c)?,
        },
        heading_offset: scalar(0x7fa0)? * scalar(0x7f90)?,
        effect_scale: scalar(0x7f94)?,
        bursts,
    })
}
