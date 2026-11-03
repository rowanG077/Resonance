//! Bind shared enemy controller names to the spelling in the imported source rig.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_action::{EnemyBehavior, EnemyPolicy};

fn source_name(name: &str, nodes: &[String]) -> Result<String> {
    if nodes.iter().any(|node| node == name) {
        return Ok(name.to_owned());
    }
    let mut matches = nodes.iter().filter(|node| node.eq_ignore_ascii_case(name));
    let found = matches
        .next()
        .with_context(|| format!("enemy controller bone {name} is absent from its source rig"))?;
    ensure!(
        matches.next().is_none(),
        "enemy controller bone {name} has ambiguous source spelling"
    );
    Ok(found.clone())
}

pub(super) fn bind(policy: &mut EnemyPolicy, nodes: &[String]) -> Result<()> {
    match policy.behavior.as_mut() {
        Some(EnemyBehavior::Transformation { revealed_bones, .. }) => {
            for name in revealed_bones {
                *name = source_name(name, nodes)?;
            }
        }
        Some(EnemyBehavior::ModelParts { bone_scales }) => {
            for (name, _) in bone_scales {
                *name = source_name(name, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_names_bind_existing_source_spelling_without_fabricating_bones() -> Result<()> {
        let nodes = vec!["SC_BONE_UDE_L".into(), "sc_Bone_koshi".into()];
        assert_eq!(source_name("sc_Bone_ude_L", &nodes)?, "SC_BONE_UDE_L");
        assert_eq!(source_name("sc_Bone_koshi", &nodes)?, "sc_Bone_koshi");
        assert!(source_name("missing", &nodes).is_err());
        assert!(source_name("Bone", &["BONE".into(), "bone".into()]).is_err());
        Ok(())
    }
}
