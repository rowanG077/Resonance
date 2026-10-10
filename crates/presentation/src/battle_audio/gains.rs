//! Native voice level, panning, and send to the shared reverb bus.
use super::*;

const VOICE_REVERB_SEND: f32 = 0.08;

pub(super) fn for_voice(level: u8, pan: u8, channels: usize) -> [[f32; 2]; 3] {
    let level = f32::from(level.min(127)) / 127.;
    let pan = ((f32::from(pan) - 64.) / 63.).clamp(-1., 1.);
    let stereo = if channels == 1 {
        let angle = (pan + 1.) * std::f32::consts::FRAC_PI_4;
        [angle.cos(), angle.sin()]
    } else {
        [(1. - pan).min(1.), (1. + pan).min(1.)]
    };
    let direct = stereo.map(|gain| (level * gain).clamp(0., 1.));
    [direct, [0.; 2], direct.map(|gain| gain * VOICE_REVERB_SEND)]
}

pub(super) fn add(buses: &mut BusFrame, pcm: [f32; 2], gains: [[f32; 2]; 3]) -> [[i16; 2]; 3] {
    let products = gains.map(|gains| {
        std::array::from_fn(|channel| {
            (pcm[channel] * gains[channel] * 32768.)
                .round()
                .clamp(-32768., 32767.) as i16
        })
    });
    for (bus, products) in buses.iter_mut().zip(products) {
        for (out, product) in bus.iter_mut().zip(products) {
            *out += i32::from(product);
        }
    }
    products
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_pan_and_reverb_are_bounded_and_stereo_keeps_its_center_level() {
        assert_eq!(for_voice(0, 64, 1), [[0.; 2]; 3]);
        let center = for_voice(127, 64, 1);
        assert!((center[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((center[0][0] - center[0][1]).abs() < 1e-6);
        assert_eq!(for_voice(127, 64, 2)[0], [1.; 2]);
        for pan in [0, 64, 127] {
            let full = for_voice(127, pan, 1);
            let quiet = for_voice(32, pan, 1);
            assert!(
                full.into_iter()
                    .flatten()
                    .all(|gain| (0.0..=1.0).contains(&gain))
            );
            for channel in 0..2 {
                assert!((quiet[0][channel] - full[0][channel] * 32. / 127.).abs() < 1e-6);
                assert_eq!(full[2][channel], full[0][channel] * VOICE_REVERB_SEND);
            }
        }
        assert_eq!(for_voice(127, 0, 1)[0], [1., 0.]);
        assert!(for_voice(127, 127, 1)[0][0].abs() < 1e-6);
    }

    #[test]
    fn mixing_retains_wide_bus_sums_and_returns_the_exact_release_tail() {
        let mut buses = [[30000; 2], [0; 2], [0; 2]];
        let tail = add(&mut buses, [0.5, -0.5], for_voice(127, 64, 2));
        assert_eq!(tail[0], [16384, -16384]);
        assert_eq!(buses[0], [46384, 13616]);
        assert_eq!(buses[2], tail[2].map(i32::from));
    }
}
