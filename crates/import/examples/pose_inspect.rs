//! Compare cooked animation keys with independently observed local joint poses.
//! Read-only diagnostics: no savestate data becomes a cooked game asset.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let glb = fs::read(args.next().context("GLB path")?)?;
    let observations: Value =
        serde_json::from_slice(&fs::read(args.next().context("observation JSON")?)?)?;
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(glb.get(at..at + 4).context("GLB range")?.try_into()?) as usize)
    };
    ensure!(&glb[..4] == b"glTF", "expected GLB");
    let json_end = 20 + word(12)?;
    let json: Value = serde_json::from_slice(&glb[20..json_end])?;
    let binary = &glb[json_end + 8..];
    let values = |accessor: &Value, key: usize, width: usize| -> Result<Vec<f64>> {
        let view = &json["bufferViews"][accessor["bufferView"].as_u64().context("view")? as usize];
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + accessor["byteOffset"].as_u64().unwrap_or(0) as usize
            + key * width * 4;
        binary
            .get(offset..offset + width * 4)
            .context("key range")?
            .chunks_exact(4)
            .map(|b| Ok(f64::from(f32::from_le_bytes(b.try_into()?))))
            .collect()
    };
    let nodes = observations["controlled_actor"]["model_nodes"]
        .as_array()
        .context("actor nodes")?;
    let secondary: std::collections::BTreeSet<_> =
        observations["controlled_actor"]["secondary_chains"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|chain| chain["joints"].as_array().into_iter().flatten())
            .filter_map(|joint| joint["node"].as_u64())
            .collect();
    let mut ranked = Vec::new();
    for animation in json["animations"].as_array().context("animations")? {
        let channels = animation["channels"].as_array().context("channels")?;
        let first = &animation["samplers"][0];
        let count = json["accessors"][first["input"].as_u64().unwrap() as usize]["count"]
            .as_u64()
            .unwrap() as usize;
        let mut best = (f64::INFINITY, 0);
        for key in 0..count {
            let mut error = 0.;
            let mut terms = 0;
            for channel in channels {
                let index = channel["target"]["node"].as_u64().unwrap() as usize;
                // Primary skeleton only; simulated accessory chains have their
                // own dynamics and are not a valid animation identity test.
                if secondary.contains(&(index as u64)) {
                    continue;
                }
                let Some(node) = nodes.get(index) else {
                    continue;
                };
                let path = channel["target"]["path"].as_str().unwrap();
                let (field, width, flag) = match path {
                    "translation" => ("animated_translation", 3, 8),
                    "rotation" => ("animated_rotation", 4, 4),
                    _ => continue,
                };
                if node["animated_flags"].as_u64().unwrap_or(0) & flag == 0 {
                    continue;
                }
                let sample = &animation["samplers"][channel["sampler"].as_u64().unwrap() as usize];
                let accessor = &json["accessors"][sample["output"].as_u64().unwrap() as usize];
                let actual = values(accessor, key, width)?;
                let expected: Vec<_> = node[field]
                    .as_array()
                    .context("pose")?
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                error += if width == 4 {
                    let dot: f64 = actual.iter().zip(&expected).map(|(a, b)| a * b).sum();
                    (1. - dot.abs().min(1.)) * 400.
                } else {
                    actual
                        .iter()
                        .zip(&expected)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum()
                };
                terms += 1;
            }
            let error = error / f64::from(terms);
            if error < best.0 {
                best = (error, key);
            }
        }
        ranked.push((best.0, best.1, animation["name"].clone()));
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("{}", serde_json::to_string_pretty(&ranked)?);
    Ok(())
}
