use super::*;
use resonance_content::effect::{FieldEffects, OverlayArt};

/// Typed field artwork retained between native binding and terminal publication.
#[derive(Clone)]
pub struct FieldArt {
    pub effects: std::sync::Arc<FieldEffects>,
    pub overlays: BTreeMap<i32, OverlayArt>,
    pub unbound: BTreeMap<u32, Vec<super::PhysicalScene>>,
}

impl FieldArt {
    /// Authoring tools may republish current content through the same typed path.
    pub fn capture(snapshot: &mut library::Snapshot, field: &FieldAssets) -> Result<Self> {
        let effects = std::sync::Arc::new(snapshot.json(&field.effects, 64 * 1024 * 1024)?);
        let overlays = field
            .overlays
            .iter()
            .map(|(&id, path)| Ok((id, snapshot.json(path, 64 * 1024 * 1024)?)))
            .collect::<Result<_>>()?;
        let unbound = field
            .unbound_geometry
            .iter()
            .map(|geometry| {
                let scenes = geometry
                    .scenes
                    .iter()
                    .map(|path| super::PhysicalScene::capture(snapshot, path))
                    .collect::<Result<_>>()?;
                Ok((geometry.resource, scenes))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            effects,
            overlays,
            unbound,
        })
    }
}

impl Publisher<'_> {
    pub(super) fn field_art(
        &mut self,
        root: &str,
        field: &mut FieldAssets,
        art: FieldArt,
    ) -> Result<()> {
        let shared = "effects/field";
        field.contact_shadow.texture = self.image(
            &field.contact_shadow.texture,
            &format!("{shared}/textures/contact-shadow"),
        )?;
        field.toon_ramp = self.image(&field.toon_ramp, &format!("{shared}/textures/toon-ramp"))?;

        let mut effects = (*art.effects).clone();
        effects.validate()?;
        // An atlas is deliberately shared within this artwork collection. Its
        // first semantic use names it; unrelated field-owned artwork never
        // shares a destination simply because its pixels happen to match.
        let mut atlases = BTreeMap::new();
        for (name, texture) in [
            ("emotes", &mut effects.emote_texture),
            ("status", &mut effects.status_texture),
            ("refraction", &mut effects.refraction.sprite.texture),
            ("air-refraction", &mut effects.air_refraction.texture),
        ] {
            *texture = self.atlas(&mut atlases, texture, &format!("{shared}/textures/{name}"))?;
        }
        for (id, sprite) in &mut effects.sprites {
            sprite.texture = self.atlas(
                &mut atlases,
                &sprite.texture,
                &format!("{shared}/textures/sprites-{id:04}"),
            )?;
        }
        field.effects = format!("{shared}/effects.json");
        self.json(&field.effects, &effects)?;

        for (id, recipe) in &mut field.particles {
            recipe.texture =
                self.image(&recipe.texture, &format!("{root}/textures/particle-{id}"))?;
        }
        field.overlays.clear();
        for (id, mut art) in art.overlays {
            let owner = format!("{root}/overlays/overlay-{id}");
            art.validate()?;
            for (texture, entry) in art.textures.iter_mut().enumerate() {
                for (palette, image) in entry.images.iter_mut().enumerate() {
                    image.path = self.image(
                        &image.path,
                        &format!("{owner}/texture-{texture}-palette-{palette}"),
                    )?;
                }
            }
            let path = format!("{owner}/overlay.json");
            self.json(&path, &art)?;
            field.overlays.insert(id, path);
        }
        field.unbound_geometry.clear();
        for (resource, scenes) in art.unbound {
            ensure!(
                !scenes.is_empty(),
                "physical field resource {resource:#x} has no layers"
            );
            let owner = format!("{root}/models/unbound-{resource:08x}");
            let scenes = scenes
                .into_iter()
                .enumerate()
                .map(|(layer, scene)| self.physical_scene(scene, &format!("{owner}/layer-{layer}")))
                .collect::<Result<_>>()?;
            field
                .unbound_geometry
                .push(field::UnboundGeometry { resource, scenes });
        }
        Ok(())
    }

    fn atlas(
        &mut self,
        atlases: &mut BTreeMap<String, String>,
        source: &str,
        name: &str,
    ) -> Result<String> {
        if let Some(path) = atlases.get(source) {
            return Ok(path.clone());
        }
        let path = self.image(source, name)?;
        atlases.insert(source.into(), path.clone());
        Ok(path)
    }
}
