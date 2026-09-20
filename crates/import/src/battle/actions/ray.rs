//! Recover Ray's retained heading and nine authored contact positions.
use super::*;
use resonance_content::battle::actions::{
    lightning::GroundSpellOrigin,
    ray::{RayBurst, RayRecipe},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    lifetime: u16,
    origin: GroundSpellOrigin,
    presentation: StoredSpellPresentation,
    heading_offset: f32,
    effect_scale: f32,
    bursts: [RayBurst; 9],
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    ensure!(
        technique == 114 && definition.native_id as u16 == 252 && definition.flags == 0x00840193,
        "unexpected Ray menu binding"
    );
    let bundle = tables.bundle(252)?;
    ensure!(
        bundle.phases.iter().all(|phase| phase.duration == 0) && bundle.rule_count() == 1,
        "unexpected Ray phases or rule table"
    );
    let p = &tables.elemental.ray;
    let recipe = RayRecipe {
        lifetime: p.lifetime,
        origin: p.origin,
        presentation: p.presentation,
        heading_offset: p.heading_offset,
        effect_scale: p.effect_scale,
        bursts: p.bursts,
        rule: bundle.rule(0)?,
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
    Ok(TechniqueProgram::Ray {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 52 * 4)?;
    for (phase, handler) in [0x86efc, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Ray dispatch phase {phase}"
        );
    }
    for (start, end, hash) in [
        (
            0x86d2c,
            0x86efc,
            "f097f319a59912ffc86f24a2042b171fc81082990b60af7789833a2f48e3f9cb",
        ),
        (
            0x86efc,
            0x86ff8,
            "b8f6e696ec07b861022c545ef0f9b052a21abbd1810f2e73c4ca1910bc89217d",
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
                    .context("truncated Ray controller")?
            ) == hash,
            "unreviewed Ray controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x86d2c))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unreviewed Ray callback or origin fallback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset: usize| half(rel.at((1, offset + 2))?, 0);
    ensure!(
        scalar(0x7e18)?.to_bits() == 1f32.to_radians().to_bits(),
        "unexpected Ray heading conversion"
    );
    let start = immediate(0x86d8c)?;
    let end = immediate(0x86d9c)?;
    // The guarded callback divides the nine-entry interval into eight-tick steps.
    let mut bursts = [RayBurst {
        tick: 0,
        offset: [0.; 3],
    }; 9];
    for (i, burst) in bursts.iter_mut().enumerate() {
        let at = 0x7dac + i * 12;
        *burst = RayBurst {
            tick: start + i as u16 * (end - start) / 9,
            offset: [scalar(at)?, scalar(at + 4)?, scalar(at + 8)?],
        };
    }
    Ok(Parameters {
        lifetime: immediate(0x86f4c)?,
        origin: GroundSpellOrigin {
            height: scalar(0x1c80)?,
            nudge: 1.,
            direction_threshold: scalar(0x2800)?,
        },
        presentation: StoredSpellPresentation {
            color: rel.at((4, 0x7da8))?[..4].try_into()?,
            camera_distance: scalar(0x7e20)?,
            camera_elevation: scalar(0x7e24)?,
        },
        heading_offset: scalar(0x7e28)? * scalar(0x7e18)?,
        effect_scale: scalar(0x7e1c)?,
        bursts,
    })
}
