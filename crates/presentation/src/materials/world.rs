//! Retained unlit world records, independent of field/battle material storage.
use super::*;

#[derive(Asset, TypePath, Debug, Clone)]
pub(crate) struct WorldSurface {
    pub color: Option<Handle<Image>>,
    pub multiply: Option<Handle<Image>>,
    pub(crate) row: Option<u32>,
    pub uv_offsets: Vec4,
    pub uv_motion: Vec4,
    pub uv_scales: Vec4,
    pub tint: Vec4,
    pub tint_motion: Vec4,
    pub ambient_color: Vec4,
    pub fog_color: Vec4,
    pub fog_range: Vec4,
    pub world_fog: bool,
    pub vertex_color: bool,
    pub constant_color: bool,
    pub clamp_color: bool,
    pub vertex_alpha: bool,
    pub alpha_cutoff: u8,
    pub blend: Option<Blend>,
    pub red_channel: bool,
    pub depth_test: bool,
    pub depth_equal: bool,
    pub depth_write: bool,
    pub multiply_alpha_only: bool,
    pub cull: resonance_content::CullFace,
}

impl Default for WorldSurface {
    fn default() -> Self {
        TitleSurface::default().into()
    }
}

impl From<TitleSurface> for WorldSurface {
    fn from(surface: TitleSurface) -> Self {
        debug_assert!(
            surface.toon_ramp.is_none()
                && surface.scenery_lighting == SceneryLighting::default()
                && surface.ambient_color.w == 0.
                && !surface.clip_below_floor
                && surface.fog_range.z == 0.,
            "world surface requires native unlit color and linear fog"
        );
        Self {
            // World recipes already prepare a shared-pixel image with their
            // native sampler. Bind that image once for both texture and sampler.
            color: surface.sampling.or(surface.color),
            multiply: surface.multiply,
            row: None,
            uv_offsets: surface.uv_offsets,
            uv_motion: Vec4::ZERO,
            uv_scales: surface.uv_scales,
            tint: surface.tint,
            tint_motion: Vec4::ZERO,
            ambient_color: surface.ambient_color,
            fog_color: surface.fog_color,
            fog_range: surface.fog_range,
            world_fog: false,
            vertex_color: surface.vertex_color,
            constant_color: surface.constant_color,
            clamp_color: surface.clamp_color,
            vertex_alpha: surface.vertex_alpha,
            alpha_cutoff: surface.alpha_cutoff,
            blend: surface.blend,
            red_channel: surface.red_channel,
            depth_test: surface.depth_test,
            depth_equal: surface.depth_equal,
            depth_write: surface.depth_write,
            multiply_alpha_only: surface.multiply_alpha_only,
            cull: surface.cull,
        }
    }
}

#[derive(Clone, Debug, PartialEq, ShaderType)]
pub(crate) struct WorldUniform {
    uv_offsets: Vec4,
    uv_motion: Vec4,
    uv_scales: Vec4,
    tint: Vec4,
    tint_motion: Vec4,
    ambient_color: Vec4,
    fog_color: Vec4,
    fog_range: Vec4,
    alpha_cutoff: f32,
    flags: u32,
}

impl From<&WorldSurface> for WorldUniform {
    fn from(surface: &WorldSurface) -> Self {
        Self {
            uv_offsets: surface.uv_offsets,
            uv_motion: surface.uv_motion,
            uv_scales: surface.uv_scales,
            tint: surface.tint,
            tint_motion: surface.tint_motion,
            ambient_color: surface.ambient_color,
            fog_color: surface.fog_color,
            fog_range: surface
                .fog_range
                .truncate()
                .extend(if surface.world_fog { 2. } else { 0. }),
            alpha_cutoff: f32::from(surface.alpha_cutoff) / 255.,
            flags: u32::from(surface.red_channel)
                | (u32::from(surface.vertex_color) << 2)
                | (u32::from(surface.uv_motion != Vec4::ZERO || surface.tint_motion != Vec4::ZERO)
                    << 4),
        }
    }
}

impl From<&WorldSurface> for SurfaceKey {
    fn from(surface: &WorldSurface) -> Self {
        Self {
            world_surface: true,
            constant_color: surface.constant_color,
            clamp_color: surface.clamp_color,
            vertex_alpha: surface.vertex_alpha,
            depth_test: surface.depth_test,
            depth_equal: surface.depth_equal,
            depth_write: surface.depth_write,
            blend: surface.blend,
            multiply_alpha_only: surface.multiply_alpha_only,
            cull: surface.cull,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_records_are_compact_and_preserve_the_field_battle_uniform_abi() {
        assert_eq!(SurfaceUniform::min_size().get(), 256);
        assert_eq!(WorldUniform::min_size().get(), 144);
        assert_eq!(UVec4::min_size().get(), 16);
    }
}
