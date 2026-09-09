//! Compile the title's feather attachment paths and effect atlas offline.
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

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
        for channel in animation["channels"]
            .as_array()
            .context("glow animation channels")?
        {
            let target = channel["target"]["node"].as_u64().context("channel node")? as usize;
            let path = channel["target"]["path"].as_str().context("channel path")?;
            let sampler = &animation["samplers"]
                [channel["sampler"].as_u64().context("sampler index")? as usize];
            let data = read(
                sampler["output"].as_u64().context("channel output")? as usize,
                tick,
                if path == "rotation" { 4 } else { 3 },
            )?;
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

pub fn texture(source: &Path, output: &Path, ktx: &Path) -> Result<(String, String)> {
    let source = fs::read(source)?;
    let mut cabinet = cab::Cabinet::new(Cursor::new(&source))?;
    let mut tpl = Vec::new();
    cabinet
        .read_file("EFFECT.TPL")?
        .take(16 * 1024 * 1024)
        .read_to_end(&mut tpl)?;
    // The glow uses image 2 from EFFECT.TPL.
    let (width, height, pixels) = crate::tpl::decode(&tpl)?
        .into_iter()
        .nth(2)
        .context("effect atlas 2 missing")?;
    ensure!(
        width == 256 && height == 256,
        "unexpected glow atlas dimensions"
    );
    let intermediate = output.join("intermediate/glow");
    fs::create_dir_all(&intermediate)?;
    let png = intermediate.join("atlas.png");
    image::save_buffer(&png, &pixels, width, height, image::ColorType::Rgba8)?;
    let path = "title/glow.ktx2";
    crate::texture::cook(ktx, &png, &output.join(path))?;
    Ok((path.into(), crate::digest(&source)))
}
