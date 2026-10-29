//! Native skeletal curves into editable glTF motion, shared by world and fields.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::animation::{
    Bone, Channel, FRAME_HZ, Interpolation, Motion, NodeTrack, Skeleton, Track, Transform,
};
use std::collections::{BTreeMap, BTreeSet};

fn float(bytes: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(dol::u32(bytes, at)?))
}

fn values<const N: usize>(bytes: &[u8], at: usize, width: usize, scale: f32) -> Result<[f32; N]> {
    let mut output = [0.; N];
    for (index, value) in output.iter_mut().enumerate() {
        *value = if width == 4 {
            float(bytes, at + index * width)?
        } else {
            f32::from(dol::u16(bytes, at + index * width)? as i16)
        } * scale;
        ensure!(value.is_finite(), "nonfinite portal motion value");
    }
    Ok(output)
}

#[derive(PartialEq)]
struct Curve<const N: usize> {
    value: Vec<[f32; N]>,
    incoming: Vec<[f32; N]>,
    outgoing: Vec<[f32; N]>,
    ease: Vec<[f32; 2]>,
    mode: u8,
}

fn curve<const N: usize>(
    bytes: &[u8],
    keys: &[usize],
    value_offset: usize,
    tangent_offset: usize,
    width: usize,
    scale: f32,
    mode: u8,
    controls: bool,
    eased: bool,
) -> Result<Curve<N>> {
    let mut result = Curve {
        value: Vec::new(),
        incoming: Vec::new(),
        outgoing: Vec::new(),
        ease: Vec::new(),
        mode,
    };
    for &key in keys {
        let source = dol::u32(bytes, key + 4)? as usize;
        let control_at = dol::u32(bytes, key + 8)? as usize;
        ensure!(source != 0, "missing native motion key");
        result
            .value
            .push(values(bytes, source + value_offset, width, scale)?);
        if controls {
            ensure!(control_at != 0, "missing native motion controls");
            result
                .incoming
                .push(values(bytes, control_at + tangent_offset, width, scale)?);
            result.outgoing.push(values(
                bytes,
                control_at + tangent_offset + N * width,
                width,
                scale,
            )?);
        }
        if eased {
            let at = control_at + tangent_offset + N * width * 2;
            result.ease.push([
                f32::from(dol::u16(bytes, at)? as i16) / 16384.,
                f32::from(dol::u16(bytes, at + 2)? as i16) / 16384.,
            ]);
        }
    }
    Ok(result)
}

#[derive(PartialEq)]
struct SourceTrack {
    node: u16,
    duration: f32,
    times: Vec<f32>,
    scale: Option<Curve<3>>,
    rotation: Option<Curve<4>>,
    translation: Option<Curve<3>>,
}

pub(crate) fn same_source(a: &[u8], b: &[u8]) -> Result<bool> {
    if dol::u16(a, 12)? != dol::u16(b, 12)? || dol::u32(a, 36)? != dol::u32(b, 36)? {
        return Ok(false);
    }
    let count = usize::from(dol::u16(a, 12)?);
    Ok(source(a)? == source(b)?
        && source_names(a, count).unwrap_or_default() == source_names(b, count).unwrap_or_default())
}

fn source(bytes: &[u8]) -> Result<(f32, Vec<SourceTrack>)> {
    let track_count = usize::from(dol::u16(bytes, 12)?);
    ensure!(
        dol::u32(bytes, 0)? == 0x007b_7960
            && dol::u32(bytes, 4)? == 24
            && dol::u16(bytes, 10)? == 1
            && (1..=512).contains(&track_count),
        "unsupported world actor animation header: magic={:#x}, header={}, kind={}, tracks={track_count}",
        dol::u32(bytes, 0)?,
        dol::u32(bytes, 4)?,
        dol::u16(bytes, 10)?
    );
    ensure!(
        dol::u32(bytes, 28)? == 36
            && usize::from(dol::u16(bytes, 32)?) == track_count
            && dol::u16(bytes, 34)? == 0,
        "unsupported world portal animation descriptor"
    );
    let mut tracks = Vec::new();
    let mut duration = 0_f32;
    for index in 0..track_count {
        let at = 36 + index * 16;
        let count = usize::from(dol::u16(bytes, at + 8)?);
        let table = dol::u32(bytes, at + 4)? as usize;
        let format = *bytes.get(at + 12).context("missing portal track format")?;
        let flags = bytes[at + 13];
        let period = float(bytes, at)?;
        duration = duration.max(period);
        let mode = bytes[at + 14];
        let has_rotation = flags & 8 != 0;
        let rotation_mode = mode >> 4;
        let scale_mode = (mode >> 2) & 3;
        let translation_mode = mode & 3;
        ensure!(
            period > 0.
                && count >= 1
                && (if has_rotation {
                    matches!(rotation_mode, 0 | 4 | 5 | 6)
                } else {
                    rotation_mode == 0
                })
                && bytes[at + 15] == 1
                && flags & !0x0b == 0
                && flags & 0x0b != 0
                && matches!(format >> 4, 3 | 4),
            "unsupported world actor animation track {index}"
        );
        let width = if format >> 4 == 4 { 4 } else { 2 };
        let scale = if width == 4 {
            1.
        } else {
            2_f32.powi(-i32::from(format & 15))
        };
        let keys = (0..count).map(|key| table + key * 12).collect::<Vec<_>>();
        let times = keys
            .iter()
            .map(|&key| float(bytes, key))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            times.first() == Some(&0.)
                && times.last().is_some_and(|&last| last <= period)
                && times.windows(2).all(|pair| pair[0] <= pair[1]),
            "invalid world portal animation key times"
        );
        let scale_curve = (flags & 2 != 0)
            .then(|| {
                curve(
                    bytes,
                    &keys,
                    0,
                    0,
                    width,
                    scale,
                    scale_mode,
                    scale_mode >= 2,
                    scale_mode == 3,
                )
            })
            .transpose()?;
        let vector_stride = if scale_curve.is_some() { 3 * width } else { 0 };
        let scale_controls = usize::from(scale_curve.is_some() && scale_mode >= 2);
        let scale_ease = usize::from(scale_curve.is_some() && scale_mode == 3) * 4;
        let rotation = has_rotation
            .then(|| {
                curve(
                    bytes,
                    &keys,
                    vector_stride,
                    scale_controls * 6 * width + scale_ease,
                    2,
                    1. / 16384.,
                    rotation_mode,
                    matches!(rotation_mode, 4 | 5),
                    rotation_mode == 5,
                )
            })
            .transpose()?;
        let translation = (flags & 1 != 0)
            .then(|| {
                curve(
                    bytes,
                    &keys,
                    vector_stride + usize::from(has_rotation) * 8,
                    scale_controls * 6 * width
                        + scale_ease
                        + usize::from(has_rotation && matches!(rotation_mode, 4 | 5)) * 16
                        + usize::from(has_rotation && rotation_mode == 5) * 4,
                    width,
                    scale,
                    translation_mode,
                    translation_mode >= 2,
                    translation_mode == 3,
                )
            })
            .transpose()?;
        tracks.push(SourceTrack {
            node: dol::u16(bytes, at + 10)?,
            duration: period,
            times,
            scale: scale_curve,
            rotation,
            translation,
        });
    }
    Ok((duration, tracks))
}

fn vector(curve: &Curve<3>, a: usize, b: usize, t: f32) -> [f32; 3] {
    let t = eased_time(curve, a, b, t);
    if curve.mode == 0 {
        return curve.value[a];
    }
    if curve.mode == 1 {
        return interpolate(curve.value[a], curve.value[b], t, false);
    }
    if curve.mode == 3 {
        let t2 = t * t;
        let t3 = t2 * t;
        return std::array::from_fn(|axis| {
            curve.value[a][axis] * (2. * t3 - 3. * t2 + 1.)
                + curve.value[b][axis] * (3. * t2 - 2. * t3)
                + curve.outgoing[a][axis] * (t3 - 2. * t2 + t)
                + curve.incoming[b][axis] * (t3 - t2)
        });
    }
    let u = 1. - t;
    std::array::from_fn(|axis| {
        let av = curve.value[a][axis];
        let bv = curve.value[b][axis];
        av * (u * u * u)
            + (av + curve.outgoing[a][axis]) * (3. * u * u * t)
            + (bv + curve.incoming[b][axis]) * (3. * u * t * t)
            + bv * (t * t * t)
    })
}

fn spherical(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot = a
        .iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        .clamp(-1., 1.);
    if 1. + dot <= 0.000001 {
        let other = [-b[1], b[0], -b[3], b[2]];
        let angle = std::f32::consts::FRAC_PI_2 * t;
        return std::array::from_fn(|i| a[i] * angle.cos() + other[i] * angle.sin());
    }
    if 1. - dot <= 0.000001 {
        return std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    }
    let angle = dot.acos();
    std::array::from_fn(|i| {
        (a[i] * ((1. - t) * angle).sin() + b[i] * (t * angle).sin()) / angle.sin()
    })
}

fn eased_time<const N: usize>(curve: &Curve<N>, a: usize, b: usize, t: f32) -> f32 {
    if curve.ease.is_empty() || t == 0. || t == 1. {
        return t;
    }
    let mut first = curve.ease[a][1];
    let mut second = curve.ease[b][0];
    let total = first + second;
    if total == 0. {
        return t;
    }
    let inverse = 1. / total;
    if total > 1. {
        first *= inverse;
        second *= inverse;
    }
    let shape = 0.5 - inverse;
    if t < first {
        t * (t * (shape / first))
    } else if t < 1. - second {
        shape * (2. * t - first)
    } else {
        let rest = 1. - t;
        1. - rest * (rest * (shape / second))
    }
}

fn squad(curve: &Curve<4>, a: usize, b: usize, t: f32) -> [f32; 4] {
    let t = eased_time(curve, a, b, t).clamp(0., 1.);
    if curve.mode == 0 {
        return curve.value[a];
    }
    if curve.mode == 6 {
        return glam::Quat::from_array(curve.value[a])
            .slerp(glam::Quat::from_array(curve.value[b]), t)
            .normalize()
            .to_array();
    }
    let q = spherical(
        spherical(curve.value[a], curve.value[b], t),
        spherical(curve.outgoing[a], curve.incoming[b], t),
        2. * t * (1. - t),
    );
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / length)
}

fn interpolate<const N: usize>(a: [f32; N], b: [f32; N], t: f32, rotation: bool) -> [f32; N] {
    if rotation {
        let a = glam::Quat::from_xyzw(a[0], a[1], a[2], a[3]);
        let b = glam::Quat::from_xyzw(b[0], b[1], b[2], b[3]);
        let q = a.slerp(b, t).normalize().to_array();
        std::array::from_fn(|i| q[i])
    } else {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    }
}

fn error<const N: usize>(a: [f32; N], mut b: [f32; N], rotation: bool, tolerance: f32) -> f32 {
    if rotation && a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>() < 0. {
        b = b.map(|value| -value);
    }
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).abs() / tolerance)
        .fold(0., f32::max)
}

fn subdivide<const N: usize>(
    a: f32,
    b: f32,
    av: [f32; N],
    bv: [f32; N],
    rotation: bool,
    tolerance: f32,
    sample: &impl Fn(f32) -> [f32; N],
    times: &mut Vec<f32>,
    values: &mut Vec<[f32; N]>,
    depth: u8,
) -> Result<()> {
    ensure!(times.len() < 100_000, "portal motion has too many keys");
    let mid = a + (b - a) * 0.5;
    // Bound pathological native discontinuities while resolving each frame
    // sixteen times more finely than ordinary playback.
    if mid == a || mid == b || b - a <= 1. / (FRAME_HZ * 16.) {
        times.push(b);
        values.push(bv);
        return Ok(());
    }
    let mut split = b - a > 1. / FRAME_HZ;
    for t in [0.25, 0.5, 0.75] {
        let time = a + (b - a) * t;
        split |= error(
            sample(time),
            interpolate(av, bv, (time - a) / (b - a), rotation),
            rotation,
            tolerance,
        ) > 0.5;
    }
    if split {
        ensure!(depth < 40, "portal curve exceeds float precision");
        let mv = sample(mid);
        subdivide(
            a,
            mid,
            av,
            mv,
            rotation,
            tolerance,
            sample,
            times,
            values,
            depth + 1,
        )?;
        subdivide(
            mid,
            b,
            mv,
            bv,
            rotation,
            tolerance,
            sample,
            times,
            values,
            depth + 1,
        )?;
    } else {
        times.push(b);
        values.push(bv);
    }
    Ok(())
}

fn reduce<const N: usize>(
    times: &[f32],
    values: &[[f32; N]],
    rotation: bool,
    tolerance: f32,
    a: usize,
    b: usize,
    keep: &mut [bool],
) {
    if b <= a + 1 {
        return;
    }
    let (worst, distance) = (a + 1..b)
        .map(|i| {
            let t = (times[i] - times[a]) / (times[b] - times[a]);
            (
                i,
                error(
                    values[i],
                    interpolate(values[a], values[b], t, rotation),
                    rotation,
                    tolerance,
                ),
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    if distance > 0.5 {
        keep[worst] = true;
        reduce(times, values, rotation, tolerance, a, worst, keep);
        reduce(times, values, rotation, tolerance, worst, b, keep);
    }
}

fn sample_source<const N: usize>(
    source: &SourceTrack,
    frame: f32,
    value: impl Fn(usize, usize, f32) -> [f32; N],
) -> [f32; N] {
    let frame = frame.min(source.duration);
    let last = *source.times.last().unwrap();
    if frame >= last && last < source.duration {
        let t = ((frame - last) / (source.duration - last)).clamp(0., 1.);
        return value(source.times.len() - 1, 0, t);
    }
    let a = source
        .times
        .partition_point(|&time| time <= frame)
        .saturating_sub(1);
    let b = (a + 1).min(source.times.len() - 1);
    if a == b {
        return value(a, a, 0.);
    }
    let t = ((frame - source.times[a]) / (source.times[b] - source.times[a])).clamp(0., 1.);
    value(a, b, t)
}

fn bake<const N: usize>(
    source: &SourceTrack,
    rotation: bool,
    tolerance: f32,
    value: impl Fn(usize, usize, f32) -> [f32; N],
) -> Result<Channel<N>> {
    let sample = |seconds: f32| sample_source(source, seconds * FRAME_HZ, &value);
    let mut times = vec![0.];
    let mut values = vec![sample(0.)];
    // Native tracks may repeat a timestamp to author a discontinuity. glTF
    // cannot store duplicate input times, so preserve both sides at adjacent
    // representable times instead of dropping either key.
    let end = source.duration / FRAME_HZ;
    let mut boundaries = source
        .times
        .iter()
        .map(|time| time / FRAME_HZ)
        .collect::<Vec<_>>();
    boundaries.push(end);
    for pair in source.times.windows(2).filter(|pair| pair[0] == pair[1]) {
        let at = pair[0] / FRAME_HZ;
        if at > 0. {
            boundaries.push(at.next_down());
        }
        if at < end {
            boundaries.push(at.next_up());
        }
    }
    boundaries.sort_by(f32::total_cmp);
    boundaries.dedup();
    for key in boundaries.windows(2) {
        let a = key[0];
        let b = key[1];
        let av = *values.last().unwrap();
        let bv = sample(b);
        subdivide(
            a,
            b,
            av,
            bv,
            rotation,
            tolerance,
            &sample,
            &mut times,
            &mut values,
            0,
        )?;
    }
    let mut keep = vec![false; times.len()];
    keep[0] = true;
    keep[times.len() - 1] = true;
    reduce(
        &times,
        &values,
        rotation,
        tolerance,
        0,
        times.len() - 1,
        &mut keep,
    );
    Ok(Channel {
        interpolation: Interpolation::Linear,
        times: times
            .iter()
            .enumerate()
            .filter_map(|(i, &t)| keep[i].then_some(t))
            .collect(),
        values: values
            .into_iter()
            .enumerate()
            .filter_map(|(i, value)| keep[i].then_some(value))
            .collect(),
    })
}

pub(crate) fn encode(bytes: &[u8], model_glb: &[u8]) -> Result<Vec<u8>> {
    encode_with_skeleton(bytes, Skeleton::from_glb(model_glb)?, None, None)
}

/// Bake native half-frame attachment positions for collision. Visual glTF
/// motion is approximated within a bounded tolerance, but collision samples
/// can use the same source curves directly while the native member is here.
pub(crate) fn sample_bound_points(
    bytes: &[u8],
    skeleton: &Skeleton,
    bindings: &[(u16, String)],
    bones: &[u16],
    lifetime: u32,
) -> Result<Vec<Vec<[f32; 3]>>> {
    let (duration, source) = source(bytes)?;
    ensure!(
        bindings.len() == skeleton.bones.len(),
        "native motion rig differs from model"
    );
    let names = source_names(bytes, source.len()).unwrap_or_default();
    let use_names = names
        .iter()
        .any(|name| bindings.iter().any(|(_, model)| model == name));
    let selected = source
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            let bone = if use_names {
                bindings.iter().position(|(_, name)| name == &names[index])
            } else {
                bindings.iter().position(|(id, _)| *id == track.node)
            }?;
            Some((bone, track))
        })
        .collect::<Vec<_>>();
    let final_age = lifetime.min((duration * 2.).ceil() as u32);
    let times = (0..=final_age)
        .map(|age| age as f32 * 0.5 / FRAME_HZ)
        .collect::<Vec<_>>();
    let tracks = selected
        .into_iter()
        .map(|(bone, track)| {
            let mut node = NodeTrack::new(skeleton.bones[bone].bind);
            if let Some(curve) = &track.scale {
                node.scale = Some(Channel {
                    interpolation: Interpolation::Linear,
                    times: times.clone(),
                    values: (0..=final_age)
                        .map(|age| {
                            sample_source(track, age as f32 * 0.5, |a, b, t| vector(curve, a, b, t))
                        })
                        .collect(),
                });
            }
            if let Some(curve) = &track.rotation {
                node.rotation = Some(Channel {
                    interpolation: Interpolation::Linear,
                    times: times.clone(),
                    values: (0..=final_age)
                        .map(|age| {
                            sample_source(track, age as f32 * 0.5, |a, b, t| squad(curve, a, b, t))
                        })
                        .collect(),
                });
            }
            if let Some(curve) = &track.translation {
                node.translation = Some(Channel {
                    interpolation: Interpolation::Linear,
                    times: times.clone(),
                    values: (0..=final_age)
                        .map(|age| {
                            sample_source(track, age as f32 * 0.5, |a, b, t| vector(curve, a, b, t))
                        })
                        .collect(),
                });
            }
            Track {
                bone: bone as u16,
                nodes: vec![node],
            }
        })
        .collect();
    let motion = Motion {
        duration_frames: duration,
        tracks,
    };
    (0..=final_age)
        .map(|age| {
            let frame = (age as f32 * 0.5).min(duration);
            bones
                .iter()
                .map(|&bone| skeleton.sample_point(&motion, frame, bone, [0.; 3]))
                .collect()
        })
        .collect()
}

/// Field-local animation banks have no single owner rig. Keep native indices as
/// editable glTF node names; actors remap these nodes in their field document.
pub(crate) fn indexed_joints(bytes: &[u8]) -> Result<Vec<u16>> {
    let (_, tracks) = source(bytes)?;
    let mut indices = tracks.iter().map(|track| track.node).collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    ensure!(
        !indices.is_empty() && indices.len() <= 512,
        "invalid field motion joints ({})",
        indices.len()
    );
    Ok(indices)
}

/// Pose chains follow native key counts; glTF baking may simplify those keys.
pub(crate) fn indexed_animated_joints(bytes: &[u8]) -> Result<BTreeSet<u16>> {
    Ok(source(bytes)?
        .1
        .into_iter()
        .filter(|track| track.times.len() > 2)
        .map(|track| track.node)
        .collect())
}

/// Native labels for an ownerless motion, with stable numeric names where its
/// optional name table is absent or unusable. Pass this map to encode_named_indexed.
pub(crate) fn indexed_track_names(bytes: &[u8]) -> Result<BTreeMap<u16, String>> {
    let (_, tracks) = source(bytes)?;
    let labels = source_names(bytes, tracks.len()).unwrap_or_default();
    let numeric: BTreeSet<_> = tracks
        .iter()
        .map(|track| format!("joint-{}", track.node))
        .collect();
    let mut names = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (index, track) in tracks.iter().enumerate() {
        if names.contains_key(&track.node) {
            continue;
        }
        let fallback = format!("joint-{}", track.node);
        let name = labels
            .get(index)
            .filter(|name| {
                !name.is_empty()
                    && (!numeric.contains(*name) || **name == fallback)
                    && used.insert((*name).clone())
            })
            .cloned()
            .unwrap_or_else(|| {
                used.insert(fallback.clone());
                fallback
            });
        names.insert(track.node, name);
    }
    Ok(names)
}

/// Encode an ownerless bank with its native joint labels where available.
pub(crate) fn encode_named_indexed(bytes: &[u8], names: &BTreeMap<u16, String>) -> Result<Vec<u8>> {
    let indices = indexed_joints(bytes)?;
    let skeleton = Skeleton {
        bones: indices
            .iter()
            .map(|index| Bone {
                name: names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("joint-{index}")),
                shadow_bounds: false,
                parent: None,
                parent_transform: None,
                bind: Transform::default(),
            })
            .collect(),
    };
    encode_with_skeleton(bytes, skeleton, Some(&indices), None)
}

fn source_names(bytes: &[u8], count: usize) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut cursor = dol::u32(bytes, 20)? as usize;
    if cursor != 0 && dol::u32(bytes, 16)? != 0 {
        for _ in 0..count {
            let tail = bytes
                .get(cursor..)
                .context("animation name table exceeds source")?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated animation joint name")?;
            names.push(tail[..end].escape_ascii().to_string());
            cursor += end + 1;
        }
    }
    Ok(names)
}

/// Names a native bank's tracks exactly as the shared GLB encoder will, using
/// the reference model's native ID, cooked target name, and raw joint name.
pub(crate) fn shared_track_names(
    bytes: &[u8],
    owner: &[(u16, String, String)],
) -> Result<BTreeMap<u16, String>> {
    let (_, tracks) = source(bytes)?;
    let labels = shared_track_labels(bytes, owner)?;
    Ok(tracks
        .iter()
        .zip(labels)
        .fold(BTreeMap::new(), |mut names, (track, label)| {
            names.entry(track.node).or_insert(label);
            names
        }))
}

fn shared_track_labels(bytes: &[u8], owner: &[(u16, String, String)]) -> Result<Vec<String>> {
    let (_, tracks) = source(bytes)?;
    // Some native banks advertise a truncated name table. Their numeric joint
    // IDs remain valid, so bind by ID as the indexed motion writer does.
    let names = source_names(bytes, tracks.len()).unwrap_or_default();
    let use_names = names
        .iter()
        .any(|name| owner.iter().any(|(_, _, raw)| raw == name));
    let readable = if use_names {
        crate::world_gpl::animation_target_bindings(
            tracks
                .iter()
                .zip(&names)
                .map(|(track, name)| (track.node, name.clone()))
                .collect(),
        )
    } else {
        Vec::new()
    };
    let mut used = owner
        .iter()
        .map(|(_, target, _)| target.clone())
        .collect::<BTreeSet<_>>();
    let mut output = Vec::with_capacity(tracks.len());
    let mut bound = BTreeSet::new();
    for (index, track) in tracks.iter().enumerate() {
        let matched = if use_names {
            owner.iter().find(|(_, _, raw)| raw == &names[index])
        } else {
            owner.iter().find(|(id, _, _)| *id == track.node)
        };
        // The native receiver uses the first track for each actual target.
        // Keep later tracks as separate editable nodes without replacing it.
        if let Some((_, target, _)) = matched
            && bound.insert(target.clone())
        {
            output.push(target.clone());
            continue;
        }
        let base = matched
            .map(|(_, target, _)| target.clone())
            .or_else(|| readable.get(index).map(|(_, name)| name.clone()))
            .unwrap_or_else(|| format!("joint-{}", track.node));
        let mut name = base.clone();
        if !used.insert(name.clone()) {
            name = format!("{base}-source-{}", track.node);
            for suffix in 2.. {
                if used.insert(name.clone()) {
                    break;
                }
                name = format!("{base}-source-{}-{suffix}", track.node);
            }
        }
        output.push(name);
    }
    Ok(output)
}

/// Bind a motion to its reference body while retaining every native track.
/// Joints absent from this body remain editable as additional named GLB nodes.
pub fn encode_bound(bytes: &[u8], model_glb: &[u8], bindings: &[(u16, String)]) -> Result<Vec<u8>> {
    let mut skeleton = Skeleton::from_glb(model_glb)?;
    ensure!(
        bindings.len() == skeleton.bones.len(),
        "native model and GLB bone counts differ"
    );
    let (_, source) = source(bytes)?;
    let owner = bindings
        .iter()
        .zip(&skeleton.bones)
        .map(|((id, raw), bone)| (*id, bone.name.clone(), raw.clone()))
        .collect::<Vec<_>>();
    let labels = shared_track_labels(bytes, &owner)?;
    let mut track_bones = Vec::with_capacity(source.len());
    for label in labels {
        let bone = if let Some(index) = skeleton.bones.iter().position(|bone| bone.name == label) {
            index
        } else {
            let index = skeleton.bones.len();
            skeleton.bones.push(Bone {
                name: label,
                shadow_bounds: false,
                parent: None,
                parent_transform: None,
                bind: Transform::default(),
            });
            index
        };
        track_bones.push(bone);
    }
    encode_with_skeleton(bytes, skeleton, None, Some(&track_bones))
}

fn encode_with_skeleton(
    bytes: &[u8],
    skeleton: Skeleton,
    indices: Option<&[u16]>,
    track_bones: Option<&[usize]>,
) -> Result<Vec<u8>> {
    let (duration, source) = source(bytes)?;
    let selected: Vec<(usize, &SourceTrack)> = if let Some(bones) = track_bones {
        ensure!(
            bones.len() == source.len(),
            "shared motion target count differs from tracks"
        );
        source
            .iter()
            .zip(bones)
            .map(|(track, &bone)| (bone, track))
            .collect()
    } else {
        source
            .iter()
            .map(|track| {
                let index = indices
                    .map(|ids| {
                        ids.binary_search(&track.node)
                            .expect("indexed motion joint")
                    })
                    .unwrap_or(usize::from(track.node));
                (index, track)
            })
            .collect()
    };
    let tracks = selected
        .into_iter()
        .map(|(index, track)| {
            let mut node = NodeTrack::new(
                skeleton
                    .bones
                    .get(index)
                    .context("world motion joint exceeds model")?
                    .bind,
            );
            if let Some(curve) = &track.scale {
                node.scale = Some(bake(track, false, 0.0005, |a, b, t| {
                    vector(curve, a, b, t)
                })?);
            }
            if let Some(curve) = &track.rotation {
                node.rotation = Some(bake(track, true, 0.00001, |a, b, t| squad(curve, a, b, t))?);
            }
            if let Some(curve) = &track.translation {
                node.translation = Some(bake(track, false, 0.0005, |a, b, t| {
                    vector(curve, a, b, t)
                })?);
            }
            Ok(Track {
                bone: index as u16,
                nodes: vec![node],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Motion {
        duration_frames: duration,
        tracks,
    }
    .encode(&skeleton)
}
