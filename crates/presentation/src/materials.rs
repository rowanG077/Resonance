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
use resonance_events::effect::Blend;

#[cfg(test)]
#[path = "surface_shader_tests.rs"]
mod shader_tests;

pub(super) fn embed_shaders(app: &mut App) {
    bevy::asset::embedded_asset!(app, "title_surface.wgsl");
    bevy::asset::embedded_asset!(app, "title_surface_vertex.wgsl");
    bevy::shader::load_shader_library!(app, "surface_bindings.wgsl");
    bevy::shader::load_shader_library!(app, "effect_color.wgsl");
}

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
pub struct TitleOutput {
    #[texture(0)]
    #[sampler(1)]
    pub source: Handle<Image>,
    #[uniform(2)]
    pub brightness: Vec4,
    #[uniform(3)]
    pub screen_offset: Vec2,
}
impl TitleOutput {
    pub(super) fn position(
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
pub struct TitleText {
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

/// Register the movie/UI image pass and the final output conversion.
pub fn install_output_materials(app: &mut App) {
    app.add_plugins((
        bevy::sprite_render::Material2dPlugin::<TitleOutput>::default(),
        bevy::sprite_render::Material2dPlugin::<TitleText>::default(),
    ));
    bevy::asset::embedded_asset!(app, "title_output.wgsl");
    bevy::asset::embedded_asset!(app, "title_text.wgsl");
}

/// Register the scene material and its embedded production shaders.
pub fn install_surface_material(app: &mut App) {
    app.add_plugins(MaterialPlugin::<TitleSurface>::default());
    embed_shaders(app);
}

/// Unlit scene surface with an optional second, independently mapped texture.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(SurfaceKey)]
#[data(4, SurfaceUniform, binding_array(10))]
#[bindless]
pub struct TitleSurface {
    #[texture(0)]
    pub color: Option<Handle<Image>>,
    /// Share texture pixels while selecting a separately prepared sampler.
    #[texture(5)]
    #[sampler(1)]
    pub sampling: Option<Handle<Image>>,
    #[texture(2)]
    #[sampler(3)]
    pub multiply: Option<Handle<Image>>,
    /// Battle dual-palette effects take RGB from `color` and alpha from `multiply`.
    pub multiply_alpha_only: bool,
    pub uv_offsets: Vec4,
    pub uv_scales: Vec4,
    pub tint: Vec4,
    /// Native RGB ambient bytes (64 is neutral); W enables unlit actor modulation.
    pub ambient_color: Vec4,
    #[texture(6)]
    #[sampler(7)]
    pub toon_ramp: Option<Handle<Image>>,
    /// World-space light position and channel strength (0..255).
    pub field_light: Vec4,
    pub shade_colors: [Vec4; 2],
    /// Scene fog: RGB color, depth start/end, and nonlinear exponent (zero is linear).
    pub fog_color: Vec4,
    pub fog_range: Vec4,
    /// Use per-view field fog; menu previews and overworld materials keep their own fog.
    pub field_fog: bool,
    /// Native ambient channels are applied before byte-quantized toon shading.
    pub ambient_scale: Vec3,
    pub vertex_color: bool,
    pub constant_color: bool,
    /// Clamp the texture/color product before fog and framebuffer blending.
    pub clamp_color: bool,
    /// Apply particle opacity to byte-valued vertex alpha before interpolation.
    pub vertex_alpha: bool,
    /// Smallest covered alpha value, in byte units, after tinting.
    pub alpha_cutoff: u8,
    pub blend: Option<Blend>,
    pub red_channel: bool,
    pub depth_test: bool,
    /// Preserve native LEQUAL for translucent battle geometry without writes.
    pub depth_equal: bool,
    pub depth_write: bool,
    pub cull: resonance_content::CullFace,
}

impl Default for TitleSurface {
    fn default() -> Self {
        Self {
            color: None,
            sampling: None,
            multiply: None,
            multiply_alpha_only: false,
            toon_ramp: None,
            uv_offsets: Vec4::ZERO,
            uv_scales: Vec4::ONE,
            tint: Vec4::ONE,
            ambient_color: Vec4::new(64., 64., 64., 0.),
            field_light: Vec4::ZERO,
            shade_colors: [Vec4::ONE; 2],
            fog_color: Vec4::ZERO,
            fog_range: Vec4::ZERO,
            field_fog: false,
            ambient_scale: Vec3::ONE,
            vertex_color: true,
            constant_color: false,
            clamp_color: false,
            vertex_alpha: false,
            alpha_cutoff: 1,
            blend: None,
            red_channel: false,
            depth_test: true,
            depth_equal: false,
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
pub struct SurfaceUniform {
    uv_offsets: Vec4,
    uv_scales: Vec4,
    tint: Vec4,
    ambient_color: Vec4,
    field_light: Vec4,
    shade_colors: [Vec4; 2],
    fog_color: Vec4,
    fog_range: Vec4,
    alpha_cutoff: f32,
    ambient_scale: Vec4,
}
impl From<&TitleSurface> for SurfaceUniform {
    fn from(value: &TitleSurface) -> Self {
        Self {
            uv_offsets: value.uv_offsets,
            uv_scales: value.uv_scales,
            tint: value.tint,
            ambient_color: value.ambient_color,
            field_light: value.field_light,
            shade_colors: value.shade_colors,
            fog_color: value.fog_color,
            fog_range: value
                .fog_range
                .truncate()
                .extend(if value.field_fog { 1. } else { 0. }),
            alpha_cutoff: f32::from(value.alpha_cutoff) / 255.,
            ambient_scale: value.ambient_scale.extend(0.),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceKey {
    vertex_color: bool,
    field_lighting: bool,
    constant_color: bool,
    clamp_color: bool,
    vertex_alpha: bool,
    red_channel: bool,
    depth_test: bool,
    depth_equal: bool,
    depth_write: bool,
    blend: Option<Blend>,
    multiply_alpha_only: bool,
    cull: resonance_content::CullFace,
}

impl SurfaceKey {
    fn blend_state(self) -> Option<BlendState> {
        if self.blend == Some(Blend::Subtractive) {
            // GX_BM_SUBTRACT sets both factors to one, including stored alpha.
            let component = BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::ReverseSubtract,
            };
            return Some(BlendState {
                color: component,
                alpha: component,
            });
        }
        self.blend?;
        // GX uses the same source-alpha factors for color and stored alpha.
        // Modern OVER alpha would keep opaque destinations opaque, losing the
        // translucent scene alpha subsequently sampled by battle feedback.
        let component = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: if self.blend == Some(Blend::Additive) {
                BlendFactor::One
            } else {
                BlendFactor::OneMinusSrcAlpha
            },
            operation: BlendOperation::Add,
        };
        Some(BlendState {
            color: component,
            alpha: component,
        })
    }
}

impl From<&TitleSurface> for SurfaceKey {
    fn from(material: &TitleSurface) -> Self {
        Self {
            vertex_color: material.vertex_color,
            field_lighting: material.toon_ramp.is_some(),
            constant_color: material.constant_color,
            clamp_color: material.clamp_color,
            vertex_alpha: material.vertex_alpha,
            red_channel: material.red_channel,
            depth_test: material.depth_test,
            depth_equal: material.depth_equal,
            depth_write: material.depth_write,
            blend: material.blend,
            multiply_alpha_only: material.multiply_alpha_only,
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
        if key.bind_group_data.vertex_alpha {
            descriptor.vertex.shader_defs.push("VERTEX_ALPHA".into());
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader_defs.push("VERTEX_ALPHA".into());
            }
        }
        if key.bind_group_data.constant_color
            && let Some(fragment) = &mut descriptor.fragment
        {
            fragment.shader_defs.push("CONSTANT_COLOR".into());
        }
        if let Some(fragment) = &mut descriptor.fragment {
            if key.bind_group_data.clamp_color {
                fragment.shader_defs.push("CLAMP_COLOR".into());
            }
            if key.bind_group_data.red_channel {
                fragment.shader_defs.push("RED_CHANNEL".into());
            }
            if key.bind_group_data.multiply_alpha_only {
                fragment.shader_defs.push("MULTIPLY_ALPHA_ONLY".into());
            }
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = key.bind_group_data.blend_state();
            }
        }
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(key.bind_group_data.depth_write);
            // Convert strict/non-strict depth tests to Bevy’s reverse-Z convention.
            depth.depth_compare = Some(if !key.bind_group_data.depth_test {
                CompareFunction::Always
            } else if key.bind_group_data.depth_write || key.bind_group_data.depth_equal {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Greater
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_fog_uses_the_view_without_changing_overworld_fog() {
        let mut surface = TitleSurface {
            fog_color: Vec4::new(0.2, 0.3, 0.4, 1.),
            fog_range: Vec4::new(100., 1000., 1., 0.),
            ..default()
        };
        let overworld = SurfaceUniform::from(&surface);
        assert_eq!(overworld.fog_range, surface.fog_range);
        assert_eq!(overworld.fog_color, surface.fog_color);
        surface.field_fog = true;
        let field = SurfaceUniform::from(&surface);
        assert_eq!(field.fog_range.w, 1.);
        assert_eq!(field.fog_range.truncate(), overworld.fog_range.truncate());
        assert_eq!(
            SurfaceUniform::from(&TitleSurface::default()).fog_range,
            Vec4::ZERO
        );
    }
}

#[cfg(test)]
mod petrify_tests {
    use super::*;

    #[test]
    fn red_channel_specialization_preserves_existing_surface_recipes_and_uniforms() {
        // Enumerate independent recipes, including modes not used by stone actors.
        // Opting into a swap must change only its shader key, not a pass or sampler.
        for flags in 0u16..256 {
            let normal = TitleSurface {
                vertex_color: flags & 1 != 0,
                toon_ramp: (flags & 2 != 0).then(Handle::default),
                constant_color: flags & 4 != 0,
                depth_test: flags & 8 != 0,
                depth_equal: flags & 16 != 0,
                depth_write: flags & 32 != 0,
                blend: (flags & 64 != 0).then_some(Blend::Additive),
                multiply_alpha_only: flags & 128 != 0,
                tint: Vec4::new(0.25, 0.5, 0.75, 0.125),
                ambient_scale: Vec3::new(1., 2., 3.),
                ..Default::default()
            };
            let mut red = normal.clone();
            red.red_channel = true;
            let normal_key = SurfaceKey::from(&normal);
            let mut red_key = SurfaceKey::from(&red);
            assert!(normal_key != red_key);
            red_key.red_channel = false;
            assert!(normal_key == red_key);
            let a = SurfaceUniform::from(&normal);
            let b = SurfaceUniform::from(&red);
            assert_eq!(a.tint, b.tint);
            assert_eq!(a.ambient_scale, b.ambient_scale);
            assert_eq!(a.uv_offsets, b.uv_offsets);
            assert_eq!(a.uv_scales, b.uv_scales);
            assert_eq!(a.field_light, b.field_light);
            assert_eq!(a.shade_colors, b.shade_colors);
            assert_eq!(normal.color, red.color);
            assert_eq!(normal.sampling, red.sampling);
            assert_eq!(normal.multiply, red.multiply);
            assert_eq!(normal.toon_ramp, red.toon_ramp);
        }
    }
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    #[test]
    fn subtractive_surface_uses_destination_minus_source_rgba() {
        let surface = TitleSurface {
            blend: Some(Blend::Subtractive),
            ..Default::default()
        };
        let state = SurfaceKey::from(&surface).blend_state().unwrap();
        assert_eq!(state.color, state.alpha);
        assert_eq!(state.color.src_factor, BlendFactor::One);
        assert_eq!(state.color.dst_factor, BlendFactor::One);
        assert_eq!(state.color.operation, BlendOperation::ReverseSubtract);
        // Low alpha does not attenuate the RGB subtraction in GX mode3.
        let source = [0.5_f32, 0.25, 0.5, 0.125];
        let destination = [0.75_f32, 0.5, 0.25, 1.];
        let actual = std::array::from_fn::<_, 4, _>(|i| (destination[i] - source[i]).max(0.));
        assert_eq!(actual, [0.25, 0.25, 0., 0.875]);
        let ordinary = SurfaceKey::from(&TitleSurface {
            blend: Some(Blend::Alpha),
            ..surface
        });
        assert_ne!(ordinary.blend_state(), Some(state));
    }

    #[test]
    fn normal_surface_blending_retains_scene_alpha_for_feedback() {
        let key = |blend: bool, additive: bool| {
            SurfaceKey::from(&TitleSurface {
                blend: blend.then_some(if additive {
                    Blend::Additive
                } else {
                    Blend::Alpha
                }),
                ..Default::default()
            })
        };
        let normal = key(true, false).blend_state().unwrap();
        // Alpha blending changes output alpha without changing the RGB blend mode.
        assert_eq!(normal.color, BlendState::ALPHA_BLENDING.color);
        assert_eq!(normal.alpha, normal.color);
        assert_eq!(normal.alpha.operation, BlendOperation::Add);
        let factor = |factor, alpha: f32| match factor {
            BlendFactor::One => 1.,
            BlendFactor::SrcAlpha => alpha,
            BlendFactor::OneMinusSrcAlpha => 1. - alpha,
            other => panic!("unexpected surface blend factor {other:?}"),
        };
        let stored = |component: BlendComponent, source: f32, destination: f32| {
            source * factor(component.src_factor, source)
                + destination * factor(component.dst_factor, source)
        };
        // Transparent fragments leave destination alpha intact; partial opacity
        // must not force either an opaque or translucent destination opaque.
        for (source, destination, expected) in [
            (0., 0.25, 0.25),
            (0.5, 1., 0.75),
            (0.5, 0.25, 0.375),
            (1., 0.25, 1.),
        ] {
            assert_eq!(stored(normal.alpha, source, destination), expected);
        }
        assert_eq!(stored(BlendState::ALPHA_BLENDING.alpha, 0.5, 1.), 1.);
        // Existing additive precedence and opaque overwrite recipes are retained.
        assert_eq!(key(false, false).blend_state(), None);
        let additive = key(true, true).blend_state().unwrap();
        assert_eq!(additive.color, additive.alpha);
        assert_eq!(additive.alpha.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(additive.alpha.dst_factor, BlendFactor::One);
        assert_eq!(additive.alpha.operation, BlendOperation::Add);
    }
}
