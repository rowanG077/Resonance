use super::*;

/// The currently supported single-image TOTEXB_D profile. Image dimensions are
/// checked against DDS instead of trusting either resource independently.
pub struct Descriptor {
    pub width: u32,
    pub height: u32,
    pub payload: String,
}

impl Descriptor {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let dependencies = super::super::package::dependencies(bytes)?;
        ensure!(
            word(bytes, 12)? == 200 && matches!(super::super::pe::half(bytes, 16)?, 0x100 | 0x300),
            "unsupported Steam texture descriptor"
        );
        ensure!(
            super::super::pe::half(bytes, 22)? == 1
                && word(bytes, 24)? == 0
                && dependencies.len() == 1,
            "unsupported Steam texture image directory"
        );
        let width = u32::from(super::super::pe::half(bytes, 18)?);
        let height = u32::from(super::super::pe::half(bytes, 20)?);
        ensure!(
            (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
            "invalid Steam texture descriptor dimensions"
        );
        let payload = dependencies.into_iter().next().unwrap();
        // Field art also uses profile 0x100 with the same DDS dependency
        // layout. Keep the payload's authored levels, including a single
        // level, rather than assuming a full chain from the profile word.
        ensure!(
            payload.to_ascii_lowercase().ends_with(".totexp_p"),
            "texture needs DDS payload"
        );
        Ok(Self {
            width,
            height,
            payload,
        })
    }

    pub fn decode(&self, payload: &[u8]) -> Result<Decoded> {
        let decoded = decode(payload)?;
        ensure!(
            (decoded.levels[0].width, decoded.levels[0].height) == (self.width, self.height),
            "Steam texture descriptor and DDS dimensions differ"
        );
        Ok(decoded)
    }
}
