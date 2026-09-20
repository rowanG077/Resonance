//! Resource bindings installed by the shared spell/skill loader.
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct NativeResources {
    pub source_size: usize,
    pub header_word: u32,
    pub effects: u32,
    pub textures: u32,
    pub models: [u32; 10],
    pub outlines: [u32; 10],
    pub motions: [[u32; 4]; 10],
    pub projectiles: u32,
    pub actions: u32,
    /// Bound for native callbacks; their individual layouts depend on the callback.
    pub callback_resources: [u32; 4],
}

impl NativeResources {
    pub const BYTES: usize = 276;

    pub fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= Self::BYTES,
            "truncated native resource header"
        );
        let words: [u32; 69] = std::array::from_fn(|index| {
            u32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let result = Self {
            source_size: bytes.len(),
            header_word: words[0],
            effects: words[1],
            textures: words[2],
            models: words[3..13].try_into()?,
            outlines: words[13..23].try_into()?,
            motions: std::array::from_fn(|row| {
                words[23 + row * 4..27 + row * 4].try_into().unwrap()
            }),
            projectiles: words[63],
            actions: words[64],
            callback_resources: words[65..69].try_into()?,
        };
        result.validate()?;
        Ok(result)
    }

    fn offsets(&self) -> impl Iterator<Item = u32> + '_ {
        [self.effects, self.textures]
            .into_iter()
            .chain(self.models)
            .chain(self.outlines)
            .chain(self.motions.into_iter().flatten())
            .chain([self.projectiles, self.actions])
            .chain(self.callback_resources)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_size >= Self::BYTES
                && self.offsets().all(|offset| {
                    offset == 0 || (Self::BYTES..self.source_size).contains(&(offset as usize))
                }),
            "native resource pointer exceeds package"
        );
        Ok(())
    }

    pub fn member(&self, field: usize) -> Result<Option<std::ops::Range<usize>>> {
        ensure!(
            (4..Self::BYTES).contains(&field) && field.is_multiple_of(4),
            "invalid native resource field"
        );
        let start = self.offsets().nth(field / 4 - 1).unwrap() as usize;
        Ok((start != 0).then(|| {
            start
                ..self
                    .offsets()
                    .map(|v| v as usize)
                    .filter(|&v| v > start)
                    .min()
                    .unwrap_or(self.source_size)
        }))
    }
}
