//! Bake skeletal curves into ordinary glTF channels; evaluate source curves
//! offline so the runtime only interpolates cooked samples.
use crate::read::{f32 as f32_at, u16 as u16_at, u32 as u32_at};
use anyhow::{Context, Result, ensure};
use glam::{Vec3, Vec4};
use serde_json::{Value, json};

struct Key {
    time: f32,
    values: usize,
    tangents: usize,
}
struct Track {
    period: f32,
    node: usize,
    format: u8,
    flags: u8,
    interpolation: u8,
    keys: Vec<Key>,
}

fn tracks(bytes: &[u8]) -> Result<Vec<Track>> {
    ensure!(
        u32_at(bytes, 0)? == 0x007b7960 && u32_at(bytes, 4)? == 24,
        "invalid animation header"
    );
    let a_count = usize::from(u16_at(bytes, 10)?);
    let b_count = usize::from(u16_at(bytes, 12)?);
    let c_count = usize::from(u16_at(bytes, 14)?);
    ensure!(
        a_count == 1 && b_count > 0 && b_count <= 256,
        "unsupported animation table layout"
    );
    let b_start = 24 + a_count * 12;
    let c_start = b_start + b_count * 16;
    let c_end = c_start + c_count * 12;
    ensure!(c_end <= bytes.len(), "animation tables exceed resource");
    ensure!(
        usize::from(u16_at(bytes, 32)?) == b_count,
        "animation descriptor count mismatch"
    );
    let starts = (0..b_count)
        .map(|i| u32_at(bytes, b_start + i * 16 + 4).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?;
    let mut tracks = Vec::new();
    for (i, start) in starts.iter().copied().enumerate() {
        let end = starts
            .iter()
            .copied()
            .filter(|v| *v > start)
            .min()
            .unwrap_or(c_end);
        ensure!(
            start >= c_start && end <= c_end && end > start && (end - start).is_multiple_of(12),
            "invalid animation key range"
        );
        let at = b_start + i * 16;
        let period = f32_at(bytes, at)?;
        ensure!(period > 0. && period <= 18000., "invalid animation period");
        let mut keys = Vec::new();
        for key in (start..end).step_by(12) {
            let time = f32_at(bytes, key)?;
            ensure!(
                time >= 0. && time <= period && keys.last().is_none_or(|k: &Key| k.time < time),
                "invalid animation key times"
            );
            keys.push(Key {
                time,
                values: u32_at(bytes, key + 4)? as usize,
                tangents: u32_at(bytes, key + 8)? as usize,
            });
        }
        ensure!(keys[0].time == 0., "animation must start at zero");
        let format = bytes[at + 12];
        let flags = bytes[at + 13];
        ensure!(
            format >> 4 == 3 && flags & !11 == 0 && bytes[at + 15] == 1,
            "unsupported title animation encoding"
        );
        tracks.push(Track {
            period,
            node: usize::from(u16_at(bytes, at + 10)?),
            format,
            flags,
            interpolation: bytes[at + 14],
            keys,
        });
    }
    Ok(tracks)
}

fn vector<const N: usize>(bytes: &[u8], at: usize, scale: f32) -> Result<[f32; N]> {
    ensure!(at > 0, "missing animation value/tangent data");
    let mut values = [0.; N];
    for (i, v) in values.iter_mut().enumerate() {
        *v = f32::from(u16_at(bytes, at + i * 2)? as i16) * scale;
    }
    Ok(values)
}

fn cubic(a: Vec3, b: Vec3, outgoing: Vec3, incoming: Vec3, t: f32) -> Vec3 {
    // Stored tangents are offsets of Bezier control points, not derivatives.
    let u = 1. - t;
    a * (u * u * u)
        + (a + outgoing) * (3. * u * u * t)
        + (b + incoming) * (3. * u * t * t)
        + b * (t * t * t)
}

#[allow(clippy::too_many_arguments)] // Paired keys and their packed component layout.
fn vector_curve(
    bytes: &[u8],
    a: &Key,
    b: &Key,
    value_offset: usize,
    tangent_offset: usize,
    scale: f32,
    mode: u8,
    t: f32,
) -> Result<Vec3> {
    let av = Vec3::from_array(vector(bytes, a.values + value_offset, scale)?);
    let bv = Vec3::from_array(vector(bytes, b.values + value_offset, scale)?);
    Ok(match mode {
        0 => av,
        1 => av.lerp(bv, t),
        2 => {
            ensure!(a.tangents > 0 && b.tangents > 0, "missing Bezier tangents");
            cubic(
                av,
                bv,
                Vec3::from_array(vector(bytes, a.tangents + tangent_offset + 6, scale)?),
                Vec3::from_array(vector(bytes, b.tangents + tangent_offset, scale)?),
                t,
            )
        }
        3 => {
            ensure!(a.tangents > 0 && b.tangents > 0, "missing Hermite tangents");
            let outgoing = Vec3::from_array(vector(bytes, a.tangents + tangent_offset + 6, scale)?);
            let incoming = Vec3::from_array(vector(bytes, b.tangents + tangent_offset, scale)?);
            let ease_out = vector::<1>(bytes, a.tangents + tangent_offset + 14, 1. / 16384.)?[0];
            let ease_in = vector::<1>(bytes, b.tangents + tangent_offset + 12, 1. / 16384.)?[0];
            ensure!(
                ease_out == 0. && ease_in == 0.,
                "nonzero vector ease needs validation"
            );
            hermite(av, bv, outgoing, incoming, t)
        }
        _ => anyhow::bail!("unsupported title vector interpolation {mode}"),
    })
}

fn hermite(a: Vec3, b: Vec3, outgoing: Vec3, incoming: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    a * (2. * t3 - 3. * t2 + 1.)
        + b * (3. * t2 - 2. * t3)
        + outgoing * (t3 - 2. * t2 + t)
        + incoming * (t3 - t2)
}

fn spherical(a: Vec4, b: Vec4, t: f32) -> Vec4 {
    // The original quaternion interpolator keeps the authored arc, including
    // negative dot products. Generic shortest-path slerp changes these clips.
    let dot = a.dot(b).clamp(-1., 1.);
    if 1. + dot <= 0.000001 {
        let orthogonal = Vec4::new(-b.y, b.x, -b.w, b.z);
        let angle = std::f32::consts::FRAC_PI_2 * t;
        return a * angle.cos() + orthogonal * angle.sin();
    }
    if 1. - dot <= 0.000001 {
        return a.lerp(b, t);
    }
    let angle = dot.acos();
    (a * ((1. - t) * angle).sin() + b * (t * angle).sin()) / angle.sin()
}

impl Track {
    fn sample(&self, bytes: &[u8], time: f32, component: &str) -> Result<Vec<f32>> {
        let left = self
            .keys
            .partition_point(|k| k.time <= time)
            .saturating_sub(1);
        let a = &self.keys[left];
        let b = &self.keys[(left + 1) % self.keys.len()];
        let delta = if b.time > a.time {
            b.time - a.time
        } else {
            self.period - a.time
        };
        let mut t = if delta > 0. {
            (time - a.time) / delta
        } else {
            0.
        };
        let scale = 2f32.powi(-i32::from(self.format & 15));
        let scale_mode = (self.interpolation >> 2) & 3;
        let rotation_mode = (self.interpolation >> 4) & 7;
        let scale_tangents = if self.flags & 2 == 0 {
            0
        } else {
            match scale_mode {
                2 => 12,
                3 => 16,
                _ => 0,
            }
        };
        match component {
            "scale" => Ok(vector_curve(bytes, a, b, 0, 0, scale, scale_mode, t)?
                .to_array()
                .to_vec()),
            "translation" => {
                let value_offset =
                    usize::from(self.flags & 2 != 0) * 6 + usize::from(self.flags & 8 != 0) * 8;
                let rotation_tangents = if self.flags & 8 == 0 {
                    0
                } else {
                    match rotation_mode {
                        4 => 16,
                        5 => 20,
                        _ => 0,
                    }
                };
                Ok(vector_curve(
                    bytes,
                    a,
                    b,
                    value_offset,
                    scale_tangents + rotation_tangents,
                    scale,
                    self.interpolation & 3,
                    t,
                )?
                .to_array()
                .to_vec())
            }
            "rotation" => {
                let offset = usize::from(self.flags & 2 != 0) * 6;
                let av = Vec4::from_array(vector(bytes, a.values + offset, 1. / 16384.)?);
                let bv = Vec4::from_array(vector(bytes, b.values + offset, 1. / 16384.)?);
                let value = match rotation_mode {
                    0 => av,
                    // This interpolation mode takes the shortest arc; cubic quaternion curves
                    // retain their authored long arc.
                    6 => spherical(av, if av.dot(bv) < 0. { -bv } else { bv }, t),
                    4 | 5 => {
                        ensure!(
                            a.tangents > 0 && b.tangents > 0,
                            "missing quaternion controls"
                        );
                        if rotation_mode == 5 {
                            let incoming =
                                vector::<1>(bytes, b.tangents + scale_tangents + 16, 1. / 16384.)?
                                    [0];
                            let outgoing =
                                vector::<1>(bytes, a.tangents + scale_tangents + 18, 1. / 16384.)?
                                    [0];
                            // All title clips currently have zero easing. Reject
                            // new nonzero values until their scale is verified.
                            ensure!(
                                incoming == 0. && outgoing == 0.,
                                "nonzero quaternion ease needs validation"
                            );
                        }
                        t = t.clamp(0., 1.);
                        let ac = Vec4::from_array(vector(
                            bytes,
                            a.tangents + scale_tangents + 8,
                            1. / 16384.,
                        )?);
                        let bc = Vec4::from_array(vector(
                            bytes,
                            b.tangents + scale_tangents,
                            1. / 16384.,
                        )?);
                        spherical(
                            spherical(av, bv, t),
                            spherical(ac, bc, t),
                            2. * t * (1. - t),
                        )
                    }
                    _ => {
                        anyhow::bail!("unsupported title quaternion interpolation {rotation_mode}")
                    }
                };
                ensure!(
                    value.is_finite() && value.length_squared() > 0.5,
                    "invalid animation quaternion"
                );
                Ok(value.normalize().to_array().to_vec())
            }
            _ => anyhow::bail!("unknown animation component"),
        }
    }
}

fn accessor(gltf: &mut Value, binary: &mut Vec<u8>, data: &[f32], components: usize) -> usize {
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let offset = binary.len();
    for v in data {
        binary.extend(v.to_le_bytes());
    }
    let view = gltf["bufferViews"].as_array().unwrap().len();
    gltf["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":offset,"byteLength":data.len()*4}));
    let mut value = json!({"bufferView":view,"componentType":5126,"count":data.len()/components,"type":match components { 1 => "SCALAR",3 => "VEC3",4 => "VEC4",_ => unreachable!() }});
    if components == 1 {
        value["min"] = json!([data[0]]);
        value["max"] = json!([data[data.len() - 1]]);
    }
    let index = gltf["accessors"].as_array().unwrap().len();
    gltf["accessors"].as_array_mut().unwrap().push(value);
    index
}

/// Bake quarter-frame curve samples to ordinary animation channels. Slow clips
/// can advance by a quarter authored frame in one game update; interpolating
/// only half-frame keys visibly straightens their curved motion.
pub fn bake(
    section: &[u8],
    offset: usize,
    model_offset: usize,
    gltf: &mut Value,
    binary: &mut Vec<u8>,
    name: &str,
) -> Result<f32> {
    let bytes = section.get(offset..).context("animation outside section")?;
    let model = section
        .get(model_offset..)
        .context("model outside section")?;
    bake_with_model(bytes, model, gltf, binary, name)
}

/// Field groups store their default animation separately from the model.
pub fn bake_with_model(
    bytes: &[u8],
    model: &[u8],
    gltf: &mut Value,
    binary: &mut Vec<u8>,
    name: &str,
) -> Result<f32> {
    let tracks = tracks(bytes)?;
    let duration = tracks[0].period;
    let model_count = usize::from(u16_at(model, 6)?);
    let model_nodes = u32_at(model, 12)? as usize;
    let animation_names = names(bytes, u32_at(bytes, 20)? as usize, tracks.len())?;
    let model_names = names(model, u32_at(model, 28)? as usize, model_count)?;
    // If no bone name matches, use numeric track IDs for the entire clip.
    let use_names = u16_at(bytes, 34)? == 0
        && match (&animation_names, &model_names) {
            (Some(a), Some(m)) => a.iter().any(|name| m.contains(name)),
            _ => false,
        };
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    let mut secondary_pose_nodes = Vec::new();
    let mut missing = 0;
    for (index, track) in tracks.into_iter().enumerate() {
        ensure!(track.period == duration, "title clip has mixed periods");
        // Resolve named tracks against model bones. Omitted variant bones
        // receive no controller; numeric IDs are the fallback.
        let node = if use_names {
            let animation_names = animation_names.as_ref().expect("checked name binding");
            let model_names = model_names.as_ref().expect("checked name binding");
            model_names
                .iter()
                .position(|n| *n == animation_names[index])
        } else {
            (0..model_count).find(|i| {
                u16_at(model, model_nodes + i * 28 + 22)
                    .is_ok_and(|id| usize::from(id) == track.node)
            })
        };
        let Some(node) = node else {
            missing += 1;
            continue;
        };
        // Use the source track’s key count to identify static channels.
        if track.keys.len() > 2 {
            secondary_pose_nodes.push(node);
        }
        let steps = (duration * 4.).round() as usize;
        ensure!(
            steps as f32 == duration * 4.,
            "animation duration is not aligned to quarter frames"
        );
        let times = (0..=steps).map(|i| i as f32 / 120.).collect::<Vec<_>>();
        let input = accessor(gltf, binary, &times, 1);
        for (flag, component, width) in [(1, "translation", 3), (2, "scale", 3), (8, "rotation", 4)]
        {
            if track.flags & flag == 0 {
                continue;
            }
            let mut values = Vec::new();
            for i in 0..=steps {
                values.extend(track.sample(bytes, i as f32 * 0.25, component)?);
            }
            let output = accessor(gltf, binary, &values, width);
            channels
                .push(json!({"sampler":samplers.len(),"target":{"node":node,"path":component}}));
            samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
        }
    }
    if channels.is_empty() {
        // Props also carry placeholder clips for nodes absent from their
        // model. The source leaves those models at their bind pose while the
        // clip clock runs. glTF requires a channel, so retain that duration
        // with a constant bind translation.
        let translation = gltf["nodes"][0]
            .get("translation")
            .cloned()
            .unwrap_or_else(|| json!([0., 0., 0.]));
        let values: Vec<f32> = serde_json::from_value(translation)?;
        ensure!(
            values.len() == 3 && values.iter().all(|v| v.is_finite()),
            "invalid bind translation"
        );
        let input = accessor(gltf, binary, &[0., duration / 30.], 1);
        let output = accessor(gltf, binary, &[values.clone(), values].concat(), 3);
        channels.push(json!({"sampler":0,"target":{"node":0,"path":"translation"}}));
        samplers.push(json!({"input":input,"output":output,"interpolation":"LINEAR"}));
    }
    ensure!(
        !channels.is_empty(),
        "animation {name} has no bound channels"
    );
    if missing != 0 {
        eprintln!("Animation {name}: {missing} tracks have no matching bone in this model variant");
    }
    if gltf.get("animations").is_none() {
        gltf["animations"] = json!([]);
    }
    gltf["animations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":name,"channels":channels,"samplers":samplers,"extras":{"secondary_pose_nodes":secondary_pose_nodes}}));
    gltf["buffers"][0]["byteLength"] = json!(binary.len());
    Ok(duration / 30.)
}

fn names(bytes: &[u8], offset: usize, count: usize) -> Result<Option<Vec<&[u8]>>> {
    if offset == 0 {
        return Ok(None);
    }
    let mut remaining = bytes.get(offset..).context("name table exceeds resource")?;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let end = remaining
            .iter()
            .position(|b| *b == 0)
            .context("unterminated animation/model name")?;
        names.push(&remaining[..end]);
        remaining = &remaining[end + 1..];
    }
    Ok(Some(names))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bezier_controls_are_offsets_and_keep_endpoints() {
        let a = Vec3::ZERO;
        let b = Vec3::new(8., 0., 0.);
        let out = Vec3::new(0., 4., 0.);
        assert_eq!(cubic(a, b, out, Vec3::ZERO, 0.), a);
        assert_eq!(cubic(a, b, out, Vec3::ZERO, 1.), b);
        assert_eq!(cubic(a, b, out, Vec3::ZERO, 0.5), Vec3::new(4., 1.5, 0.));
    }
    #[test]
    fn spherical_curve_preserves_authored_long_arc() {
        let a = Vec4::W;
        let b = Vec4::new(0., 0., 0.8660254, -0.5);
        let mid = spherical(a, b, 0.5);
        assert!(mid.z > 0.86 && mid.w > 0.49);
    }
    #[test]
    fn malformed_animation_tables_are_rejected() {
        assert!(tracks(&[0; 24]).is_err());
        let mut data = vec![0; 36];
        data[0..4].copy_from_slice(&0x007b7960u32.to_be_bytes());
        data[4..8].copy_from_slice(&24u32.to_be_bytes());
        data[10..12].copy_from_slice(&1u16.to_be_bytes());
        data[12..14].copy_from_slice(&1u16.to_be_bytes());
        assert!(tracks(&data).is_err());
    }
}
