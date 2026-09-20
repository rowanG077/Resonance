//! Recover the shared ground pulse controller with each spell's authored rule.
use super::*;
use resonance_content::battle::actions::{
    ground_pulse::{GroundPulseRecipe, GroundPulseSpell},
    lightning::GroundSpellOrigin,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    kind: GroundPulseSpell,
    lifetime: u16,
    pulse_tick: u16,
    effect_scale: f32,
    presentation: StoredSpellPresentation,
    origin: GroundSpellOrigin,
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let p = tables
        .elemental
        .ground_pulses
        .iter()
        .find(|p| p.kind as u16 == definition.native_id as u16)
        .context("missing cooked ground pulse controller")?;
    ensure!(
        technique == p.kind.menu() && definition.flags == 0x00440193,
        "unexpected ground pulse menu binding"
    );
    let bundle = tables.bundle(p.kind as u16)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected ground pulse action phases or rule table"
    );
    let recipe = GroundPulseRecipe {
        kind: p.kind,
        lifetime: p.lifetime,
        pulse_tick: p.pulse_tick,
        effect_scale: p.effect_scale,
        presentation: p.presentation,
        origin: p.origin,
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
    Ok(TechniqueProgram::GroundPulse {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, initializer, callback, settings, init_hash, callback_hash) = match native {
        223 => (
            GroundPulseSpell::RagingMist,
            0x82f78,
            0x82f10,
            0x7170,
            "30caa976c4cc429328b2e70af6cc951cead96f93ff6c4f09820cf60efafd3d72",
            "6192d1473186174d4ef3b512262b5627d51e432058fe57035bbf378971cea9a5",
        ),
        224 => (
            GroundPulseSpell::DreadedWave,
            0x84158,
            0x840f0,
            0x7700,
            "caa8363d2fc29b16caea7afeb777b88d2c049d777b029228140165fffc8b7a73",
            "733d0a32c9df551920b7a9dcf5509563e3be93c12791fc47011ae853e53c3c63",
        ),
        228 => (
            GroundPulseSpell::GravityWell,
            0x82750,
            0x826e8,
            0x7010,
            "4e7d5fd68fc47663a7979505fc39e36004f8c66916cbe11a23b6675d350caac3",
            "ebe47e7f7ce0b0a2847c4ba399ca67aff8f82b3185e59aac8505c39ada5f9b8c",
        ),
        229 => (
            GroundPulseSpell::Atlas,
            0x89ad4,
            0x89a6c,
            0x84b8,
            "3fb52b6511eea493a1f1434be748d658d6831bbbbf5a4f08d0f158f1a80652d0",
            "6242e0d0c62a808a47d445c5ee9afb82e4e23c176b11ec8fcef677d85fbd1529",
        ),
        native => bail!("unsupported ground pulse native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(kind as u16 - 200) * 4)?;
    for (phase, handler) in [initializer, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected ground pulse dispatch phase {phase}"
        );
    }
    for (start, size, hash) in [
        (
            initializer,
            if kind == GroundPulseSpell::Atlas {
                0x114
            } else {
                0xe8
            },
            init_hash,
        ),
        (callback, 0x68, callback_hash),
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
                    .context("truncated ground pulse controller")?
            ) == hash,
            "unreviewed ground pulse controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, callback))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unreviewed ground pulse callback or origin fallback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    Ok(Parameters {
        kind,
        lifetime: half(rel.at((1, initializer + 0x50))?, 2)?,
        pulse_tick: half(rel.at((1, callback + 0x18))?, 2)?,
        effect_scale: scalar(
            settings
                + if kind == GroundPulseSpell::Atlas {
                    16
                } else {
                    12
                },
        )?,
        presentation: StoredSpellPresentation {
            color: rel.at((4, settings))?[..4].try_into()?,
            camera_distance: scalar(settings + 4)?,
            camera_elevation: scalar(settings + 8)?,
        },
        origin: GroundSpellOrigin {
            // Atlas replaces the shared anchor with the retained target body center.
            height: scalar(if kind == GroundPulseSpell::Atlas {
                0x84c4
            } else {
                0x1c80
            })?,
            nudge: if kind == GroundPulseSpell::Atlas {
                0.
            } else {
                1.
            },
            direction_threshold: scalar(0x2800)?,
        },
    })
}
