//! Contact footprints from the animated body anchors, excluding carried artwork.
use super::*;

pub(super) struct Bounds {
    bones: Vec<usize>,
}

impl Bounds {
    pub fn prepare(skeleton: &Skeleton) -> Self {
        Self {
            bones: skeleton
                .bones
                .iter()
                .enumerate()
                .filter_map(|(index, bone)| bone.shadow_bounds.then_some(index))
                .collect(),
        }
    }

    pub fn sample(
        &self,
        world: Mat4,
        bones: &[Matrix],
        scale: f32,
        height: f32,
    ) -> Option<([f32; 3], f32)> {
        self.measure(world, bones).map(|(center, extent)| {
            let radius = ((extent.x + extent.z) / 1.5 * scale).max(10.);
            ([center.x, height, center.z], radius)
        })
    }

    pub fn center(&self, world: Mat4, bones: &[Matrix]) -> Option<[f32; 3]> {
        self.measure(world, bones)
            .map(|(center, _)| center.to_array())
    }

    fn measure(&self, world: Mat4, bones: &[Matrix]) -> Option<(Vec3, Vec3)> {
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        let mut found = false;
        for &bone in &self.bones {
            let point =
                (world * Mat4::from_cols_array_2d(&bones[bone])).transform_point3(Vec3::ZERO);
            if point.y < 0.1 {
                continue;
            }
            found = true;
            minimum = minimum.min(point);
            maximum = maximum.max(point);
        }
        found.then(|| {
            let extent = maximum - minimum;
            (minimum + extent * 0.5, extent)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(names: &[&str], excluded: &[usize]) -> Bounds {
        Bounds::prepare(&Skeleton {
            bones: names
                .iter()
                .enumerate()
                .map(|(index, name)| resonance_content::animation::Bone {
                    shadow_bounds: !excluded.contains(&index),
                    name: (*name).into(),
                    parent: None,
                    parent_transform: None,
                    bind: Default::default(),
                })
                .collect(),
        })
    }

    fn point(x: f32, y: f32, z: f32) -> Matrix {
        Mat4::from_translation(Vec3::new(x, y, z)).to_cols_array_2d()
    }

    #[test]
    fn spinning_no_shadow_chain_does_not_resize_or_move_body_footprint() {
        let bounds = bounds(&["hips", "head", "ns_kyu02", "at02_ns_kyu13"], &[2, 3]);
        let mut bones = [
            point(-20., 20., -10.),
            point(20., 100., 10.),
            point(-300., 200., 0.),
            point(-500., 200., 0.),
        ];
        let before = bounds.sample(Mat4::IDENTITY, &bones, 1., 1.1);
        assert_eq!(before, Some(([0., 1.1, 0.], 40.)));
        bones[2] = point(0., 200., 300.);
        bones[3] = point(0., 200., 500.);
        assert_eq!(bounds.sample(Mat4::IDENTITY, &bones, 1., 1.1), before);

        // The body still drives the footprint when its actual stance changes.
        bones[0] = point(-35., 20., -25.);
        assert_eq!(
            bounds.sample(Mat4::IDENTITY, &bones, 1., 1.1),
            Some(([-7.5, 1.1, -7.5], 60.))
        );
    }

    #[test]
    fn shadow_bounds_obey_explicit_flags_independently_of_joint_names() {
        let names = [
            "root",
            "mo05_kubi",
            "mo15-head",
            "DM3",
            "at02",
            "Kk01",
            "pa00",
            "AB00",
            "ef00",
            "ki00",
            "ns00",
            "Bone_ude01",
            "Bone_te01",
            "Bone_yu01",
            "Bone_ring01",
            "obj_nuno01",
            "manto",
            "bone_ude01",
            "mon_hair01",
        ];
        let excluded = [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18];
        assert_eq!(bounds(&names, &excluded).bones, [0, 1, 2, 17]);
        let renamed = vec!["custom-joint"; names.len()];
        assert_eq!(bounds(&renamed, &excluded).bones, [0, 1, 2, 17]);
    }

    #[test]
    fn footprint_uses_world_bounds_scale_and_ground_admission() {
        let bounds = bounds(&["left", "right", "under_floor"], &[]);
        let bones = [
            point(-5., 2., -3.),
            point(5., 4., 3.),
            point(100., -1., 100.),
        ];
        let world =
            Mat4::from_translation(Vec3::new(100., 0., -20.)) * Mat4::from_scale(Vec3::splat(2.));
        let (position, radius) = bounds.sample(world, &bones, 0.75, 1.1).unwrap();
        assert_eq!(position, [100., 1.1, -20.]);
        assert_eq!(bounds.center(world, &bones), Some([100., 6., -20.]));
        assert_eq!(radius, 16.);
        assert_eq!(bounds.sample(world, &bones, 0., 1.1).unwrap().1, 10.);
        assert_eq!(
            bounds.sample(Mat4::IDENTITY, &[point(0., 0., 0.); 3], 1., 1.1),
            None
        );
    }
}
