//! Recover the captured forward launch point and travelling fire projectile.
use super::*;
use resonance_content::battle::actions::spiral_flare::SpiralFlareRecipe;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    lifetime: u16,
    pulse_tick: u16,
    effect_scale: f32,
    presentation: StoredSpellPresentation,
    forward_distance: f32,
    height: f32,
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    ensure!(
        technique == 88 && definition.native_id as u16 == 226 && definition.flags == 0x00440193,
        "unexpected Spiral Flare menu binding"
    );
    let bundle = tables.bundle(226)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected Spiral Flare action phases or rule table"
    );
    let p = &tables.elemental.spiral_flare;
    let recipe = SpiralFlareRecipe {
        lifetime: p.lifetime,
        pulse_tick: p.pulse_tick,
        effect_scale: p.effect_scale,
        presentation: p.presentation,
        forward_distance: p.forward_distance,
        height: p.height,
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
    Ok(TechniqueProgram::SpiralFlare {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 26 * 4)?;
    for (phase, handler) in [0x8530c, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Spiral Flare dispatch phase {phase}"
        );
    }
    for (start, size, hash) in [
        (
            0x8530c,
            0x120,
            "710925da9cfec5aaaa7caff07de4ccf79b560fdf924196864349aecfc143e2d7",
        ),
        (
            0x852a4,
            0x68,
            "7b2053c2924f1e1fa26583a9a21090dc36f890c9483a11404c0b603db3c191ed",
        ),
        (
            0x37b10,
            0x194,
            "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, start))?
                    .get(..size)
                    .context("truncated Spiral Flare controller")?
            ) == hash,
            "unreviewed Spiral Flare controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x852a4)),
        "unreviewed Spiral Flare callback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    Ok(Parameters {
        lifetime: half(rel.at((1, 0x8535c))?, 2)?,
        pulse_tick: half(rel.at((1, 0x852bc))?, 2)?,
        effect_scale: scalar(0x7a14)?,
        presentation: StoredSpellPresentation {
            color: rel.at((4, 0x7a00))?[..4].try_into()?,
            camera_distance: scalar(0x7a04)?,
            camera_elevation: scalar(0x7a08)?,
        },
        forward_distance: scalar(0x7a0c)?,
        height: scalar(0x7a10)?,
    })
}
