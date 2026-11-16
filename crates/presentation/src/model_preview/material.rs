use bevy::prelude::*;
/// Preview models are ordinary title surfaces on their own render layer, so
/// they share the field's streamed records, pipelines and native draws. Field
/// warm-up only waits for the field's cameras, which do not see that layer.
pub(crate) type Surface = crate::materials::TitleSurface;

#[derive(Asset, TypePath, Debug, Clone)]
pub(crate) struct Composite {
    pub image: Handle<Image>,
    pub opacity: f32,
}
impl Composite {
    /// The preview image and opacity the native frame composites.
    pub(crate) fn native(&self) -> (AssetId<Image>, f32) {
        (self.image.id(), self.opacity)
    }
}
