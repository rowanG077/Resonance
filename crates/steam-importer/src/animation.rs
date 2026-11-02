//! Steam TOANMB curves. Native targets remain source identities until a model
//! selects them; no channel is silently discarded while decoding a resource.
use crate::pe;
use anyhow::{Context, Result, ensure};
use resonance_content::animation::Interpolation;

mod bind;
mod dct;
mod stream;

const MAX_KEYS: usize = 65536;
const MAX_VALUES: usize = 4_000_000;

pub struct Animation {
    pub duration_frames: f32,
    pub targets: Vec<Target>,
    pub curves: Vec<Curve>,
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub kind: u32,
    pub binding: u32,
    pub curve: usize,
}

pub struct Curve {
    pub dimensions: usize,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    /// Unused components are zero. Quaternion sign and scalar state are retained.
    pub values: Vec<[f32; 4]>,
}

impl Animation {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut header = Cursor::new(bytes);
        ensure!(header.word()? == 1, "unsupported Steam animation version");
        let duration_frames = header.float()?;
        ensure!(duration_frames >= 0., "negative Steam animation duration");
        ensure!(
            header.float()? == 0.,
            "unsupported Steam animation time origin"
        );
        let targets = header.relative()?;
        let count = header.word()? as usize;
        let directory = header.relative()?;
        let streams = header.word()? as usize;
        ensure!(
            header.word()? == 0,
            "unsupported Steam animation header flags"
        );
        ensure!(
            count <= MAX_KEYS && count == streams,
            "invalid Steam animation channel count"
        );
        if count == 0 {
            ensure!(
                targets == 12 && directory == 20 && bytes.len() == 32,
                "invalid empty Steam animation"
            );
            return Ok(Self {
                duration_frames,
                targets: Vec::new(),
                curves: Vec::new(),
            });
        }
        ensure!(
            targets == 32 && directory == targets + count * 16,
            "unsupported Steam animation directory layout"
        );
        let stream_start = directory + streams * 8;
        ensure!(
            stream_start <= bytes.len(),
            "truncated Steam animation directories"
        );
        let mut source = Cursor::new(&bytes[targets..directory]);
        let mut targets = Vec::new();
        let mut unique = std::collections::BTreeSet::new();
        for _ in 0..count {
            let kind = source.word()?;
            let binding = source.word()?;
            let curve = source.word()? as usize;
            ensure!(
                source.word()? == 0 && curve < streams,
                "invalid Steam animation target"
            );
            ensure!(
                unique.insert((kind, binding)),
                "duplicate Steam animation target"
            );
            targets.push(Target {
                kind,
                binding,
                curve,
            });
        }
        let mut curves = Vec::new();
        let mut values = 0usize;
        for i in 0..streams {
            let at = directory + i * 8;
            let start = at
                .checked_add(pe::word(bytes, at)? as usize)
                .context("animation pointer overflow")?;
            let length = pe::word(bytes, at + 4)? as usize;
            ensure!(
                start >= stream_start && start % 4 == 0,
                "animation stream overlaps directory or is unaligned"
            );
            let data = bytes
                .get(start..)
                .and_then(|b| b.get(..length))
                .context("animation stream exceeds resource")?;
            let curve = stream::read(data).with_context(|| format!("Steam animation curve {i}"))?;
            values = values
                .checked_add(curve.values.len() * curve.dimensions)
                .context("animation size overflow")?;
            ensure!(
                values <= MAX_VALUES,
                "Steam animation exceeds decoded value budget"
            );
            ensure!(
                curve.times.last().is_some_and(|t| *t <= duration_frames),
                "animation key exceeds duration"
            );
            curves.push(curve);
        }
        for target in &targets {
            ensure!(
                target.kind & 15 == curves[target.curve].dimensions as u32,
                "animation target and vector width differ"
            );
        }
        Ok(Self {
            duration_frames,
            targets,
            curves,
        })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(length)
            .context("animation offset overflow")?;
        let data = self
            .bytes
            .get(self.at..end)
            .context("truncated Steam animation")?;
        self.at = end;
        Ok(data)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn half(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn float(&mut self) -> Result<f32> {
        let value = f32::from_bits(self.word()?);
        ensure!(value.is_finite(), "non-finite Steam animation value");
        Ok(value)
    }
    fn relative(&mut self) -> Result<usize> {
        let at = self.at;
        at.checked_add(self.word()? as usize)
            .context("animation pointer overflow")
    }
    fn align(&mut self) -> Result<()> {
        self.take((4 - self.at % 4) % 4)?;
        Ok(())
    }
    fn vector(&mut self, dimensions: usize, normalized: bool) -> Result<[f32; 4]> {
        let mut value = [0.; 4];
        for v in &mut value[..dimensions] {
            *v = if normalized {
                (self.half()? as i16) as f32 / 32767.
            } else {
                self.float()?
            };
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        coordinates,
        model::{Node, Rig},
    };
    use resonance_content::animation::{Motion, Skeleton, Transform};

    #[test]
    fn native_motion_uses_shared_clock_rest_pose_and_coordinates() -> Result<()> {
        // One native linear XYZ stream with byte-sized source frame times.
        let mut stream = [0x3300u32, 2, 1f32.to_bits()]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        stream.extend([0, 60, 0, 0]);
        stream.extend(
            [0f32, 0., 0., 12., 6., 18.]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        let mut bytes = [
            1u32,
            60f32.to_bits(),
            0,
            20,
            1,
            28,
            1,
            0,
            0x20003,
            7,
            0,
            0,
            8,
            stream.len() as u32,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
        bytes.extend(stream);
        let rest = Transform {
            translation: [2., 3., 4.],
            scale: [-2., 0., 3.],
            rotation: glam::Quat::from_rotation_y(0.7).to_array(),
        };
        let native = Rig {
            nodes: vec![Node {
                binding: 7,
                name: "root".into(),
                parent: None,
                flags: 0,
                rest,
                visible: true,
                global: glam::Mat4::IDENTITY.to_cols_array(),
                inverse_bind: glam::Mat4::IDENTITY.to_cols_array(),
            }],
            animation_bindings: vec![(3, 7), (0x10014, 7), (0x20003, 7)],
        };
        let mut skeleton = Skeleton::single("root");
        skeleton.bones[0].bind = coordinates::transform(rest);
        let motion = Animation::read(&bytes)?.bind(&native, &skeleton)?;
        let decoded = Motion::decode_for_skeleton(&motion.encode(&skeleton)?, &skeleton)?;
        assert_eq!(decoded.duration_frames, 30.);
        let pose = decoded.tracks[0].sample(15., skeleton.bones[0].bind)?;
        assert_eq!(pose.translation, [8., 6., -13.]);
        assert_eq!(pose.scale, rest.scale);
        for (a, b) in pose
            .rotation
            .into_iter()
            .zip(coordinates::rotation(rest.rotation))
        {
            assert!((a - b).abs() < 0.00001);
        }
        assert!(Animation::read(&bytes[..bytes.len() - 1]).is_err());
        Ok(())
    }
}
