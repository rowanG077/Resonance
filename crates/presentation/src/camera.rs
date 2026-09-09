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
