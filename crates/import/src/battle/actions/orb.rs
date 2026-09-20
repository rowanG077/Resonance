//! Photon and Dark Sphere share a controller, with distinct banks, rules and scene colors.
use super::*;
use resonance_content::battle::actions::orb::{OrbPulse, OrbRecipe, OrbSpell};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    kind: OrbSpell,
    lifetime: u16,
    presentation: StoredSpellPresentation,
    effect_scale: f32,
    pulse_ticks: [u16; 2],
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let native = definition.native_id as u16;
    let flags = match native {
        251 => 0x0084018b,
        278 => 0x00840187,
        _ => bail!("unsupported orb spell {native}"),
    };
    ensure!(definition.flags == flags, "unexpected orb spell binding");
    let p = tables
        .elemental
        .orbs
        .iter()
        .find(|p| p.kind as u16 == native)
        .context("missing cooked orb controller")?;
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 2,
        "unexpected orb phase or rule table"
    );
    let recipe = OrbRecipe {
        kind: p.kind,
        lifetime: p.lifetime,
        presentation: p.presentation,
        effect_scale: p.effect_scale,
        pulses: [
            OrbPulse {
                tick: p.pulse_ticks[0],
                rule: bundle.rule(0)?,
            },
            OrbPulse {
                tick: p.pulse_ticks[1],
                rule: bundle.rule(1)?,
            },
        ],
    };
    recipe.validate()?;
    let casters = if p.kind == OrbSpell::Photon {
        recovery::casters(
            catalogue,
            tables,
            technique,
            definition,
            recovery::Release::Stored,
        )?
    } else {
        // Enemy actions own Dark Sphere's chanting; the event-only party template has no release voice.
        Vec::new()
    };
    let resume = if casters.is_empty() {
        stored_resume::character(tables, 4)?
    } else {
        stored_resume::shared(tables, &casters)?
    };
    Ok(TechniqueProgram::Orb {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, color) = match native {
        251 => (OrbSpell::Photon, 0x6990),
        278 => (OrbSpell::DarkSphere, 0x6994),
        _ => bail!("unsupported orb spell {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [0x8042c, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected orb phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x80354)),
        "missing orb callback"
    );
    for (offset, instruction) in [
        (0x80380, 0x2c00000a),
        (0x80390, 0x807e195c),
        (0x803ac, 0x38600002),
        (0x803b0, 0x38e00001),
        (0x803cc, 0x2c00003c),
        (0x803d4, 0x807e195c),
        (0x803e8, 0x391f001c),
        (0x803f0, 0x38600002),
        (0x803f4, 0x38e00002),
        (0x8045c, 0x2c000116),
        (0x80494, 0x38c000a5),
        (0x804c8, 0x38c000a5),
        (0x804e8, 0x80a7195c),
        (0x804fc, 0x3907195c),
        (0x80504, 0x38a00001),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected orb callback at {offset:#x}"
        );
    }
    Ok(Parameters {
        kind,
        // The initializer replaces the bundle's duration before the active clock runs.
        lifetime: half(rel.at((1, 0x80494 + 2))?, 0)?,
        presentation: StoredSpellPresentation {
            color: rel.at((4, color))?[..4].try_into()?,
            camera_distance: float(rel.at((4, 0x6998))?, 0)?,
            camera_elevation: float(rel.at((4, 0x699c))?, 0)?,
        },
        effect_scale: float(rel.at((4, 0x69a0))?, 0)?,
        pulse_ticks: [
            half(rel.at((1, 0x80380))?, 2)?,
            half(rel.at((1, 0x803cc))?, 2)?,
        ],
    })
}
