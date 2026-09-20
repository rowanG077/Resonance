//! Recover the retained ground origin and one late, persistent vortex contact.
use super::*;
#[cfg(test)]
use crate::battle::effect_program::{MagicArchive, magic_member};
use resonance_content::battle::actions::ice_tornado::IceTornadoRecipe;

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<IceTornadoRecipe> {
    ensure!(
        definition.native_id == 221 && definition.flags == 0x0044018b,
        "unexpected Ice Tornado binding"
    );
    let bundle = tables.bundle(221)?;
    ensure!(
        bundle.phase(0)?.duration == 190
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected Ice Tornado phases or hit rules"
    );
    let parameters = &tables.elemental.ice_tornado;
    let pulse = parameters.single_pulse()?;
    let recipe = IceTornadoRecipe {
        lifetime: parameters.lifetime,
        origin: parameters.origin,
        presentation: parameters.presentation,
        effect_scale: parameters.effect_scale,
        projectile_tick: pulse.tick,
        rule: bundle.phase_rule(0, pulse.rule)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 21 * 4)?;
    for (phase, handler) in [0x7bd14, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Ice Tornado dispatch"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x7bcac)),
        "missing Ice Tornado callback"
    );
    for (offset, size, digest) in [
        (
            0x7bcac,
            0x68,
            "4bcf7ffca8a199085b193fd258c7aba49290116e7549ead26e9f0bddc892ab44",
        ),
        (
            0x7bd14,
            0xe8,
            "b4b9882da418e0288e7f87458c13ebfcd0708e07fb943ef253ec61e3871d0a93",
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
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Ice Tornado callback")?
            ) == digest,
            "unrecovered Ice Tornado operation at {offset:#x}"
        );
    }
    stored_parameters::Ground::read(rel, 0x5948, 200, &[(45, 1, 0)])
}

#[cfg(test)]
mod tests;
