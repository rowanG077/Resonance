//! The single editable motion format retains native curves and float values.
use super::*;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document<Tracks = Vec<Track>> {
    version: u32,
    duration_frames: f32,
    tracks: Tracks,
}

impl Motion {
    /// Preserve original keys and controls without normalization or resampling.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate_bones(usize::from(u16::MAX))?;
        let mut bytes = serde_json::to_vec_pretty(&Document {
            version: VERSION,
            duration_frames: self.duration_frames,
            tracks: &self.tracks,
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let document: Document = serde_json::from_slice(bytes).context("decode motion document")?;
        ensure!(
            document.version == VERSION,
            "unsupported motion version {}; expected {VERSION}",
            document.version
        );
        let motion = Self {
            duration_frames: document.duration_frames,
            tracks: document.tracks,
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
    fn editable_curves_preserve_every_float_bit_and_apply_edits() -> Result<()> {
        let original = clip();
        let bytes = original.encode()?;
        assert_eq!(float_bits(&Motion::decode(&bytes)?), float_bits(&original));
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        value["tracks"][0]["translation"]["values"][0][0] = 42.0.into();
        let edited = Motion::decode(&serde_json::to_vec(&value)?)?;
        assert_eq!(
            edited.tracks[0].translation.as_ref().unwrap().values[0][0],
            42.
        );
        value["tracks"][0]["translation"]["interpolation"] = "typo".into();
        assert!(Motion::decode(&serde_json::to_vec(&value)?).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        value["version"] = 999.into();
        assert!(Motion::decode(&serde_json::to_vec(&value)?).is_err());
        Ok(())
    }

    fn float_bits(motion: &Motion) -> Vec<u32> {
        let mut values = vec![motion.duration_frames];
        for track in &motion.tracks {
            values.push(track.period_frames);
            values.extend_from_slice(&track.times);
            for curve in [&track.translation, &track.scale, &track.euler_degrees]
                .into_iter()
                .flatten()
            {
                values.extend(curve.values.iter().flatten().copied());
                values.extend(curve.incoming.iter().flatten().copied());
                values.extend(curve.outgoing.iter().flatten().copied());
                values.extend(curve.ease.iter().flatten().copied());
            }
            if let Some(curve) = &track.rotation {
                values.extend(curve.values.iter().flatten().copied());
                values.extend(curve.incoming.iter().flatten().copied());
                values.extend(curve.outgoing.iter().flatten().copied());
                values.extend(curve.ease.iter().flatten().copied());
            }
            if let Some(matrices) = &track.matrices {
                values.extend(matrices.iter().flatten().copied());
            }
        }
        values.into_iter().map(f32::to_bits).collect()
    }

    #[test]
    fn lossless_modes_and_malformed_documents() -> Result<()> {
        let motion = clip();
        let bytes = motion.encode()?;
        let decoded = Motion::decode(&bytes)?;
        assert_eq!(
            serde_json::to_value(&motion)?,
            serde_json::to_value(&decoded)?
        );
        assert_eq!(float_bits(&motion), float_bits(&decoded));
        assert!(decoded.tracks[0].times[0].is_sign_negative());
        for size in [0, 1, bytes.len() / 2, bytes.len() - 2] {
            assert!(Motion::decode(&bytes[..size]).is_err());
        }
        // The removed cooked binary format has no admission or fallback path.
        assert!(Motion::decode(b"RMOT\x02\0\0\0\0\0\0\0\0\0\0\0").is_err());
        for case in 0..13 {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
            match case {
                0 => value["tracks"][0]["times"] = serde_json::json!([]),
                1 => value["tracks"][0]["period_frames"] = (-1).into(),
                2 => value["duration_frames"] = (-1).into(),
                3 => value["tracks"][1]["bone"] = 0.into(),
                4 => value["tracks"][0]["times"][0] = 2.into(),
                5 => value["tracks"][5]["rotation"]["values"][0] = serde_json::json!([0, 0, 1]),
                6 => value["tracks"][0]["bind_channels"] = 32.into(),
                7 => {
                    value["tracks"][0]["translation"]["values"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
                8 => value["unknown"] = true.into(),
                9 => value["tracks"][0]["unknown"] = true.into(),
                10 => value["tracks"][3]["translation"]["interpolation"] = "step".into(),
                11 => value["version"] = 0.into(),
                12 => value["tracks"][0]["bone"] = u16::MAX.into(),
                _ => unreachable!(),
            }
            assert!(
                Motion::decode(&serde_json::to_vec(&value)?).is_err(),
                "malformed case {case}"
            );
        }
        let mut invalid = motion;
        invalid.tracks[0].translation.as_mut().unwrap().values[0][0] = f32::INFINITY;
        assert!(invalid.encode().is_err());
        Ok(())
    }
}
