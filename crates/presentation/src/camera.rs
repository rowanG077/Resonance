use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};

use resonance_content::{HEIGHT, SCENE_HEIGHT};

pub(super) const RASTER_SUBDIVISIONS: f32 = 12.;
pub(super) const FIELD_NEAR: f32 = 100.;
pub(super) const FIELD_FAR: f32 = 40_000.;

/// Use the full display aspect even though the scene viewport retains only
/// 448 of the 480 authored display rows.
#[derive(Debug, Clone)]
pub(super) struct TitleProjection(pub PerspectiveProjection);

impl CameraProjection for TitleProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.finite(self.0.get_clip_from_view())
    }
    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        self.finite(self.0.get_clip_from_view_for_sub(view))
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.0.far
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
    }
}

impl TitleProjection {
    /// Clip geometry itself at the far plane, including batched particles and
    /// large tiles whose combined bounds still intersect the view.
    fn finite(&self, mut matrix: Mat4) -> Mat4 {
        let projection = &self.0;
        // Reverse depth: near maps to one and far maps to zero.
        let scale = projection.near / (projection.far - projection.near);
        matrix.z_axis.z = scale;
        matrix.w_axis.z = projection.far * scale;
        aligned_projection(matrix, projection.aspect_ratio)
    }
}

// The reference raster samples lie 1/12 pixel past a modern pixel center.
// Align the ordinary scene projection, retaining Bevy meshes and depth tests.
fn aligned_projection(projection: Mat4, aspect: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        -2. / (HEIGHT as f32 * aspect * RASTER_SUBDIVISIONS),
        2. / (SCENE_HEIGHT as f32 * RASTER_SUBDIVISIONS),
        0.,
    )) * projection
}

pub(super) fn overlay_alignment() -> Transform {
    Transform::from_xyz(1. / 12., -480. / (448. * 12.), 0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_clips_distant_vertices_without_changing_screen_coordinates() {
        for aspect_ratio in [4. / 3., 16. / 9.] {
            let projection = TitleProjection(PerspectiveProjection {
                near: 100.,
                far: 12800.,
                aspect_ratio,
                ..default()
            });
            let matrix = projection.get_clip_from_view();
            let base = aligned_projection(projection.0.get_clip_from_view(), aspect_ratio);
            for (distance, visible) in [(99., false), (101., true), (12799., true), (12801., false)]
            {
                let point = Vec4::new(25., -50., -distance, 1.);
                let clip = matrix * point;
                assert_eq!(clip.z >= 0. && clip.z <= clip.w, visible);
                assert_eq!(clip.xy(), (base * point).xy());
                assert_eq!(clip.w, (base * point).w);
            }
        }
    }
}
