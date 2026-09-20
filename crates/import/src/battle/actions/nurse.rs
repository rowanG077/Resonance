//! Nurse captures the caster's heading, creates the roster models, then heals at age 120.
use super::*;
use resonance_content::battle::actions::nurse::NurseRecipe;

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    row: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    ensure!(
        row.native_id as u16 == 237 && row.flags == 0x0088028b,
        "unexpected Nurse identity or flags"
    );
    let casters = recovery::casters(catalogue, tables, technique, row, recovery::Release::Stored)?;
    let resume = stored_resume::shared(tables, &casters)?;
    Ok(TechniqueProgram::Nurse {
        casters,
        resume,
        recipe: tables.recovery.nurse.clone(),
    })
}

pub(super) fn read_parameters(rel: &Rel) -> Result<NurseRecipe> {
    let dispatch = rel.pointer(DATA, 0x1238 + 37 * 4)?;
    for (slot, function) in [(0, 0x60694), (4, 0x37e48), (8, 0x37dd8)] {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + slot)? == (1, function),
            "unexpected Nurse dispatch"
        );
    }
    // These operands guard the only callback, resource IDs, roster loop and recovery contract.
    for (address, instruction) in [
        (0x606f0, 0x38c000fa),
        (0x6072c, 0x38a00001),
        (0x607b0, 0x381f0002),
        (0x60528, 0x2c000078),
        (0x605a0, 0x38a00006),
        (0x605e4, 0x38800028),
        (0x60648, 0x3880002a),
    ] {
        ensure!(
            word(rel.at((1, address))?, 0)? == instruction,
            "changed Nurse callback at {address:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x60510)),
        "missing Nurse active callback"
    );
    let settings = rel.at((4, 0x36c8))?;
    ensure!(
        float(settings, 12)? == 0. && float(settings, 16)? == 1.,
        "unsupported Nurse model origin or scale"
    );
    let recipe = NurseRecipe {
        lifetime: 250,
        recovery_tick: 120,
        percent: 40,
        first_heading: float(settings, 20)?,
        heading_step: float(settings, 24)?,
        presentation: StoredSpellPresentation {
            color: settings[..4].try_into()?,
            camera_distance: float(settings, 4)?,
            camera_elevation: float(settings, 8)?,
        },
    };
    recipe.validate()?;
    Ok(recipe)
}

#[cfg(test)]
mod tests;
