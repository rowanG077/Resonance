//! Sample the title's feather attachments and bind its shared effect atlas.
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use std::path::Path;

/// Sample already-baked glTF channels. This shares the exact skeletal data used
/// by the renderer, including parent transforms and actor placement.
pub fn positions(
    gltf: &Value,
    binary: &[u8],
    name: &str,
    placement: Vec3,
    clip: usize,
    steps: usize,
) -> Result<Vec<[f32; 3]>> {
    let nodes = gltf["nodes"].as_array().context("glow nodes")?;
    let node = nodes
        .iter()
        .position(|n| n["name"] == name)
        .context("glow attachment is missing")?;
    let mut parents = vec![None; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(children) = n["children"].as_array() {
            for child in children {
                parents[child.as_u64().context("child index")? as usize] = Some(i);
            }
        }
    }
    let read = |accessor: usize, sample: usize, width: usize| -> Result<Vec<f32>> {
        let a = &gltf["accessors"][accessor];
        ensure!(
            a["componentType"] == 5126,
            "glow channel must contain floats"
        );
        let count = a["count"].as_u64().context("sample count")? as usize;
        ensure!(sample < count, "glow sample exceeds clip");
        let view = &gltf["bufferViews"][a["bufferView"].as_u64().context("view index")? as usize];
        let at = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + a["byteOffset"].as_u64().unwrap_or(0) as usize
            + sample * width * 4;
        (0..width)
            .map(|i| {
                Ok(f32::from_le_bytes(
                    binary
                        .get(at + i * 4..at + i * 4 + 4)
                        .context("truncated glow channel")?
                        .try_into()?,
                ))
            })
            .collect()
    };
    let initial = |n: &Value, key: &str, fallback: &[f32]| -> Result<Vec<f32>> {
        if n[key].is_null() {
            return Ok(fallback.to_vec());
        }
        n[key]
            .as_array()
            .context("node transform")?
            .iter()
            .map(|v| Ok(v.as_f64().context("transform value")? as f32))
            .collect()
    };
    let animation = &gltf["animations"][clip];
    let channels = animation["channels"]
        .as_array()
        .context("glow animation channels")?
        .iter()
        .map(|channel| {
            let target = channel["target"]["node"].as_u64().context("channel node")? as usize;
            let path = channel["target"]["path"].as_str().context("channel path")?;
            let sampler = &animation["samplers"]
                [channel["sampler"].as_u64().context("sampler index")? as usize];
            ensure!(
                sampler["interpolation"] == "LINEAR",
                "expected baked linear animation"
            );
            let input = sampler["input"].as_u64().context("channel times")? as usize;
            let count = gltf["accessors"][input]["count"]
                .as_u64()
                .context("time count")? as usize;
            let times = (0..count)
                .map(|index| Ok(read(input, index, 1)?[0]))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                !times.is_empty()
                    && times.iter().all(|time| time.is_finite())
                    && times.windows(2).all(|pair| pair[0] < pair[1]),
                "invalid glow animation timestamps"
            );
            Ok((
                target,
                path,
                sampler["output"].as_u64().context("channel output")? as usize,
                times,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut output = Vec::new();
    for tick in 0..=steps {
        let mut transforms = nodes
            .iter()
            .map(|n| {
                Ok((
                    initial(n, "translation", &[0.; 3])?,
                    initial(n, "rotation", &[0., 0., 0., 1.])?,
                    initial(n, "scale", &[1.; 3])?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        // Baked key density is independent of the game's attachment clock.
        let time = tick as f32 / resonance_content::ANIMATION_HZ;
        for &(target, path, output, ref times) in &channels {
            let left = times.partition_point(|&key| key <= time).saturating_sub(1);
            let right = (left + 1).min(times.len() - 1);
            let width = if path == "rotation" { 4 } else { 3 };
            let a = read(output, left, width)?;
            let data = if left == right || time <= times[left] {
                a
            } else {
                let b = read(output, right, width)?;
                let weight = (time - times[left]) / (times[right] - times[left]);
                if path == "rotation" {
                    Quat::from_slice(&a)
                        .slerp(Quat::from_slice(&b), weight)
                        .to_array()
                        .to_vec()
                } else {
                    a.iter().zip(b).map(|(a, b)| a + (b - a) * weight).collect()
                }
            };
            match path {
                "translation" => transforms[target].0 = data,
                "rotation" => transforms[target].1 = data,
                "scale" => transforms[target].2 = data,
                _ => anyhow::bail!("unsupported glow channel"),
            }
        }
        let mut current = Some(node);
        let mut matrix = Mat4::IDENTITY;
        let mut depth = 0;
        while let Some(index) = current {
            ensure!(depth < nodes.len(), "cycle in glow hierarchy");
            let (t, r, s) = &transforms[index];
            matrix = Mat4::from_scale_rotation_translation(
                Vec3::from_slice(s),
                Quat::from_slice(r),
                Vec3::from_slice(t),
            ) * matrix;
            current = parents[index];
            depth += 1;
        }
        let point = matrix.transform_point3(Vec3::ZERO) + placement;
        ensure!(point.is_finite(), "invalid glow position");
        // native_cc passes integer world coordinates to the particle constructor.
        output.push(point.trunc().to_array());
    }
    Ok(output)
}

pub(crate) fn texture(
    output: &Path,
    disc: u8,
    resource: &crate::scene::title::Resource,
) -> Result<String> {
    let textures = resource.bind(output, disc)?.cabinet_textures()?;
    let image = textures
        .get(2)
        .context("effect atlas 2 missing")?
        .image(0)?;
    ensure!(
        image.width == 256 && image.height == 256,
        "unexpected glow atlas dimensions"
    );
    Ok(image.path)
}

#[test]
#[cfg(unix)]
#[ignore = "requires both discs' cooked publications; no source reads or texture conversion"]
fn original_glow_binding_requires_only_cooked_source_identity() -> Result<()> {
    use std::{fs, os::unix::fs::symlink};
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let cooked = local.join("all-assets");
    let root = crate::temporary_path(&std::env::temp_dir().join("glow-declaration"));
    let result = (|| -> Result<()> {
        fs::create_dir_all(root.join("cooked"))?;
        symlink(cooked.join("assets"), root.join("cooked/assets"))?;
        for disc in [1, 2] {
            let recipe = crate::scene::title::Recipe::bind(&cooked, disc)?;
            let expected = texture(&cooked, disc, &recipe.effects)?;
            let publications = crate::cooked::Source::open(&cooked, disc, &recipe.effects.path)?;
            fs::write(
                root.join("cooked/sources.json"),
                serde_json::to_vec(&std::collections::BTreeMap::from([(
                    format!("disc{disc}/Glow.cab"),
                    publications.publications(),
                )]))?,
            )?;
            let mut renamed = crate::scene::title::Resource {
                path: "Glow.cab".into(),
                sha256: recipe.effects.sha256,
            };
            assert_eq!(texture(&root.join("cooked"), disc, &renamed)?, expected);
            renamed.path = "missing.cab".into();
            assert!(texture(&root.join("cooked"), disc, &renamed).is_err());
            renamed.path = "Glow.cab".into();
            renamed.sha256 = "0".repeat(64);
            assert!(texture(&root.join("cooked"), disc, &renamed).is_err());
        }
        Ok(())
    })();
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    result
}

#[test]
fn attachment_sampling_uses_timestamps_instead_of_baked_key_indices() -> Result<()> {
    use serde_json::json;
    for intervals in [1, 2, 4] {
        let mut bytes = Vec::new();
        for key in 0..=intervals {
            bytes.extend_from_slice(
                &(key as f32 / intervals as f32 * 2. / resonance_content::ANIMATION_HZ)
                    .to_le_bytes(),
            );
        }
        let values_at = bytes.len();
        for key in 0..=intervals {
            for value in [key as f32 / intervals as f32 * 2., 0., 0.] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let gltf = json!({
            "nodes": [{"name": "attachment"}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": intervals + 1},
                {"bufferView": 1, "componentType": 5126, "count": intervals + 1}
            ],
            "bufferViews": [{"byteOffset": 0}, {"byteOffset": values_at}],
            "animations": [{
                "channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}],
                "samplers": [{"input": 0, "output": 1, "interpolation": "LINEAR"}]
            }]
        });
        assert_eq!(
            positions(&gltf, &bytes, "attachment", Vec3::ZERO, 0, 2)?,
            [[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]]
        );
    }
    Ok(())
}
