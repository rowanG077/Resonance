//! Steam's model Z axis is reflected relative to the shared scene convention.
//! Apply the same basis change to geometry, rig matrices and motion channels.
pub(super) fn vector([x, y, z]: [f32; 3]) -> [f32; 3] {
    [x, y, -z]
}

pub(super) fn rotation([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    [-x, -y, z, w]
}

pub(super) fn transform(
    mut value: resonance_content::animation::Transform,
) -> resonance_content::animation::Transform {
    value.translation = vector(value.translation);
    value.rotation = rotation(value.rotation);
    value
}

pub(super) fn matrix(mut values: [f32; 16]) -> [f32; 16] {
    for (i, value) in values.iter_mut().enumerate() {
        if (i / 4 == 2) != (i % 4 == 2) {
            *value = -*value;
        }
    }
    values
}

pub(super) fn triangles(indices: &[u32]) -> Vec<u32> {
    indices
        .chunks_exact(3)
        .flat_map(|t| [t[0], t[2], t[1]])
        .collect()
}
