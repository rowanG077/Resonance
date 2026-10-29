//! Cook named hair, scarf, and clothing chains into joint dynamics.
mod names;
mod tuning;
use anyhow::{Context, Result, ensure};
use resonance_content::secondary_motion::{Chain, Definition, Joint};
use serde_json::Value;

/// Native chain-root labels paired with the same readable names written to model.json.
pub(crate) fn chain_roots(raw_names: &[String]) -> Vec<(String, String)> {
    let mut used = std::collections::BTreeSet::new();
    raw_names
        .iter()
        .filter(|name| name.starts_with("AB_ROOT"))
        .map(|name| (name.clone(), names::name(name, &mut used)))
        .collect()
}

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

pub(crate) fn bind(
    model: &str,
    gltf: &Value,
    names: &[String],
    friendly: &[String],
) -> Result<Definition> {
    ensure!(
        names.len() == friendly.len(),
        "native and cooked skeletons differ"
    );
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
    let mut used = std::collections::BTreeSet::new();
    Definition::from_chains(
        chains
            .into_iter()
            .map(|chain| {
                let label = names::name(&names[usize::from(chain.joints[0].node)], &mut used);
                (label, chain)
            })
            .collect(),
        friendly,
    )
}
