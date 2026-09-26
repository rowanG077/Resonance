//! The SDK vector length used by fn_1_4DC18 (DOL fn_800FE6F8).
//! A correctly rounded host sqrt can change the strict clash comparison: the
//! original uses a hardware estimate followed by one single-precision refinement.

/// SDK fn_800FE73C: rounded Y/Z products, fused X + Y, then Z.
pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    let y = single(f64::from(a[1]) * f64::from(b[1]));
    let z = single(f64::from(a[2]) * f64::from(b[2]));
    let xy = single(f64::from(a[0]).mul_add(f64::from(b[0]), f64::from(y)));
    single(f64::from(xy) + f64::from(z))
}

fn squared_length([x, y, z]: [f32; 3]) -> f32 {
    let x_squared = single(f64::from(x) * f64::from(x));
    let y_squared = single(f64::from(y) * f64::from(y));
    let xz_squared = if f64::from(z).mul_add(f64::from(z), f64::from(x_squared))
        < f64::from(f32::MIN_POSITIVE)
    {
        0.
    } else {
        z.mul_add(z, x_squared)
    };
    single(f64::from(xz_squared) + f64::from(y_squared))
}

pub(crate) fn length(vector: [f32; 3]) -> f32 {
    let squared = squared_length(vector);
    if squared == 0. {
        return 0.;
    }
    if !squared.is_finite() {
        return f32::NAN;
    }
    single(f64::from(squared) * f64::from(inverse_length(squared)))
}

/// 4DA50 keeps the supplied direction below its 0.5 distance threshold. Contact
/// setup supplies zero; other callers can retain their previous direction.
pub(crate) fn planar_direction(a: [f32; 3], b: [f32; 3], previous: [f32; 3]) -> [f32; 3] {
    let difference = [a[0] - b[0], 0., a[2] - b[2]];
    if length(difference) >= 0.5 {
        normalize(difference)
    } else {
        previous
    }
}

pub(crate) fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let inverse = inverse_length(squared_length(vector));
    vector.map(|value| single(f64::from(value) * f64::from(inverse)))
}

// Shared single-refinement body of SDK length and normalization (800FE620).
fn inverse_length(squared: f32) -> f32 {
    // Every positive f32, including subnormals, is normal when widened to f64.
    let bits = f64::from(squared).to_bits();
    let exponent = (bits >> 52) & 0x7ff;
    let segment = (((exponent & 1) << 4) | ((bits >> 48) & 15)) as usize;
    let fraction = (bits >> 37) & 2047;
    let (base, slope) = ESTIMATE[segment];
    let mantissa = u64::from(base) - u64::from(slope) * fraction;
    let estimate_bits = ((3068 - exponent) / 2) << 52 | (mantissa << 26);
    let estimate = f64::from_bits(estimate_bits);
    // fmuls rounds its second multiplicand to 25 significant bits. Only the
    // estimate needs this treatment; the remaining operands are already f32.
    let rounded = f64::from_bits((estimate_bits + (1 << 27)) & !((1 << 28) - 1));
    let square = single(estimate * rounded);
    let half = single(estimate * 0.5);
    let correction = -square.mul_add(squared, -3.);
    single(f64::from(correction) * f64::from(half))
}

// The observed original FPSCR has NI set: a subnormal result is flushed before
// rounding, even if rounding alone would have promoted it to a normal f32.
fn single(value: f64) -> f32 {
    if value.abs() < f64::from(f32::MIN_POSITIVE) {
        0.
    } else {
        value as f32
    }
}

// Gekko frsqrte response points (base and decrement for each mantissa segment),
// documented by Dolphin's hardware tests, Common/FloatUtils.cpp:
// https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/Common/FloatUtils.cpp
const ESTIMATE: [(u32, u16); 32] = [
    (0x1a7e800, 0x568),
    (0x17cb800, 0x4f3),
    (0x1552800, 0x48d),
    (0x130c000, 0x435),
    (0x10f2000, 0x3e7),
    (0x0eff000, 0x3a2),
    (0x0d2e000, 0x365),
    (0x0b7c000, 0x32e),
    (0x09e5000, 0x2fc),
    (0x0867000, 0x2d0),
    (0x06ff000, 0x2a8),
    (0x05ab800, 0x283),
    (0x046a000, 0x261),
    (0x0339800, 0x243),
    (0x0218800, 0x226),
    (0x0105800, 0x20b),
    (0x3ffa000, 0x7a4),
    (0x3c29000, 0x700),
    (0x38aa000, 0x670),
    (0x3572000, 0x5f2),
    (0x3279000, 0x584),
    (0x2fb7000, 0x524),
    (0x2d26000, 0x4cc),
    (0x2ac0000, 0x47e),
    (0x2881000, 0x43a),
    (0x2665000, 0x3fa),
    (0x2468000, 0x3c2),
    (0x2287000, 0x38e),
    (0x20c1000, 0x35e),
    (0x1f12000, 0x332),
    (0x1d79000, 0x30a),
    (0x1bf4000, 0x2e6),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_length_matches_the_original_sdk_observed_in_dolphin() {
        let trace: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-distance.json")).unwrap();
        for row in trace["observations"].as_array().unwrap() {
            let vector = std::array::from_fn(|i| {
                f32::from_bits(row["input_bits"][i].as_u64().unwrap() as u32)
            });
            assert_eq!(
                length(vector).to_bits(),
                row["result_bits"].as_u64().unwrap() as u32,
                "call {} from {}",
                row["index"],
                row["lr"]
            );
        }
    }

    #[test]
    fn planar_direction_preserves_short_vectors_and_discards_vertical_distance() {
        let previous = [0., 0., -1.];
        for difference in [[0.; 3], [0.25, 1000., 0.], [0.5, -1000., 0.]] {
            // The SDK estimate puts exactly 0.5 just below the threshold.
            assert_eq!(planar_direction(difference, [0.; 3], previous), previous);
        }
        let above = f32::from_bits(0.5f32.to_bits() + 1);
        let direction = planar_direction([above, 1000., 0.], [0.; 3], previous);
        assert!((direction[0] - 1.).abs() < 0.0000002);
        assert_eq!(direction[1..], [0., 0.]);
        assert_eq!(planar_direction([0., 10., 0.], [0.; 3], [0.; 3]), [0.; 3]);
    }

    #[test]
    fn zero_underflow_and_overflow_follow_the_sdk_operations() {
        assert_eq!(length([0.; 3]), 0.);
        assert_eq!(length([1.0e-20, 0., 0.]), 0.);
        assert!(length([f32::MAX, 0., 0.]).is_nan());
    }
}
