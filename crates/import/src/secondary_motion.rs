//! Cook named hair, scarf, and clothing chains into joint dynamics.
use anyhow::{Context, Result, ensure};
use resonance_content::secondary_motion::{Chain, Definition, Joint};
use serde_json::Value;

fn parameter(name: &str, tag: &str) -> Result<Option<f32>> {
    let Some((_, value)) = name.split_once(tag) else {
        return Ok(None);
    };
    let value: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | 'e' | '.'))
        .map(|c| if c == 'e' { '.' } else { c })
        .collect();
    let value: f32 = value.parse().context("invalid embedded chain parameter")?;
    ensure!(value.is_finite(), "nonfinite embedded chain parameter");
    Ok((value != 0.).then_some(value))
}

pub(crate) fn cook(source: &[u8], gltf: &Value, names: &[String]) -> Result<Definition> {
    let mut definition = bind("", gltf, names)?;
    if !definition.chains.is_empty() {
        let model = source
            .get(crate::read::u32(source, 4)? as usize..)
            .context("chain model")?;
        let name = model
            .get(crate::read::u32(model, 16)? as usize..)
            .context("chain model name")?;
        let end = name
            .iter()
            .position(|&b| b == 0)
            .context("unterminated model name")?;
        definition.model = std::str::from_utf8(&name[..end])?.to_owned();
    }
    Ok(definition)
}

pub(crate) fn bind(model: &str, gltf: &Value, names: &[String]) -> Result<Definition> {
    let nodes = gltf["nodes"].as_array().context("chain skeleton")?;
    ensure!(nodes.len() >= names.len(), "incomplete chain skeleton");
    let mut chains = Vec::new();
    for (index, name) in names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.starts_with("AB_ROOT"))
    {
        let mut chain = Chain {
            joints: Vec::new(),
            attraction: 0.,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        let mut node = index;
        let gravity = parameter(name, "_Dt")?.unwrap_or(1.2);
        let damping = parameter(name, "_Bb")?.unwrap_or(0.1);
        let mut attraction = 0.019;
        let mut follows_pose = false;
        loop {
            ensure!(
                chain.joints.len() < 128
                    && !chain.joints.iter().any(|j| usize::from(j.node) == node),
                "cyclic or oversized bone chain"
            );
            let name = names.get(node).context("chain joint outside skeleton")?;
            chain.joints.push(Joint {
                node: node.try_into()?,
                gravity: parameter(name, "_Dt")?.unwrap_or(gravity),
                damping: parameter(name, "_Bb")?.unwrap_or(damping),
            });
            attraction = parameter(name, "_Pow")?.unwrap_or(attraction);
            follows_pose |= name.contains("_FP_");
            chain.preserve_rotation |= name.contains("_NR_");
            let Some(child) = nodes[node]["children"].as_array().and_then(|c| c.first()) else {
                break;
            };
            node = child.as_u64().context("invalid chain child")?.try_into()?;
        }
        chain.attraction = if follows_pose { attraction } else { 0. };
        chain.validate(names.len())?;
        chains.push(chain);
    }
    if chains.is_empty() {
        return Ok(Definition::default());
    }
    Ok(Definition {
        model: model.to_owned(),
        chains,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_identity_does_not_change_authored_chain_decoding() -> Result<()> {
        let names = vec![
            "AB_ROOT_NR_FP_01_kami".into(),
            "AB_Dt-0e5_Bb0e4_Pow0e1".into(),
        ];
        let gltf = serde_json::json!({"nodes": [{"children": [1]}, {}]});
        let mut reference = None;
        for model in ["llo00", "col00", "ref00", "new_model"] {
            let mut source = vec![0; 44];
            source[4..8].copy_from_slice(&12_u32.to_be_bytes());
            source[28..32].copy_from_slice(&32_u32.to_be_bytes());
            source.extend_from_slice(model.as_bytes());
            source.push(0);
            let definition = cook(&source, &gltf, &names)?;
            assert_eq!(definition.model, model);
            let chain = &definition.chains[0];
            assert_eq!(chain.attraction, 0.1);
            assert_eq!(chain.joints[0].gravity, 1.2);
            assert_eq!(chain.joints[1].gravity, -0.5);
            assert!(chain.preserve_rotation && chain.collision_plane.is_none());
            let encoded = serde_json::to_vec(&definition.chains)?;
            assert_eq!(reference.get_or_insert_with(|| encoded.clone()), &encoded);
            source.pop();
            assert!(cook(&source, &gltf, &names).is_err());
        }
        assert!(cook(&[], &serde_json::json!({"nodes": []}), &names).is_err());
        Ok(())
    }

    #[test]
    fn encoded_decimal_parameters_preserve_sign_and_zero_fallback() {
        assert_eq!(
            parameter("AB_FP_Pow0e03_Bb0e766_Dt-0e733", "_Dt").unwrap(),
            Some(-0.733)
        );
        assert_eq!(parameter("AB_Pow0e03_Bb0e766", "_Pow").unwrap(), Some(0.03));
        assert_eq!(parameter("AB_Dt0", "_Dt").unwrap(), None);
        assert!(parameter("AB_Dt-", "_Dt").is_err());
    }
}
