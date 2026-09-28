use bevy::{
    camera::{CameraProjection, SubCameraView},
    math::Vec3A,
    prelude::*,
};

use resonance_content::{HEIGHT, SCENE_HEIGHT};

const RASTER_SUBDIVISIONS: f32 = 12.;

/// Use the full display aspect even though the scene viewport retains only
/// 448 of the 480 authored display rows.
#[derive(Debug, Clone)]
pub(super) struct TitleProjection(pub PerspectiveProjection);

impl CameraProjection for TitleProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        aligned_projection(self.0.get_clip_from_view(), self.0.aspect_ratio)
    }
    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        aligned_projection(self.0.get_clip_from_view_for_sub(view), self.0.aspect_ratio)
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.0.far
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
    }
}

/// Clip triangles at the authored far plane, including large world tiles whose
/// bounds intersect the frustum. Bevy's default perspective only culls bounds.
#[derive(Debug, Clone)]
pub(super) struct WorldProjection(pub TitleProjection);

impl WorldProjection {
    fn finite(&self, mut matrix: Mat4) -> Mat4 {
        let projection = &self.0.0;
        // Reverse depth: near maps to one and far maps to zero.
        let scale = projection.near / (projection.far - projection.near);
        matrix.z_axis.z = scale;
        matrix.w_axis.z = projection.far * scale;
        matrix
    }
}
impl CameraProjection for WorldProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.finite(self.0.get_clip_from_view())
    }
    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        self.finite(self.0.get_clip_from_view_for_sub(view))
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.0.far()
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
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
    fn world_projection_clips_distant_vertices_without_changing_screen_coordinates() {
        for aspect_ratio in [4. / 3., 16. / 9.] {
            let projection = WorldProjection(TitleProjection(PerspectiveProjection {
                near: 100.,
                far: 12800.,
                aspect_ratio,
                ..default()
            }));
            let matrix = projection.get_clip_from_view();
            let base = projection.0.get_clip_from_view();
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
