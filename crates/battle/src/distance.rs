//! Vector operations shared by movement, contacts and targeting.
use glam::Vec3;

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    Vec3::from_array(a).dot(Vec3::from_array(b))
}

pub(crate) fn length(vector: [f32; 3]) -> f32 {
    Vec3::from_array(vector).length()
}

pub(crate) fn normalize(vector: [f32; 3]) -> [f32; 3] {
    Vec3::from_array(vector).normalize_or_zero().to_array()
}

/// Preserve the previous facing when two ground positions nearly coincide.
pub(crate) fn planar_direction(a: [f32; 3], b: [f32; 3], previous: [f32; 3]) -> [f32; 3] {
    let delta = Vec3::new(a[0] - b[0], 0., a[2] - b[2]);
    if delta.length() >= 0.5 {
        delta.normalize_or_zero().to_array()
    } else {
        previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_ignore_height_and_preserve_coincident_facing() {
        assert_eq!(
            planar_direction([0., 10., 0.], [0.; 3], [1., 0., 0.]),
            [1., 0., 0.]
        );
        assert_eq!(
            planar_direction([3., 100., 4.], [0.; 3], [0.; 3]),
            [0.6, 0., 0.8]
        );
        assert_eq!(normalize([0.; 3]), [0.; 3]);
        assert!((length([3., 0., 4.]) - 5.).abs() < 1e-6);
    }
}
