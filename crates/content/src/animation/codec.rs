//! Versioned clip metadata followed by lossless little-endian float buffers.
use super::*;

const MAGIC: &[u8; 4] = b"RMOT";
const VERSION: u32 = 2;
const HEADER: usize = 16;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    duration_bits: u32,
    tracks: Vec<TrackData>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackData {
    bone: u16,
    bind_channels: TransformChannels,
    period_bits: u32,
    times: Buffer,
    translation: Option<Curve<VectorInterpolation>>,
    scale: Option<Curve<VectorInterpolation>>,
    rotation: Option<Curve<QuaternionInterpolation>>,
    euler_degrees: Option<Curve<VectorInterpolation>>,
    matrices: Option<Buffer>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Curve<I> {
    interpolation: I,
    values: Buffer,
    incoming: Buffer,
    outgoing: Buffer,
    ease: Buffer,
}

/// Byte offset and scalar count. Component widths come from the typed channel.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Buffer {
    offset: u32,
    count: u32,
}

#[derive(Default)]
struct Writer(Vec<u8>);
impl Writer {
    fn put(&mut self, values: impl IntoIterator<Item = f32>) -> Result<Buffer> {
        let offset = u32::try_from(self.0.len())?;
        for value in values {
            self.0.extend_from_slice(&value.to_le_bytes());
        }
        Ok(Buffer {
            offset,
            count: u32::try_from((self.0.len() - offset as usize) / 4)?,
        })
    }

    fn curve<I, const N: usize>(
        &mut self,
        interpolation: I,
        values: &[[f32; N]],
        incoming: &[[f32; N]],
        outgoing: &[[f32; N]],
        ease: &[[f32; 2]],
    ) -> Result<Curve<I>> {
        Ok(Curve {
            interpolation,
            values: self.put(values.iter().flatten().copied())?,
            incoming: self.put(incoming.iter().flatten().copied())?,
            outgoing: self.put(outgoing.iter().flatten().copied())?,
            ease: self.put(ease.iter().flatten().copied())?,
        })
    }

    fn vector(&mut self, curve: &VectorCurve) -> Result<Curve<VectorInterpolation>> {
        self.curve(
            curve.interpolation,
            &curve.values,
            &curve.incoming,
            &curve.outgoing,
            &curve.ease,
        )
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, buffer: Buffer, width: usize) -> Result<&'a [u8]> {
        let count = usize::try_from(buffer.count)?;
        ensure!(
            usize::try_from(buffer.offset)? == self.cursor && count % width == 0,
            "invalid motion buffer offset or component count"
        );
        let size = count
            .checked_mul(4)
            .context("motion buffer size overflow")?;
        let end = self
            .cursor
            .checked_add(size)
            .context("motion buffer offset overflow")?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .context("truncated motion buffer")?;
        self.cursor = end;
        Ok(bytes)
    }

    fn arrays<const N: usize>(&mut self, buffer: Buffer) -> Result<Vec<[f32; N]>> {
        Ok(self
            .take(buffer, N)?
            .chunks_exact(N * 4)
            .map(|row| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap())
                })
            })
            .collect())
    }

    fn vector(&mut self, curve: Curve<VectorInterpolation>) -> Result<VectorCurve> {
        Ok(VectorCurve {
            interpolation: curve.interpolation,
            values: self.arrays(curve.values)?,
            incoming: self.arrays(curve.incoming)?,
            outgoing: self.arrays(curve.outgoing)?,
            ease: self.arrays(curve.ease)?,
        })
    }
}

impl Motion {
    /// Encode original keys and controls without normalization or resampling.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate_bones(usize::from(u16::MAX))?;
        let mut writer = Writer::default();
        let tracks = self
            .tracks
            .iter()
            .map(|track| {
                Ok(TrackData {
                    bone: track.bone,
                    bind_channels: track.bind_channels,
                    period_bits: track.period_frames.to_bits(),
                    times: writer.put(track.times.iter().copied())?,
                    translation: track
                        .translation
                        .as_ref()
                        .map(|v| writer.vector(v))
                        .transpose()?,
                    scale: track.scale.as_ref().map(|v| writer.vector(v)).transpose()?,
                    rotation: track
                        .rotation
                        .as_ref()
                        .map(|v| {
                            writer.curve(
                                v.interpolation,
                                &v.values,
                                &v.incoming,
                                &v.outgoing,
                                &v.ease,
                            )
                        })
                        .transpose()?,
                    euler_degrees: track
                        .euler_degrees
                        .as_ref()
                        .map(|v| writer.vector(v))
                        .transpose()?,
                    matrices: track
                        .matrices
                        .as_ref()
                        .map(|v| writer.put(v.iter().flatten().copied()))
                        .transpose()?,
                })
            })
            .collect::<Result<_>>()?;
        let metadata = serde_json::to_vec(&Clip {
            duration_bits: self.duration_frames.to_bits(),
            tracks,
        })?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(metadata.len())?.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(writer.0.len())?.to_le_bytes());
        bytes.extend_from_slice(&metadata);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        bytes.extend_from_slice(&writer.0);
        Ok(bytes)
    }

    /// Accept only this version's canonical, contiguous buffer layout.
    /// Counts never allocate until checked against the actual file length.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() >= HEADER && &bytes[..4] == MAGIC,
            "invalid motion header"
        );
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        ensure!(
            word(4) == VERSION,
            "unsupported motion version; recook assets"
        );
        let end = HEADER
            .checked_add(usize::try_from(word(8))?)
            .context("motion metadata size overflow")?;
        let start = end
            .checked_add(3)
            .context("motion metadata padding overflow")?
            & !3;
        ensure!(
            start.checked_add(usize::try_from(word(12))?) == Some(bytes.len()),
            "invalid motion file size"
        );
        ensure!(
            bytes[end..start].iter().all(|&v| v == 0),
            "invalid motion padding"
        );
        let metadata: Clip = serde_json::from_slice(&bytes[HEADER..end])?;
        ensure!(
            metadata.tracks.len() <= usize::from(u16::MAX),
            "invalid motion track count"
        );
        let mut reader = Reader {
            bytes: &bytes[start..],
            cursor: 0,
        };
        let tracks = metadata
            .tracks
            .into_iter()
            .map(|track| {
                Ok(Track {
                    bone: track.bone,
                    bind_channels: track.bind_channels,
                    period_frames: f32::from_bits(track.period_bits),
                    times: reader
                        .take(track.times, 1)?
                        .chunks_exact(4)
                        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
                        .collect(),
                    translation: track.translation.map(|v| reader.vector(v)).transpose()?,
                    scale: track.scale.map(|v| reader.vector(v)).transpose()?,
                    rotation: track
                        .rotation
                        .map(|v| -> Result<_> {
                            Ok(QuaternionCurve {
                                interpolation: v.interpolation,
                                values: reader.arrays(v.values)?,
                                incoming: reader.arrays(v.incoming)?,
                                outgoing: reader.arrays(v.outgoing)?,
                                ease: reader.arrays(v.ease)?,
                            })
                        })
                        .transpose()?,
                    euler_degrees: track.euler_degrees.map(|v| reader.vector(v)).transpose()?,
                    matrices: track.matrices.map(|v| reader.arrays(v)).transpose()?,
                })
            })
            .collect::<Result<_>>()?;
        ensure!(
            reader.cursor == reader.bytes.len(),
            "unreferenced motion buffer bytes"
        );
        let motion = Self {
            duration_frames: f32::from_bits(metadata.duration_bits),
            tracks,
        };
        motion.validate_bones(usize::from(u16::MAX))?;
        Ok(motion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> Motion {
        let empty = |bone| Track {
            bone,
            bind_channels: TransformChannels(13),
            period_frames: 2.,
            times: vec![-0., 1., 1., 2.],
            translation: None,
            scale: None,
            rotation: None,
            euler_degrees: None,
            matrices: None,
        };
        let values = vec![[-0., f32::from_bits(1), -1.25]; 4];
        let mut tracks = Vec::new();
        for interpolation in [
            VectorInterpolation::Step,
            VectorInterpolation::Linear,
            VectorInterpolation::ShortestAngleLinear,
            VectorInterpolation::Bezier,
            VectorInterpolation::Hermite,
        ] {
            let mut track = empty(tracks.len() as u16);
            let controls = matches!(
                interpolation,
                VectorInterpolation::Bezier | VectorInterpolation::Hermite
            );
            let curve = VectorCurve {
                interpolation,
                values: values.clone(),
                incoming: if controls { values.clone() } else { Vec::new() },
                outgoing: if controls { values.clone() } else { Vec::new() },
                ease: if interpolation == VectorInterpolation::Hermite {
                    vec![[0.3, -0.]; 4]
                } else {
                    Vec::new()
                },
            };
            match interpolation {
                VectorInterpolation::Linear => track.scale = Some(curve),
                VectorInterpolation::ShortestAngleLinear => track.euler_degrees = Some(curve),
                _ => track.translation = Some(curve),
            }
            tracks.push(track);
        }
        for interpolation in [
            QuaternionInterpolation::Step,
            QuaternionInterpolation::ShortestSlerp,
            QuaternionInterpolation::Squad,
        ] {
            let mut track = empty(tracks.len() as u16);
            let values = vec![
                [0., -0., 0., -1.],
                [0., 0., 0., 1.],
                [0., 0., 0., -1.],
                [0., 0., 0., 1.],
            ];
            let controls = interpolation == QuaternionInterpolation::Squad;
            track.rotation = Some(QuaternionCurve {
                interpolation,
                values: values.clone(),
                incoming: if controls { values.clone() } else { Vec::new() },
                outgoing: if controls { values } else { Vec::new() },
                ease: if controls {
                    vec![[-0.1, 0.2]; 4]
                } else {
                    Vec::new()
                },
            });
            tracks.push(track);
        }
        let mut track = empty(tracks.len() as u16);
        track.matrices = Some(vec![[1., 0.25, 0., -0., 0., 1., 0., 2., 0., 0., 1., 3.]; 4]);
        tracks.push(track);
        Motion {
            duration_frames: f32::from_bits(0x40200001),
            tracks,
        }
    }

    #[test]
    fn lossless_modes_and_malformed_buffers() -> Result<()> {
        let motion = clip();
        let bytes = motion.encode()?;
        let decoded = Motion::decode(&bytes)?;
        assert_eq!(
            serde_json::to_value(&motion)?,
            serde_json::to_value(&decoded)?
        );
        assert_eq!(
            bytes,
            decoded.encode()?,
            "every authored float bit must survive"
        );
        assert!(motion.tracks[0].times[0].is_sign_negative());
        for size in [0, 4, HEADER - 1, HEADER, bytes.len() - 1] {
            assert!(Motion::decode(&bytes[..size]).is_err());
        }
        for (at, value) in [(0, 0), (4, 1), (8, u32::MAX), (12, u32::MAX)] {
            let mut corrupt = bytes.clone();
            corrupt[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Motion::decode(&corrupt).is_err());
        }
        let metadata_len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        let payload = (HEADER + metadata_len).next_multiple_of(4);
        for case in 0..11 {
            let mut metadata: Clip = serde_json::from_slice(&bytes[HEADER..HEADER + metadata_len])?;
            let mut data = bytes[payload..].to_vec();
            match case {
                0 => metadata.tracks[0].times.offset = 4,
                1 => metadata.tracks[0].times.count = u32::MAX,
                2 => metadata.tracks[1].times.offset = 0,
                3 => {
                    metadata.tracks[0]
                        .translation
                        .as_mut()
                        .unwrap()
                        .values
                        .count -= 1
                }
                4 => metadata.tracks[0].period_bits = f32::INFINITY.to_bits(),
                5 => metadata.duration_bits = f32::NAN.to_bits(),
                6 => metadata.tracks[1].bone = 0,
                7 => data[..4].copy_from_slice(&f32::NAN.to_le_bytes()),
                8 => data.extend_from_slice(&0_f32.to_le_bytes()),
                9 => {
                    metadata.tracks[3]
                        .translation
                        .as_mut()
                        .unwrap()
                        .interpolation = VectorInterpolation::Step
                }
                10 => metadata.tracks[0].bind_channels.0 = 32,
                _ => unreachable!(),
            }
            let metadata = serde_json::to_vec(&metadata)?;
            let mut corrupt = bytes[..HEADER].to_vec();
            corrupt[8..12].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
            corrupt[12..16].copy_from_slice(&(data.len() as u32).to_le_bytes());
            corrupt.extend(metadata);
            corrupt.resize(corrupt.len().next_multiple_of(4), 0);
            corrupt.extend(data);
            assert!(Motion::decode(&corrupt).is_err(), "malformed case {case}");
        }
        let mut invalid = motion;
        invalid.tracks[0].translation.as_mut().unwrap().values[0][0] = f32::INFINITY;
        assert!(invalid.encode().is_err());
        Ok(())
    }
}
