//! Shared lance choreography with independently recovered Light and Darkness payloads.
use super::*;
use resonance_content::battle::actions::{
    lance::{LanceRecipe, LanceRing, LanceSpell},
    lightning::GroundSpellOrigin,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    kind: LanceSpell,
    lifetime: u16,
    presentation: StoredSpellPresentation,
    effect_scale: f32,
    origin: GroundSpellOrigin,
    ring: [LanceRing; 4],
    ring_radius: f32,
    ring_projectile_height: f32,
    final_tick: u16,
    final_effect_height: f32,
    final_projectile_height: f32,
    target_height: f32,
    afterglow_tick: u16,
    afterglow_height: f32,
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let native = definition.native_id as u16;
    let flags = match native {
        253 => 0x00840193,
        283 => 0x00840187,
        _ => bail!("unsupported lance native {native}"),
    };
    let p = tables
        .elemental
        .lances
        .iter()
        .find(|p| p.kind as u16 == native)
        .context("missing cooked lance controller")?;
    ensure!(
        technique == p.kind.menu() && definition.flags == flags,
        "unexpected lance menu binding"
    );
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phases.iter().all(|phase| phase.duration == 0) && bundle.rule_count() == 2,
        "unexpected lance phases or rule table"
    );
    let recipe = LanceRecipe {
        kind: p.kind,
        lifetime: p.lifetime,
        presentation: p.presentation,
        effect_scale: p.effect_scale,
        origin: p.origin,
        ring: p.ring,
        ring_radius: p.ring_radius,
        ring_projectile_height: p.ring_projectile_height,
        final_tick: p.final_tick,
        final_effect_height: p.final_effect_height,
        final_projectile_height: p.final_projectile_height,
        target_height: p.target_height,
        afterglow_tick: p.afterglow_tick,
        afterglow_height: p.afterglow_height,
        rules: [bundle.rule(0)?, bundle.rule(1)?],
    };
    recipe.validate()?;
    let (casters, resume) = if p.kind == LanceSpell::HolyLance {
        let casters = recovery::casters(
            catalogue,
            tables,
            technique,
            definition,
            recovery::Release::Stored,
        )?;
        let resume = stored_resume::shared(tables, &casters)?;
        (casters, Some(resume))
    } else {
        // The event template has no party release binding; enemy casts own their admission.
        (Vec::new(), None)
    };
    Ok(TechniqueProgram::Lance {
        casters,
        resume,
        recipe,
    })
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, color) = match native {
        253 => (LanceSpell::HolyLance, 0x50a8),
        283 => (LanceSpell::BloodyLance, 0x50ac),
        _ => bail!("unsupported lance native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [0x75144, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected lance dispatch phase {phase}"
        );
    }
    // Bind the constants and compact schedules to the complete reviewed controller bodies.
    for (start, end, hash) in [
        (
            0x74bf8,
            0x75144,
            "71de423764d40b55b77e5a0dcd8acaf871908bab27d09e0f4a3b26941dd02484",
        ),
        (
            0x75144,
            0x75278,
            "20225f046d30dd95a52e947d6adada1b7bdec4a8115604798263ef59c2b1e918",
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
                    .context("truncated lance controller")?
            ) == hash,
            "unreviewed lance controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x74bf8))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unreviewed lance callback or origin fallback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset: usize| half(rel.at((1, offset + 2))?, 0);
    // Y rotation has the engine's positive yaw convention; actor headings are already radians.
    ensure!(
        scalar(0x50cc)?.to_bits() == 1f32.to_radians().to_bits(),
        "unexpected lance heading conversion"
    );
    // Both four-entry loops divide by ten; the full callback guard above covers that arithmetic.
    let interval = immediate(0x74c58)? / 4;
    let start = immediate(0x74dbc)?;
    let mut ring = [LanceRing {
        angle: 0.,
        marker_tick: 0,
        projectile_tick: 0,
    }; 4];
    for (i, point) in ring.iter_mut().enumerate() {
        *point = LanceRing {
            angle: scalar(0x50b0 + i * 4)? * scalar(0x50c0)?,
            marker_tick: i as u16 * interval,
            projectile_tick: start + i as u16 * interval,
        };
    }
    Ok(Parameters {
        kind,
        lifetime: immediate(0x751ac)?,
        presentation: StoredSpellPresentation {
            color: rel.at((4, color))?[..4].try_into()?,
            camera_distance: scalar(0x50e8)?,
            camera_elevation: scalar(0x50ec)?,
        },
        effect_scale: scalar(0x50d0)?,
        origin: GroundSpellOrigin {
            height: scalar(0x1c80)?,
            nudge: 1.,
            direction_threshold: scalar(0x2800)?,
        },
        ring,
        ring_radius: scalar(0x50c4)?,
        ring_projectile_height: scalar(0x50d4)?,
        final_tick: immediate(0x74f8c)?,
        final_effect_height: scalar(0x50dc)?,
        final_projectile_height: scalar(0x50e0)?,
        target_height: scalar(0x50d8)?,
        afterglow_tick: immediate(0x75090)?,
        afterglow_height: scalar(0x50e4)?,
    })
}
