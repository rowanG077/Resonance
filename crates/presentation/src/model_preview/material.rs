use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey},
};
pub(super) type Surface = ExtendedMaterial<crate::materials::TitleSurface, PreviewExtension>;

/// Distinct pipeline ownership keeps field warmup assertions scoped to field draws.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub(super) struct PreviewExtension {}
impl MaterialExtension for PreviewExtension {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.label = Some("resonance/model-preview".into());
        Ok(())
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct Composite {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
    #[uniform(2)]
    pub opacity: f32,
}
impl Material2d for Composite {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/model_preview.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
        Ok(())
    }
}
