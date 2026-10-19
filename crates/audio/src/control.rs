//! Direct operations on fourteen-bit musical controls.

/// Combine independent unsigned volume and expression gains.
pub fn volume(volume: u16, expression: u16) -> u16 {
    ((u32::from(volume) * u32::from(expression)) >> 14).min(16383) as u16
}

/// Scale a signed selector by its 16.16 gain and center it in the control range.
pub fn signed(value: i16, scale: i32) -> u16 {
    (((i64::from(value) * i64::from(scale)) >> 16).clamp(-8192, 8191) + 8192) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_gains_are_bounded_and_monotonic() {
        assert_eq!(volume(0, 16383), 0);
        assert_eq!(volume(8192, 8192), 4096);
        let mut previous = 0;
        for input in 0..=16383 {
            let gain = volume(input, 12000);
            assert!((previous..=input).contains(&gain));
            previous = gain;
        }
        for scale in [0, 65536, i32::MAX] {
            let mut previous = 0;
            for input in i16::MIN..=i16::MAX {
                let gain = signed(input, scale);
                assert!((previous..=16383).contains(&gain));
                previous = gain;
            }
        }
        assert_eq!(signed(0, i32::MIN), 8192);
        assert_eq!(signed(i16::MIN, i32::MAX), 0);
        assert_eq!(signed(i16::MIN, i32::MIN), 16383);
    }
}
