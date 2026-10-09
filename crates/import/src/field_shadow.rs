//! Contact shadow artwork and presentation defaults.
use crate::field_effects::Atlas;
use resonance_content::field::ContactShadow;

pub(crate) fn read() -> ContactShadow<Atlas> {
    ContactShadow {
        texture: Atlas::Effect(2),
        uv_size: [0.25; 2],
        half_size: 42.,
        height_offset: 2.,
        alpha: 64,
        anchor_node: 1,
    }
}
