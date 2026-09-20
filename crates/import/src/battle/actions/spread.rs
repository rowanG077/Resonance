//! Decode the stored water spell independently of its party or enemy caster.
use super::*;
use resonance_content::battle::{
    actions::spread::SpreadRecipe,
    effects::{EffectBank, EffectId},
};

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<SpreadRecipe> {
    const NATIVE: u16 = 201;
    ensure!(
        arte.native_id as u16 == NATIVE && arte.flags == 0x0044018b,
        "unexpected Spread technique binding"
    );
    let bundle = tables.bundle(NATIVE)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0),
        "unexpected Spread action bundle"
    );
    let parameters = &tables.elemental.spread;
    let pulse = parameters.single_pulse()?;
    let recipe = SpreadRecipe {
        // The initializer replaces the action bundle's 180-tick duration.
        lifetime: parameters.lifetime,
        projectile_tick: pulse.tick,
        rule: bundle.phase_rule(0, pulse.rule)?,
        projectile: EffectId {
            bank: EffectBank::Magic(1),
            id: pulse.projectile,
        },
        effect: EffectId {
            bank: EffectBank::Magic(1),
            id: 1,
        },
        presentation: parameters.presentation,
        target_height: parameters.origin.height,
        target_nudge_distance: parameters.origin.nudge,
        target_nudge_threshold: parameters.origin.direction_threshold,
        effect_scale: parameters.effect_scale,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 4)?;
    for (phase, handler) in [0x80990, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Spread phase {phase} implementation"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|axis| float(fallback, axis * 4).ok() == Some(0.)),
        "unsupported stored spell target fallback"
    );
    stored_parameters::Ground::read(rel, 0x6d98, 170, &[(45, 1, 0)])
}
