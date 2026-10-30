//! Shared triangle ribbons for weapon trails and kendama strings.
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};

#[derive(Default)]
pub(crate) struct Vertices {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
}
impl Vertices {
    pub(crate) fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
    pub(crate) fn quad(&mut self, positions: [Vec3; 4], uv: [[f32; 2]; 4], colors: [[f32; 4]; 4]) {
        // Two triangles form one ribbon segment.
        for i in [0, 1, 2, 2, 1, 3] {
            self.positions.push(positions[i].to_array());
            self.normals.push([0., 1., 0.]);
            self.uv.push(uv[i]);
            self.colors.push(colors[i]);
        }
    }
    pub(crate) fn line(&mut self, a: Vec3, b: Vec3, eye: Vec3, camera: Quat, width: f32, fov: f32) {
        let inverse = camera.inverse();
        let av = inverse * (a - eye);
        let bv = inverse * (b - eye);
        if av.z >= 0. || bv.z >= 0. {
            return;
        }
        let projected = Vec2::new(bv.x / -bv.z - av.x / -av.z, bv.y / -bv.z - av.y / -av.z);
        let normal = Vec2::new(-projected.y, projected.x).normalize_or_zero();
        let scale = width * (fov * 0.5).tan() / resonance_content::SCENE_HEIGHT as f32;
        let offset = |depth: f32| camera * Vec3::new(normal.x, normal.y, 0.) * (scale * -depth);
        let aa = offset(av.z);
        let bb = offset(bv.z);
        self.quad(
            [a - aa, a + aa, b - bb, b + bb],
            [[0.; 2]; 4],
            [[1., 1., 1., 128. / 255.]; 4],
        );
    }
    pub(crate) fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }
}

/// Native rope width changes at the same camera distances in both renderers.
pub(crate) fn string_width(distance: f32) -> f32 {
    if distance > 4000. {
        1.
    } else if distance > 2800. {
        2.
    } else if distance > 1600. {
        3.
    } else if distance > 400. {
        4.
    } else {
        5.
    }
}
