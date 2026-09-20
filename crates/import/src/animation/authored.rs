//! Authored curves retain channels and interpolation even before runtime playback supports them.
use crate::read::{f32 as float, u16 as half, u32 as word};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::ops::Range;

const HEADER: usize = 24;
const DESCRIPTOR: usize = 12;
const TRACK: usize = 16;
const KEY: usize = 12;

#[derive(Serialize, Deserialize)]
pub(crate) struct Animation {
    duration_frames: f32,
    flags: u16,
    names: Vec<String>,
    /// Authored byte counts can be stale; binding walks one C string per track.
    declared_name_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    stripped_name_table: Option<StrippedNameTable>,
    descriptors: Vec<Descriptor>,
    tracks: Vec<AuthoredTrack>,
}

#[derive(Serialize, Deserialize)]
struct StrippedNameTable {
    declared_bytes: usize,
    source_offset: usize,
}

#[derive(Serialize, Deserialize)]
struct Descriptor {
    name: Option<String>,
    first_track: usize,
    track_count: usize,
    force_index_binding: bool,
    flags: u16,
}

#[derive(Serialize, Deserialize)]
struct AuthoredTrack {
    node: u16,
    kind: u8,
    flags: u8,
    period_frames: f32,
    times: Vec<f32>,
    channels: Vec<Curve>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Component {
    Scale,
    Translation,
    EulerDegrees,
    Quaternion,
    Matrix3x4,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Interpolation {
    Step,
    Linear,
    ShortestAngleLinear,
    Bezier,
    Hermite,
    Squad,
    EasedSquad,
    ShortestSlerp,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scalar {
    U8,
    I8,
    U16,
    I16,
    F32,
}

impl Scalar {
    fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::F32 => 4,
        }
    }

    fn read(self, bytes: &[u8], at: usize, count: usize, scale: f32) -> Result<Vec<f32>> {
        (0..count)
            .map(|index| {
                let at = at + index * self.width();
                Ok(match self {
                    Self::U8 => f32::from(*bytes.get(at).context("missing u8 animation value")?),
                    Self::I8 => {
                        f32::from(*bytes.get(at).context("missing i8 animation value")? as i8)
                    }
                    Self::U16 => f32::from(half(bytes, at)?),
                    Self::I16 => f32::from(half(bytes, at)? as i16),
                    Self::F32 => float(bytes, at)?,
                } * scale)
            })
            .collect()
    }
}

#[derive(Serialize, Deserialize)]
struct Curve {
    component: Component,
    interpolation: Interpolation,
    scalar: Scalar,
    scale: f32,
    values: Vec<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    incoming: Vec<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    outgoing: Vec<Vec<f32>>,
    /// Per-key incoming/outgoing time easing, independent of value quantization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ease: Vec<[f32; 2]>,
}

impl Animation {
    /// Lower the supported authored channels to the same sparse curves used by
    /// native-source preparation; sampling and GLB baking remain shared.
    pub(crate) fn motion(&self, model: &super::ModelBindings) -> Result<super::Motion> {
        let [descriptor] = self.descriptors.as_slice() else {
            anyhow::bail!("unsupported animation descriptor layout");
        };
        ensure!(
            descriptor.first_track == 0
                && descriptor.track_count == self.tracks.len()
                && !self.tracks.is_empty()
                && self.tracks.len() <= usize::from(u16::MAX)
                && descriptor.force_index_binding == (descriptor.flags != 0),
            "invalid animation descriptor range"
        );
        for track in &self.tracks {
            ensure!(
                track.kind == 1
                    && track.flags & !11 == 0
                    && track.period_frames > 0.
                    && track.period_frames <= 18000.
                    && track.times.first() == Some(&0.)
                    && track.times.iter().all(|time| time.is_finite()
                        && *time >= 0.
                        && *time <= track.period_frames)
                    && track.times.windows(2).all(|times| times[0] <= times[1]),
                "unsupported animation encoding or timeline"
            );
        }
        ensure!(
            self.duration_frames
                == self
                    .tracks
                    .iter()
                    .map(|track| track.period_frames)
                    .fold(0., f32::max),
            "animation duration differs from its tracks"
        );
        let ids = self
            .tracks
            .iter()
            .map(|track| track.node)
            .collect::<Vec<_>>();
        let names = (!self.names.is_empty()).then_some(self.names.as_slice());
        Ok(super::Motion {
            duration_frames: self.duration_frames,
            tracks: model
                .bind(&ids, names, descriptor.force_index_binding)?
                .into_iter()
                .map(|(bone, track)| self.tracks[track].pose(bone))
                .collect::<Result<_>>()?,
        })
    }
}

impl AuthoredTrack {
    fn pose(&self, bone: u16) -> Result<super::PoseTrack> {
        use super::{QuaternionCurve, QuaternionInterpolation, VectorCurve, VectorInterpolation};
        let mut track = super::PoseTrack {
            bone,
            period_frames: self.period_frames,
            times: self.times.clone(),
            translation: None,
            scale: None,
            rotation: None,
        };
        let mut flags = 0;
        for curve in &self.channels {
            let controls = matches!(
                curve.interpolation,
                Interpolation::Bezier
                    | Interpolation::Hermite
                    | Interpolation::Squad
                    | Interpolation::EasedSquad
            );
            let eased = matches!(
                curve.interpolation,
                Interpolation::Hermite | Interpolation::EasedSquad
            );
            let count = self.times.len();
            ensure!(
                curve.values.len() == count
                    && curve.incoming.len() == if controls { count } else { 0 }
                    && curve.outgoing.len() == if controls { count } else { 0 }
                    && curve.ease.len() == if eased { count } else { 0 }
                    && curve.ease.iter().all(|ease| *ease == [0.; 2]),
                "invalid animation controls or unsupported time easing"
            );
            let flag = match curve.component {
                Component::Scale | Component::Translation => {
                    let interpolation = match curve.interpolation {
                        Interpolation::Step => VectorInterpolation::Step,
                        Interpolation::Linear => VectorInterpolation::Linear,
                        Interpolation::Bezier => VectorInterpolation::Bezier,
                        Interpolation::Hermite => VectorInterpolation::Hermite,
                        _ => anyhow::bail!("unsupported vector interpolation"),
                    };
                    let vector = VectorCurve {
                        interpolation,
                        values: vectors(&curve.values)?,
                        incoming: vectors(&curve.incoming)?,
                        outgoing: vectors(&curve.outgoing)?,
                    };
                    if curve.component == Component::Scale {
                        track.scale = Some(vector);
                        2
                    } else {
                        track.translation = Some(vector);
                        1
                    }
                }
                Component::Quaternion => {
                    let interpolation = match curve.interpolation {
                        Interpolation::Step => QuaternionInterpolation::Step,
                        Interpolation::ShortestSlerp => QuaternionInterpolation::ShortestSlerp,
                        Interpolation::Squad | Interpolation::EasedSquad => {
                            QuaternionInterpolation::Squad
                        }
                        _ => anyhow::bail!("unsupported quaternion interpolation"),
                    };
                    track.rotation = Some(QuaternionCurve {
                        interpolation,
                        values: vectors(&curve.values)?,
                        incoming: vectors(&curve.incoming)?,
                        outgoing: vectors(&curve.outgoing)?,
                    });
                    8
                }
                _ => anyhow::bail!("unsupported skeletal animation component"),
            };
            ensure!(flags & flag == 0, "duplicate animation component");
            flags |= flag;
        }
        ensure!(
            flags == self.flags,
            "animation channel flags differ from curves"
        );
        Ok(track)
    }
}

fn vectors<const N: usize>(values: &[Vec<f32>]) -> Result<Vec<[f32; N]>> {
    values
        .iter()
        .map(|value| {
            ensure!(
                value.iter().all(|v| v.is_finite()),
                "nonfinite animation value"
            );
            value
                .as_slice()
                .try_into()
                .context("invalid animation component width")
        })
        .collect()
}

struct Tables {
    descriptors: Range<usize>,
    tracks: Range<usize>,
    keys: Range<usize>,
}

fn tables(bytes: &[u8]) -> Result<Tables> {
    ensure!(word(bytes, 0)? == 0x007b_7960, "invalid animation magic");
    let descriptors = HEADER..HEADER + usize::from(half(bytes, 10)?) * DESCRIPTOR;
    let tracks = descriptors.end..descriptors.end + usize::from(half(bytes, 12)?) * TRACK;
    let keys = tracks.end..tracks.end + usize::from(half(bytes, 14)?) * KEY;
    ensure!(
        !descriptors.is_empty() && keys.end <= bytes.len(),
        "animation tables exceed resource"
    );
    ensure!(
        word(bytes, 4)? as usize == descriptors.start,
        "animation descriptor table pointer mismatch"
    );
    for at in descriptors.clone().step_by(DESCRIPTOR) {
        let start = word(bytes, at + 4)? as usize;
        let count = usize::from(half(bytes, at + 8)?);
        ensure!(
            start >= tracks.start
                && (start - tracks.start).is_multiple_of(TRACK)
                && start + count * TRACK <= tracks.end,
            "animation descriptor track range exceeds table"
        );
    }
    for at in tracks.clone().step_by(TRACK) {
        let start = word(bytes, at + 4)? as usize;
        let count = usize::from(half(bytes, at + 8)?);
        ensure!(
            count != 0
                && start >= keys.start
                && (start - keys.start).is_multiple_of(KEY)
                && start + count * KEY <= keys.end,
            "animation track key range exceeds table"
        );
    }
    Ok(Tables {
        descriptors,
        tracks,
        keys,
    })
}

/// Skeletons share the magic and can have 24 nodes; all three relocation tables
/// and their authored counts distinguish animation without trusting that coincidence.
pub(crate) fn is_animation(bytes: &[u8]) -> bool {
    tables(bytes).is_ok()
}

fn name(bytes: &[u8], at: usize) -> Result<String> {
    let tail = bytes.get(at..).context("animation name exceeds resource")?;
    let end = tail
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated animation name")?;
    Ok(tail[..end].escape_ascii().to_string())
}

pub(super) fn decode(bytes: &[u8]) -> Result<Animation> {
    decode_names(bytes, false)
}

pub(super) fn decode_indexed(bytes: &[u8]) -> Result<Animation> {
    decode_names(bytes, true)
}

fn decode_names(bytes: &[u8], package_omits_names: bool) -> Result<Animation> {
    let tables = tables(bytes)?;
    // Key rows have no encoding of their own: their owning track supplies it.
    // Unowned rows still identify an animation, but cannot be decoded by guessing
    // a neighboring track's scalar or channel layout.
    let mut covered = vec![false; tables.keys.len() / KEY];
    for at in tables.tracks.clone().step_by(TRACK) {
        let first = (word(bytes, at + 4)? as usize - tables.keys.start) / KEY;
        covered[first..first + usize::from(half(bytes, at + 8)?)].fill(true);
    }
    ensure!(
        covered.into_iter().all(|used| used),
        "animation contains unreferenced key records with no track encoding"
    );
    let name_size = word(bytes, 16)? as usize;
    let name_start = word(bytes, 20)? as usize;
    let mut names = Vec::new();
    let mut stripped_name_table = None;
    if name_size != 0 && name_start == 0 {
        stripped_name_table = Some(StrippedNameTable {
            declared_bytes: name_size,
            source_offset: 0,
        });
    } else if name_size != 0 {
        ensure!(
            name_start >= tables.keys.end,
            "animation names overlap tables"
        );
        if bytes.get(name_start..name_start + name_size).is_some() {
            let mut at = name_start;
            // Names are consumed by track count, not the stale byte-size field.
            // Read the terminating NUL from the resource even if it lies beyond
            // that declaration; alignment padding is never another bone name.
            for _ in 0..tables.tracks.len() / TRACK {
                let value = name(bytes, at)?;
                at += bytes[at..].iter().position(|&byte| byte == 0).unwrap() + 1;
                names.push(value);
            }
        } else {
            ensure!(
                package_omits_names && name_start <= bytes.len(),
                "animation name table exceeds resource"
            );
            // Package boundaries exclude the removed table. Some packages leave
            // a short footer here; it is not a truncated list of bone names.
            stripped_name_table = Some(StrippedNameTable {
                declared_bytes: name_size,
                source_offset: name_start,
            });
        }
    }
    let descriptors = tables
        .descriptors
        .clone()
        .step_by(DESCRIPTOR)
        .map(|at| {
            let name_at = word(bytes, at)? as usize;
            let flags = half(bytes, at + 10)?;
            Ok(Descriptor {
                name: (name_at != 0).then(|| name(bytes, name_at)).transpose()?,
                first_track: (word(bytes, at + 4)? as usize - tables.tracks.start) / TRACK,
                track_count: usize::from(half(bytes, at + 8)?),
                force_index_binding: flags != 0,
                flags,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let tracks = tables
        .tracks
        .step_by(TRACK)
        .map(|at| decode_track(bytes, at))
        .collect::<Result<Vec<_>>>()?;
    Ok(Animation {
        duration_frames: tracks
            .iter()
            .map(|track| track.period_frames)
            .fold(0., f32::max),
        flags: half(bytes, 8)?,
        names,
        declared_name_bytes: name_size,
        stripped_name_table,
        descriptors,
        tracks,
    })
}

fn decode_track(bytes: &[u8], at: usize) -> Result<AuthoredTrack> {
    let flags = bytes[at + 13];
    ensure!(
        flags & !0x1f == 0,
        "unknown animation channel flags {flags:#x}"
    );
    let format = bytes[at + 12];
    let scalar = match format >> 4 {
        0 => Scalar::U8,
        1 => Scalar::I8,
        2 => Scalar::U16,
        3 => Scalar::I16,
        4 => Scalar::F32,
        other => anyhow::bail!("unknown animation scalar {other}"),
    };
    let fixed_scale = 2_f32.powi(-i32::from(format & 15));
    let scale = if matches!(scalar, Scalar::F32) {
        1.
    } else {
        fixed_scale
    };
    let start = word(bytes, at + 4)? as usize;
    let keys = (start..start + usize::from(half(bytes, at + 8)?) * KEY)
        .step_by(KEY)
        .collect::<Vec<_>>();
    let times = keys
        .iter()
        .map(|&key| float(bytes, key))
        .collect::<Result<Vec<_>>>()?;
    let period_frames = float(bytes, at)?;
    ensure!(
        period_frames > 0.
            && times[0] >= 0.
            && times.last().copied().unwrap() <= period_frames
            && times.windows(2).all(|pair| pair[0] <= pair[1]),
        "invalid animation timeline"
    );
    let modes = bytes[at + 14];
    let mut value_offset = 0;
    let mut tangent_offset = 0;
    let mut channels = Vec::new();
    let mut channel = |component,
                       interpolation,
                       scalar: Scalar,
                       scale: f32,
                       width,
                       controls,
                       eased|
     -> Result<()> {
        let mut curve = Curve {
            component,
            interpolation,
            scalar,
            scale,
            values: Vec::new(),
            incoming: Vec::new(),
            outgoing: Vec::new(),
            ease: Vec::new(),
        };
        let stride = scalar.width() * width;
        for &key in &keys {
            let values = word(bytes, key + 4)? as usize;
            ensure!(values != 0, "missing animation values");
            curve
                .values
                .push(scalar.read(bytes, values + value_offset, width, scale)?);
            if controls {
                let tangents = word(bytes, key + 8)? as usize;
                ensure!(tangents != 0, "missing animation controls");
                let tangents = tangents + tangent_offset;
                curve
                    .incoming
                    .push(scalar.read(bytes, tangents, width, scale)?);
                curve
                    .outgoing
                    .push(scalar.read(bytes, tangents + stride, width, scale)?);
                if eased {
                    // Ease is signed 16-bit / 16384, even for byte or float values.
                    let at = tangents + stride * 2;
                    curve.ease.push([
                        f32::from(half(bytes, at)? as i16) / 16384.,
                        f32::from(half(bytes, at + 2)? as i16) / 16384.,
                    ]);
                }
            }
        }
        channels.push(curve);
        value_offset += stride;
        if controls {
            tangent_offset += stride * 2 + usize::from(eased) * 4;
        }
        Ok(())
    };
    if flags & 0x10 != 0 {
        // Matrix tracks are 12 float samples; the evaluator applies the fraction bits.
        channel(
            Component::Matrix3x4,
            Interpolation::Step,
            Scalar::F32,
            fixed_scale,
            12,
            false,
            false,
        )?;
    } else {
        let vector_mode = |mode| match mode {
            0 => Ok(Interpolation::Step),
            1 => Ok(Interpolation::Linear),
            2 => Ok(Interpolation::Bezier),
            3 => Ok(Interpolation::Hermite),
            _ => anyhow::bail!("invalid vector animation interpolation {mode}"),
        };
        if flags & 2 != 0 {
            let mode = (modes >> 2) & 3;
            channel(
                Component::Scale,
                vector_mode(mode)?,
                scalar,
                scale,
                3,
                mode >= 2,
                mode == 3,
            )?;
        }
        ensure!(
            flags & 12 != 12,
            "animation combines Euler and quaternion rotation"
        );
        let rotation_mode = (modes >> 4) & 7;
        if flags & 8 != 0 {
            let interpolation = match rotation_mode {
                0 => Interpolation::Step,
                4 => Interpolation::Squad,
                5 => Interpolation::EasedSquad,
                6 => Interpolation::ShortestSlerp,
                other => anyhow::bail!("invalid quaternion animation interpolation {other}"),
            };
            channel(
                Component::Quaternion,
                interpolation,
                Scalar::I16,
                1. / 16384.,
                4,
                matches!(rotation_mode, 4 | 5),
                rotation_mode == 5,
            )?;
        } else if flags & 4 != 0 {
            let interpolation = match rotation_mode {
                1 => Interpolation::ShortestAngleLinear,
                other => vector_mode(other)?,
            };
            channel(
                Component::EulerDegrees,
                interpolation,
                scalar,
                scale,
                3,
                rotation_mode >= 2,
                rotation_mode == 3,
            )?;
        }
        if flags & 1 != 0 {
            let mode = modes & 3;
            channel(
                Component::Translation,
                vector_mode(mode)?,
                scalar,
                scale,
                3,
                mode >= 2,
                mode == 3,
            )?;
        }
    }
    Ok(AuthoredTrack {
        node: half(bytes, at + 10)?,
        kind: bytes[at + 15],
        flags,
        period_frames,
        times,
        channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_half(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    fn put_word(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    fn fixture(format: u8, flags: u8, interpolation: u8) -> Vec<u8> {
        let mut bytes = vec![0; 192];
        for (at, value) in [
            (0, 0x007b_7960),
            (4, 24),
            (28, 36),
            (36, 10_f32.to_bits()),
            (40, 52),
            (56, 80),
            (60, 128),
        ] {
            put_word(&mut bytes, at, value);
        }
        for (at, value) in [(10, 1), (12, 1), (14, 1), (32, 1), (44, 1), (46, 9)] {
            put_half(&mut bytes, at, value);
        }
        bytes[48..52].copy_from_slice(&[format, flags, interpolation, 1]);
        bytes
    }

    #[test]
    fn cooked_curves_preserve_binding_after_json_roundtrip_and_reject_easing() -> Result<()> {
        let mut model = vec![0; 88];
        put_word(&mut model, 0, 0x007b7960);
        put_half(&mut model, 6, 2);
        put_word(&mut model, 12, 32);
        put_word(&mut model, 40, 60);
        for at in [54, 82] {
            put_half(&mut model, at, 9);
        }
        let bindings = super::super::ModelBindings::read(&model)?;
        for interpolation in [0x00, 0x01, 0x08, 0x0c, 0x40, 0x50, 0x60, 0x53] {
            let mut bytes = fixture(0x33, 11, interpolation);
            put_half(&mut bytes, 92, 16384);
            put_half(&mut bytes, 94, 123);
            let direct = super::super::motion(&bytes, &model)?;
            let authored: Animation =
                serde_json::from_slice(&serde_json::to_vec(&decode(&bytes)?)?)?;
            assert_eq!(
                serde_json::to_value(authored.motion(&bindings)?)?,
                serde_json::to_value(direct)?,
                "interpolation {interpolation:#x}"
            );
        }
        let mut bytes = fixture(0x30, 1, 3);
        put_half(&mut bytes, 140, 1);
        assert!(decode(&bytes)?.motion(&bindings).is_err());
        assert!(super::super::motion(&bytes, &model).is_err());
        Ok(())
    }

    #[test]
    fn authored_channels_keep_all_scalar_widths_and_fractional_scales() {
        let bindings = super::super::ModelBindings {
            node_ids: vec![9],
            names: None,
        };
        for (format, encoded, expected) in [
            (0x02, vec![12, 16, 20], vec![3., 4., 5.]),
            (0x11, vec![254, 4, 6], vec![-1., 2., 3.]),
            (0x23, vec![0, 8, 0, 16, 0, 24], vec![1., 2., 3.]),
            (0x31, vec![255, 254, 0, 4, 0, 6], vec![-1., 2., 3.]),
            (
                0x4f,
                [1_f32, 2., 3.]
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect(),
                vec![1., 2., 3.],
            ),
        ] {
            let mut bytes = fixture(format, 1, 1);
            bytes[80..80 + encoded.len()].copy_from_slice(&encoded);
            let animation = decode(&bytes).unwrap();
            assert_eq!(animation.tracks[0].channels[0].values[0], expected);
            assert_eq!(animation.tracks[0].node, 9);
            let motion = animation.motion(&bindings).unwrap();
            assert_eq!(
                motion.tracks[0]
                    .sample(0., super::super::Transform::default())
                    .unwrap()
                    .translation
                    .as_slice(),
                expected.as_slice()
            );
        }
    }

    #[test]
    fn float_vectors_and_quaternions_keep_independent_controls_and_ease() {
        let mut bytes = fixture(0x40, 9, 0x53);
        put_half(&mut bytes, 86, 16384);
        put_word(&mut bytes, 88, 7_f32.to_bits());
        put_half(&mut bytes, 144, 4096);
        put_half(&mut bytes, 146, 8192);
        put_word(&mut bytes, 148, 3_f32.to_bits());
        put_word(&mut bytes, 160, 5_f32.to_bits());
        put_half(&mut bytes, 172, 1024);
        put_half(&mut bytes, 174, 2048);
        let track = decode(&bytes).unwrap().tracks.remove(0);
        let rotation = &track.channels[0];
        let translation = &track.channels[1];
        assert_eq!(rotation.values, [vec![0., 0., 0., 1.]]);
        assert_eq!(rotation.ease, [[0.25, 0.5]]);
        assert_eq!(translation.values, [vec![7., 0., 0.]]);
        assert_eq!(translation.incoming, [vec![3., 0., 0.]]);
        assert_eq!(translation.outgoing, [vec![5., 0., 0.]]);
        assert_eq!(translation.ease, [[0.0625, 0.125]]);
        assert!(
            decode(&bytes)
                .unwrap()
                .motion(&super::super::ModelBindings {
                    node_ids: vec![9],
                    names: None,
                })
                .is_err(),
            "nonzero time easing still requires sampler support"
        );
    }

    #[test]
    fn matrices_euler_angles_names_and_exact_key_counts_survive_cooking() {
        let mut bytes = fixture(0x42, 0x10, 0);
        put_word(&mut bytes, 80, 8_f32.to_bits());
        let animation = decode(&bytes).unwrap();
        assert_eq!(animation.tracks[0].channels[0].values[0][0], 2.);
        bytes[49] = 4;
        bytes[50] = 0x10;
        let animation = decode(&bytes).unwrap();
        assert!(matches!(
            animation.tracks[0].channels[0].interpolation,
            Interpolation::ShortestAngleLinear
        ));
        put_word(&mut bytes, 16, 5);
        put_word(&mut bytes, 20, 184);
        bytes[184..189].copy_from_slice(b"root\0");
        assert_eq!(decode(&bytes).unwrap().names, ["root"]);
        put_word(&mut bytes, 16, 2);
        bytes[189..].fill(0xff);
        let named = decode(&bytes).unwrap();
        assert_eq!(named.names, ["root"]);
        assert_eq!(named.declared_name_bytes, 2);
        put_word(&mut bytes, 16, 5);
        put_word(&mut bytes, 20, 0);
        let unnamed = decode(&bytes).unwrap();
        assert!(unnamed.names.is_empty());
        assert_eq!(unnamed.stripped_name_table.unwrap().declared_bytes, 5);
        assert_eq!(unnamed.tracks.len(), 1);
        put_half(&mut bytes, 14, 2);
        assert!(
            is_animation(&bytes),
            "unowned keys do not identify a skeleton"
        );
        assert!(
            decode(&bytes)
                .err()
                .unwrap()
                .to_string()
                .contains("unreferenced key")
        );
        put_half(&mut bytes, 14, 1);
        put_half(&mut bytes, 44, 2);
        assert!(
            !is_animation(&bytes),
            "key counts cannot overrun the key table"
        );
        let mut skeleton = vec![0; 2048];
        put_word(&mut skeleton, 0, 0x007b_7960);
        put_half(&mut skeleton, 6, 24);
        put_word(&mut skeleton, 8, 1);
        put_word(&mut skeleton, 12, 32);
        assert!(!is_animation(&skeleton));
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_float_field_animation_cooks_all_72_tracks() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/FIELD/e01.d");
        let bytes = std::fs::read(source).unwrap();
        let range = crate::field::sections(&bytes).unwrap()[1].clone().unwrap();
        let nested = &bytes[range];
        let range = crate::field::sections(nested).unwrap()[2].clone().unwrap();
        let bytes = &nested[range];
        let animation = decode(bytes).unwrap();
        assert_eq!(animation.tracks.len(), 72);
        assert_eq!(animation.tracks[0].times.len(), 52);
        assert_eq!(animation.duration_frames, 150.);
        assert_eq!(animation.tracks[0].channels.len(), 2);
        assert!(matches!(
            animation.tracks[0].channels[1].scalar,
            Scalar::F32
        ));
        let bindings = super::super::ModelBindings {
            node_ids: animation.tracks.iter().map(|track| track.node).collect(),
            names: None,
        };
        assert_eq!(animation.motion(&bindings).unwrap().tracks.len(), 72);
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_enemy_animation_keeps_the_declared_name_table() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL");
        let common = std::fs::read(root.join("BTLusual.dat")).unwrap();
        let ranges = crate::field::sections(&common).unwrap();
        let directory = &common[ranges[10].clone().unwrap()];
        let compressed = std::fs::read(root.join("BTLenemy.dat")).unwrap();
        let at = word(directory, 86 * 4).unwrap() as usize;
        let bytes = crate::compression::decode(&compressed[at..]).unwrap();
        let start = word(&bytes, 0x98).unwrap() as usize;
        let end = (0x18..0x1e8)
            .step_by(4)
            .map(|field| word(&bytes, field).unwrap() as usize)
            .filter(|&offset| offset > start)
            .min()
            .unwrap_or(bytes.len());
        let animation = &bytes[start..end];
        assert!(
            decode(animation).is_err(),
            "standalone truncated name tables remain invalid"
        );
        let cooked = decode_indexed(animation).unwrap_or_else(|error| panic!(
            "{error:#}; source={start:#x}..{end:#x}, names_size={:#x}, names_at={:#x}, tracks={}",
            word(animation, 16).unwrap(), word(animation, 20).unwrap(), half(animation, 12).unwrap()
        ));
        assert!(!cooked.tracks.is_empty());
        assert_eq!(cooked.stripped_name_table.unwrap().declared_bytes, 664);
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_shared_motion_bank_retains_indexed_tracks_without_name_bytes() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/d.d");
        let bytes = std::fs::read(source).unwrap();
        for (index, clip) in [(2704, "hum27_11"), (2705, "hum27_12")] {
            let start = word(&bytes, 4 + index * 8).unwrap() as usize;
            let size = word(&bytes, 8 + index * 8).unwrap() as usize;
            let source = &bytes[start..start + size];
            assert!(decode(source).is_err());
            let animation = decode_indexed(source).unwrap();
            assert_eq!(animation.tracks.len(), 42);
            assert_eq!(animation.descriptors[0].name.as_deref(), Some(clip));
            let omitted = animation.stripped_name_table.unwrap();
            assert_eq!(omitted.source_offset, size);
            assert_eq!(omitted.declared_bytes, 602);
        }
    }
}
