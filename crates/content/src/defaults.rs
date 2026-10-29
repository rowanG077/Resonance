//! Omit ordinary zero, false and empty settings from editable documents.
pub fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

pub fn enabled() -> bool {
    true
}
pub fn is_enabled(value: &bool) -> bool {
    *value
}
pub fn opaque() -> u8 {
    255
}
pub fn is_opaque(value: &u8) -> bool {
    *value == 255
}
pub fn unit() -> f32 {
    1.0
}
pub fn is_unit(value: &f32) -> bool {
    *value == 1.0
}
