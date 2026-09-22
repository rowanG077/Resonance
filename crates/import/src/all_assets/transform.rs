//! The authored transform is a 52-byte union, independent of runtime pose support.
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Transform {
    flags: u8,
    metadata: [u8; 3],
    #[serde(flatten)]
    layout: Layout,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Layout {
    Matrix {
        rows: [[f32; 4]; 3],
    },
    Components {
        scale: Option<[f32; 3]>,
        rotation: Option<Rotation>,
        translation: Option<[f32; 3]>,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Rotation {
    Quaternion { xyzw: [f32; 4] },
    EulerDegrees { xyz: [f32; 3] },
}

fn floats<const N: usize>(words: &[u32]) -> Result<[f32; N]> {
    let values: [u32; N] = words
        .try_into()
        .context("invalid transform channel width")?;
    let values = values.map(f32::from_bits);
    ensure!(
        values.iter().all(|value| value.is_finite()),
        "non-finite authored transform"
    );
    Ok(values)
}

impl Transform {
    pub(super) fn read(words: &[u32]) -> Result<Option<Self>> {
        if words.is_empty() {
            return Ok(None);
        }
        let words: &[u32; 13] = words.try_into().context("invalid transform block size")?;
        let [flags, a, b, c] = words[0].to_be_bytes();
        // Native matrix evaluation takes precedence over every component flag.
        let layout = if flags & 0x10 != 0 {
            Layout::Matrix {
                rows: [
                    floats(&words[1..5])?,
                    floats(&words[5..9])?,
                    floats(&words[9..13])?,
                ],
            }
        } else {
            let vector = |mask, at| {
                (flags & mask != 0)
                    .then(|| floats(&words[at..at + 3]))
                    .transpose()
            };
            Layout::Components {
                scale: vector(1, 1)?,
                rotation: if flags & 4 != 0 {
                    Some(Rotation::Quaternion {
                        xyzw: floats(&words[4..8])?,
                    })
                } else if flags & 2 != 0 {
                    Some(Rotation::EulerDegrees {
                        xyz: floats(&words[4..7])?,
                    })
                } else {
                    None
                },
                translation: vector(8, 8)?,
            }
        };
        Ok(Some(Self {
            flags,
            metadata: [a, b, c],
            layout,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_transform_reads_only_active_channels() -> Result<()> {
        let mut words: [u32; 13] = std::array::from_fn(|index| (index as f32).to_bits());
        for (flags, rotation) in [
            (0x0b_u8, "euler_degrees"),
            (0x0d, "quaternion"),
            (0x0f, "quaternion"),
        ] {
            words[0] = (u32::from(flags) << 24) | 0x012345;
            let value = serde_json::to_value(Transform::read(&words)?)?;
            assert_eq!(value["metadata"], serde_json::json!([1, 35, 69]));
            assert_eq!(value["rotation"]["kind"], rotation);
            assert_eq!(value["scale"], serde_json::json!([1., 2., 3.]));
            assert_eq!(value["translation"], serde_json::json!([8., 9., 10.]));
            if flags == 0x0b {
                assert_eq!(value["rotation"]["xyz"], serde_json::json!([4., 5., 6.]));
            } else {
                assert_eq!(
                    value["rotation"]["xyzw"],
                    serde_json::json!([4., 5., 6., 7.])
                );
            }
        }
        words[0] = 0x1f000000;
        let value = serde_json::to_value(Transform::read(&words)?)?;
        assert_eq!(value["kind"], "matrix");
        assert_eq!(
            value["rows"],
            serde_json::json!([[1., 2., 3., 4.], [5., 6., 7., 8.], [9., 10., 11., 12.]])
        );
        words[0] = 0;
        words[1] = 0x7fc00001;
        let value = serde_json::to_value(Transform::read(&words)?)?;
        assert!(value["scale"].is_null());
        words[0] = 0x01000000;
        assert!(Transform::read(&words).is_err());
        assert!(Transform::read(&[])?.is_none());
        Ok(())
    }
}
