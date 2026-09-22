use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction,
        Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey},
};

/// Each geometry mesh has one authored draw recipe, bound per scene instance.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Eq)]
#[reflect(Component)]
pub(super) struct MaterialSlot(pub usize);

impl MaterialSlot {
    pub fn index(self, count: usize) -> anyhow::Result<usize> {
        let index = self.0;
        anyhow::ensure!(
            index < count,
            "undeclared material slot {index} (count {count})"
        );
        Ok(index)
    }
}

pub(super) fn install(app: &mut App) {
    use bevy::gltf::extensions::{
        ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers,
    };
    struct Slots;
    impl GltfExtensionHandler for Slots {
        fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
            Box::new(Self)
        }

        fn on_spawn_mesh_and_material(
            &mut self,
            _: &mut bevy::asset::LoadContext<'_>,
            _: &bevy::gltf::gltf::Primitive,
            mesh: &bevy::gltf::gltf::Mesh,
            _: &bevy::gltf::gltf::Material,
            entity: &mut EntityWorldMut,
            _: &str,
        ) {
            entity.insert(MaterialSlot(mesh.index()));
        }
    }
    app.register_type::<MaterialSlot>();
    app.world_mut()
        .resource_mut::<GltfExtensionHandlers>()
        .0
        .write_blocking()
        .push(Box::new(Slots));
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct TitleOutput {
    #[texture(0)]
    #[sampler(1)]
    pub source: Handle<Image>,
    #[uniform(2)]
    pub brightness: Vec4,
    #[uniform(3)]
    pub screen_offset: Vec2,
}
impl TitleOutput {
    pub fn position(
        outputs: &mut Assets<Self>,
        position: [i16; 2],
        stage: super::display::OutputStage,
    ) {
        let offset = match stage {
            super::display::OutputStage::Framebuffer => Vec2::ZERO,
            super::display::OutputStage::Scanout => Vec2::new(
                f32::from(position[0]) / resonance_content::WIDTH as f32,
                f32::from(position[1]) / resonance_content::HEIGHT as f32,
            ),
        };
        let changed: Vec<_> = outputs
            .iter()
            .filter_map(|(id, output)| (output.screen_offset != offset).then_some(id))
            .collect();
        for id in changed {
            outputs.get_mut(id).unwrap().screen_offset = offset;
        }
    }
    /// Assets::iter_mut marks every visited material as modified, even if its
    /// bytes are unchanged. Avoid rebuilding bindings for unchanged output.
    pub fn update(outputs: &mut Assets<Self>, edit: impl Fn(&mut Vec4)) {
        let changed: Vec<_> = outputs
            .iter()
            .filter_map(|(id, output)| {
                let mut brightness = output.brightness;
                edit(&mut brightness);
                (brightness != output.brightness).then_some((id, brightness))
            })
            .collect();
        for (id, brightness) in changed {
            outputs.get_mut(id).unwrap().brightness = brightness;
        }
    }
}

impl Material2d for TitleOutput {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/title_output.wgsl".into()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct TitleText {
    #[texture(0)]
    #[sampler(1)]
    pub source: Handle<Image>,
    #[uniform(2)]
    pub opacity_pulse: Vec4,
}

impl Material2d for TitleText {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/title_text.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
        Ok(())
    }
}

/// Unlit scene surface with an optional second, independently mapped texture.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(SurfaceKey)]
#[data(4, SurfaceUniform, binding_array(10))]
#[bindless]
pub(super) struct TitleSurface {
    #[texture(0)]
    pub color: Option<Handle<Image>>,
    /// Share texture pixels while selecting a separately prepared sampler.
    #[texture(5)]
    #[sampler(1)]
    pub sampling: Option<Handle<Image>>,
    #[texture(2)]
    #[sampler(3)]
    pub multiply: Option<Handle<Image>>,
    pub uv_offsets: Vec4,
    pub uv_scales: Vec4,
    pub tint: Vec4,
    #[texture(6)]
    #[sampler(7)]
    pub toon_ramp: Option<Handle<Image>>,
    /// World-space light position and channel strength (0..255).
    pub field_light: Vec4,
    pub shade_colors: [Vec4; 2],
    pub vertex_color: bool,
    pub constant_color: bool,
    pub blend: bool,
    pub additive: bool,
    pub depth_test: bool,
    pub depth_write: bool,
    pub cull: resonance_content::CullFace,
}

impl Default for TitleSurface {
    fn default() -> Self {
        Self {
            color: None,
            sampling: None,
            multiply: None,
            toon_ramp: None,
            uv_offsets: Vec4::ZERO,
            uv_scales: Vec4::ONE,
            tint: Vec4::ONE,
            field_light: Vec4::ZERO,
            shade_colors: [Vec4::ONE; 2],
            vertex_color: true,
            constant_color: false,
            blend: false,
            additive: false,
            depth_test: true,
            depth_write: true,
            cull: resonance_content::CullFace::Back,
        }
    }
}

impl TitleSurface {
    pub fn textured(color: Option<Handle<Image>>) -> Self {
        Self {
            sampling: color.clone(),
            color,
            ..default()
        }
    }
}

/// One packed record per material in Bevy's reusable bindless data buffer.
/// Ordinary bindings use the same layout in a single uniform buffer.
#[derive(Clone, ShaderType)]
pub(super) struct SurfaceUniform {
    uv_offsets: Vec4,
    uv_scales: Vec4,
    tint: Vec4,
    field_light: Vec4,
    shade_colors: [Vec4; 2],
}
impl From<&TitleSurface> for SurfaceUniform {
    fn from(value: &TitleSurface) -> Self {
        Self {
            uv_offsets: value.uv_offsets,
            uv_scales: value.uv_scales,
            tint: value.tint,
            field_light: value.field_light,
            shade_colors: value.shade_colors,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SurfaceKey {
    vertex_color: bool,
    field_lighting: bool,
    constant_color: bool,
    depth_test: bool,
    depth_write: bool,
    blend: bool,
    additive: bool,
    cull: resonance_content::CullFace,
}

impl From<&TitleSurface> for SurfaceKey {
    fn from(material: &TitleSurface) -> Self {
        Self {
            vertex_color: material.vertex_color,
            field_lighting: material.toon_ramp.is_some(),
            constant_color: material.constant_color,
            depth_test: material.depth_test,
            depth_write: material.depth_write,
            blend: material.blend,
            additive: material.additive,
            cull: material.cull,
        }
    }
}

impl Material for TitleSurface {
    fn vertex_shader() -> ShaderRef {
        "embedded://resonance_presentation/title_surface_vertex.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/title_surface.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        // All authored draws must share a sortable phase. Opaque recipes still
        // disable blending in their pipeline, but cannot move ahead of actors.
        AlphaMode::Blend
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.label = Some("resonance/surface".into());
        if !key.bind_group_data.vertex_color {
            // Keep shared vertex buffers intact; this recipe does not consume color.
            let color = bevy::shader::ShaderDefVal::from("VERTEX_COLORS");
            descriptor
                .vertex
                .shader_defs
                .retain(|definition| *definition != color);
            if let Some(fragment) = &mut descriptor.fragment {
                fragment
                    .shader_defs
                    .retain(|definition| *definition != color);
            }
        }
        // Imported triangles have counter-clockwise winding.
        descriptor.primitive.cull_mode = match key.bind_group_data.cull {
            resonance_content::CullFace::Back => Some(Face::Back),
            resonance_content::CullFace::Front => Some(Face::Front),
            resonance_content::CullFace::None => None,
        };
        if key.bind_group_data.field_lighting {
            descriptor.vertex.shader_defs.push("FIELD_LIGHTING".into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push("FIELD_LIGHTING".into());
            }
        }
        if key.bind_group_data.constant_color
            && let Some(fragment) = &mut descriptor.fragment
        {
            fragment.shader_defs.push("CONSTANT_COLOR".into());
        }
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                if key.bind_group_data.additive {
                    let component = BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    };
                    target.blend = Some(BlendState {
                        color: component,
                        alpha: component,
                    });
                } else if !key.bind_group_data.blend {
                    target.blend = None;
                }
            }
        }
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(key.bind_group_data.depth_write);
            // Convert strict/non-strict depth tests to Bevy’s reverse-Z convention.
            depth.depth_compare = Some(if !key.bind_group_data.depth_test {
                CompareFunction::Always
            } else if key.bind_group_data.depth_write {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Greater
            });
        }
        Ok(())
    }
}
