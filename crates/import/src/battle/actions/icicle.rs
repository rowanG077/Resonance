//! Recover the two ordinary ground contacts and their shared visual program.
use super::*;
use resonance_content::battle::{
    actions::icicle::{IciclePulse, IcicleRecipe},
    effects::{EffectBank, EffectId, KnockbackDirection},
};

pub(super) fn cook(
    parameters: &ordinary_parameters::GroundContact,
    source: &bundle::Bundle,
    definition: &Definition,
    mut contact: ProjectileRecipe,
) -> Result<IcicleRecipe> {
    ensure!(
        definition.native_id == 220 && definition.flags == 0x00440186,
        "unexpected Icicle binding"
    );
    ensure!(
        source
            .phases
            .iter()
            .skip(1)
            .all(|phase| phase.duration == 0)
            && source.rule_count() == 2,
        "unexpected Icicle phase/rule closure"
    );
    contact.birth_bank = Some(EffectBank::Techniques);
    contact.lifetime = 10;
    contact.shape.kind = HitShapeKind::Cylinder;
    contact.knockback = KnockbackDirection::Velocity;
    contact.shape.radius = parameters.radius;
    contact.shape.height = parameters.height;
    let recipe = IcicleRecipe {
        lifetime: source.phases[0].duration,
        origin: parameters.origin,
        effect: EffectId {
            bank: EffectBank::Techniques,
            id: 32,
        },
        effect_scale: parameters.effect_scale,
        contact,
        pulses: [
            IciclePulse {
                tick: 2,
                reaction: 10,
                rule: source.rule(0)?,
            },
            IciclePulse {
                tick: 26,
                reaction: 7,
                rule: source.rule(1)?,
            },
        ],
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn validate_controller(rel: &Rel) -> Result<()> {
    let dispatch = rel.pointer(DATA, 0x1238 + 20 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x721e0)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48)
            && rel.local_targets().contains(&(1, 0x72100)),
        "unexpected Icicle dispatch"
    );
    for (offset, size, digest) in [
        (
            0x72100,
            0xe0,
            "a5fb329a4a552168c0b001c02603ca156799094a94397214cfe2ad4ee89bcb1b",
        ),
        (
            0x721e0,
            0xa0,
            "b439155c42edf199118e7f7ae95a0908491d9169fdb580349ce6011c9755792b",
        ),
        (
            0x204c8,
            0xe4,
            "0b0b3660c1fed0b113e511b21f06024e5197ad0782c1d46399b10ae5eef04901",
        ),
        (
            0x37ed0,
            0x104,
            "be2bbe2ecebd4d1c73aded956430fd0410c293984067b2c23334721a992eafec",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Icicle callback")?
            ) == digest,
            "unrecovered Icicle callback operation at {offset:#x}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
