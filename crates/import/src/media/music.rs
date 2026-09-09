use anyhow::{Result, ensure};

/// Two reverb buses; the title selects preset 1. Parameter order is
/// coloration, mix, time, damping, and pre-delay.
pub(super) fn title_reverbs(executable: &[u8]) -> Result<[[f32; 5]; 2]> {
    ensure!(
        crate::dol::slice(executable, 0x8021_08b1, 1)? == [1],
        "title song has an unsupported auxiliary-effect setup"
    );
    let addresses = [
        [
            0x8035_c47c,
            0x8035_c480,
            0x8035_c470,
            0x8035_c478,
            0x8035_c474,
        ],
        [
            0x8035_c490,
            0x8035_c4ac,
            0x8035_c490,
            0x8035_c47c,
            0x8035_c4a8,
        ],
    ];
    let mut result = [[0.; 5]; 2];
    for (bus, addresses) in result.iter_mut().zip(addresses) {
        for (parameter, address) in bus.iter_mut().zip(addresses) {
            *parameter = f32::from_be_bytes(crate::dol::slice(executable, address, 4)?.try_into()?);
        }
        for (value, (min, max)) in
            bus.iter()
                .zip([(0., 1.), (0., 1.), (0.01, 10.), (0., 1.), (0., 0.1)])
        {
            ensure!(
                value.is_finite() && (min..=max).contains(value),
                "invalid original reverb parameter"
            );
        }
    }
    Ok(result)
}
