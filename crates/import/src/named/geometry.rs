use super::*;
use resonance_content::physical_scene::{Scene, TextureCatalogue, TextureSource};

#[cfg(test)]
mod tests;

/// Prepared physical material recipe and its optional local palette catalogue.
#[derive(Clone)]
pub struct PhysicalScene {
    pub scene: Scene,
    pub textures: Option<TextureCatalogue>,
}

impl PhysicalScene {
    pub fn capture(snapshot: &mut library::Snapshot, path: &str) -> Result<Self> {
        let scene: Scene = snapshot.json(path, 64 * 1024 * 1024)?;
        scene.validate()?;
        let textures = match &scene.textures {
            TextureSource::Local { catalogue } => Some(snapshot.json(catalogue, 64 * 1024 * 1024)?),
            _ => None,
        };
        Ok(Self { scene, textures })
    }
}

impl Publisher<'_> {
    /// Preserve physical geometry whose renderer binding needs caller state.
    /// Its meshes, recipes and complete palette pages remain owned and editable.
    pub(crate) fn physical_scene(
        &mut self,
        prepared: PhysicalScene,
        owner: &str,
    ) -> Result<String> {
        let PhysicalScene {
            mut scene,
            textures,
        } = prepared;
        scene.validate().with_context(|| owner.to_owned())?;
        let mesh = format!("{owner}/mesh.glb");
        self.copy(&scene.mesh, &mesh)?;
        scene.mesh = mesh;
        if let TextureSource::Local { catalogue } = &mut scene.textures {
            let textures = textures.context("physical geometry has no local texture catalogue")?;
            textures.validate().with_context(|| catalogue.clone())?;
            *catalogue = self.physical_textures(textures, &format!("{owner}/textures"))?;
        } else {
            ensure!(
                textures.is_none(),
                "caller geometry supplied a local texture catalogue"
            );
        }
        let path = format!("{owner}/scene.json");
        self.json(&path, &scene)?;
        Ok(path)
    }

    pub(crate) fn physical_textures(
        &mut self,
        mut textures: TextureCatalogue,
        owner: &str,
    ) -> Result<String> {
        textures.validate()?;
        for (index, texture) in textures.textures.iter_mut().enumerate() {
            let Some(texture) = texture else { continue };
            for (palette, image) in texture.images.iter_mut().enumerate() {
                *image =
                    self.image(image, &format!("{owner}/texture-{index}-palette-{palette}"))?;
            }
        }
        let path = format!("{owner}/textures.json");
        self.json(&path, &textures)?;
        Ok(path)
    }
}
