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
