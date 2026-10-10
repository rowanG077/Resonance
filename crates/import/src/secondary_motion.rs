//! Cook named hair, scarf, and clothing chains into joint dynamics.
mod tuning;
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
        tuning::apply(model, &mut chain, names)?;
        chains.push(chain);
    }
    Ok(Definition { chains })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_chains_decode_authored_dynamics_and_reject_invalid_skeletons() -> Result<()> {
        let names = vec![
            "AB_ROOT_NR_FP_01_kami".into(),
            "AB_Dt-0e5_Bb0e4_Pow0e1".into(),
        ];
        let gltf = serde_json::json!({"nodes": [{"children": [1]}, {}]});
        let definition = bind("new_model", &gltf, &names)?;
        let chain = &definition.chains[0];
        assert_eq!(chain.attraction, 0.1);
        assert_eq!(chain.joints[0].gravity, 1.2);
        assert_eq!(chain.joints[1].gravity, -0.5);
        assert!(chain.preserve_rotation && chain.collision_plane.is_none());
        assert!(bind("new_model", &serde_json::json!({"nodes": []}), &names).is_err());
        let cycle = serde_json::json!({"nodes": [{"children": [1]}, {"children": [0]}]});
        assert!(bind("new_model", &cycle, &names).is_err());
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
