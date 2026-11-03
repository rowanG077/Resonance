//! Native effect models use half-range RGB; the shared GC model pass uses white.
use super::*;

pub(crate) fn normalize(
    source: &Source,
    library: &Path,
    descriptor: &str,
    scene: &resonance_content::ScenePart,
    model: &mut model::Model,
) -> Result<()> {
    let stem = descriptor
        .rsplit_once('.')
        .context("effect model has no extension")?
        .0;
    let (tints, _, _) =
        model::material_parameters(&source.require(&format!("{stem}.mxd"))?, &model.materials)?;
    for mesh in &mut model.meshes {
        for color in &mut mesh.colors {
            // Steam's neutral effect vertex is RGB128 (expanded source colors
            // can be129), whereas the GC effect mesh has RGB255. Opacity is
            // already full-range, including the authored edge gradients.
            for channel in &mut color[..3] {
                *channel = (*channel * 2.).min(1.);
            }
        }
    }
    output::write(&library.join(&scene.mesh), &model.glb(&tints)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; publishes only enemy effect banks"]
    fn installed_publish_enemy_effect_category() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        super::super::cook_enemies(&source, &library, None)
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; publishes only resident and enemy effect categories"]
    fn installed_publish_effect_model_categories() -> Result<()> {
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let library = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        super::super::cook_resident(&source, &library)?;
        eprintln!("Published both resident effect banks and their thirteen normalized model slots");
        super::super::cook_enemies(&source, &library, None)?;
        eprintln!("Published the enemy-local effect category; no read-all or GPU pass requested");
        Ok(())
    }
}
