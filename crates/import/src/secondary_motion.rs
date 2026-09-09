//! Cook named hair, scarf, and clothing chains into joint dynamics.
use anyhow::{Context, Result, ensure};
use resonance_content::secondary_motion::{Chain, CollisionPlane, Joint};
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

pub(crate) fn cook(gltf: &Value, names: &[String], lloyd: bool) -> Result<Vec<Chain>> {
    let nodes = gltf["nodes"].as_array().context("chain skeleton")?;
    let spine = if lloyd {
        Some(
            names
                .iter()
                .position(|name| name.starts_with("Bone_sebone02"))
                .context("Lloyd chain collision anchor")? as u16,
        )
    } else {
        None
    };
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
            node = child.as_u64().context("invalid chain child")? as usize;
        }
        chain.attraction = if follows_pose { attraction } else { 0. };
        // Lloyd’s hair has separate dynamics; both scarf chains stay within
        // the spine’s local XZ half-space.
        if lloyd && name.starts_with("AB_ROOT_NR_FP_01_kami") {
            chain.attraction = 0.4165;
            for joint in &mut chain.joints {
                joint.gravity = -0.7333;
                joint.damping = 0.7666;
            }
        } else if let Some(spine) = spine
            && name.contains("manto_")
        {
            chain.collision_plane = Some(CollisionPlane {
                anchor: spine,
                normal: [0., -1., 0.],
                offset: 0.,
                strength: 1.,
            });
        }
        chain.validate(names.len())?;
        chains.push(chain);
    }
    ensure!(
        !lloyd || chains.len() == 3,
        "unexpected Lloyd secondary-motion rig"
    );
    Ok(chains)
}

/// Colette’s chain dynamics and body collision planes.
pub(crate) fn colette(chains: &mut [Chain], names: &[String]) -> Result<()> {
    let bone = |prefix: &str| -> Result<u16> {
        Ok(names
            .iter()
            .position(|name| name.starts_with(prefix))
            .context("Colette chain collision anchor")? as u16)
    };
    for chain in chains {
        let name = &names[usize::from(chain.joints[0].node)];
        let plane = if name.contains("_kami01") || name.contains("_manto02_") {
            chain.rotation_locks[1] = true;
            if name.contains("_kami01") {
                // DOL doubles 8035B7B8=1.66 and 8035B790=1.2.
                chain.joints[0].gravity = (f64::from(chain.joints[0].gravity) * 1.66) as f32;
                chain.joints[0].damping = (f64::from(chain.joints[0].damping) * 1.66) as f32;
                for index in 1..chain.joints.len() {
                    chain.joints[index].gravity =
                        (f64::from(chain.joints[index - 1].gravity) / 1.2) as f32;
                    chain.joints[index].damping =
                        (f64::from(chain.joints[index - 1].damping) / 1.2) as f32;
                }
            }
            Some(CollisionPlane {
                anchor: bone("Bone_sebone02")?,
                normal: [0., -1., 0.],
                offset: 1.,
                strength: 0.2,
            })
        } else if name.contains("_manto01_") {
            chain.rotation_locks[1] = true;
            Some(CollisionPlane {
                anchor: bone("Bone_sebone02")?,
                normal: [0., 1., 0.],
                offset: 0.,
                strength: 1.,
            })
        } else if name.contains("_kata_") {
            chain.rotation_locks[0] = true;
            Some(CollisionPlane {
                anchor: bone(if name.contains("_kata_L_") {
                    "Bone_ude01_L"
                } else {
                    "Bone_ude01_R"
                })?,
                normal: [0., 1., 0.],
                offset: 0.,
                strength: 1.,
            })
        } else {
            None
        };
        chain.collision_plane = plane;
        chain.validate(names.len())?;
    }
    Ok(())
}

/// Raine’s coat follows her spine and legs. Cook joint indices, dynamics,
/// and collision planes into the same format as other characters.
pub(crate) fn raine(chains: &mut [Chain], names: &[String]) -> Result<()> {
    let bone = |prefix: &str| -> Result<u16> {
        Ok(names
            .iter()
            .position(|name| name.starts_with(prefix))
            .with_context(|| format!("Raine chain collision anchor {prefix}"))? as u16)
    };
    for chain in chains {
        let name = &names[usize::from(chain.joints[0].node)];
        let back = name.contains("_manto02_") || name.contains("_manto03_");
        let front = name.contains("_manto04_");
        if back {
            chain.attraction = 0.008167;
            let (mut gravity, mut damping) = (1.6_f32, 0.633_f32);
            for joint in &mut chain.joints {
                joint.gravity = gravity;
                joint.damping = damping;
                gravity = (f64::from(gravity) / 1.2) as f32;
                damping = (f64::from(damping) / 1.2) as f32;
            }
        } else if front {
            chain.attraction = 0.04;
            for joint in &mut chain.joints {
                joint.gravity = 3.933;
                joint.damping = 0.666;
            }
        }
        let plane = if name.contains("_manto01_") {
            Some(("Bone_sebone03", [0., 1., 0.], -1., 0.4))
        } else if name.contains("_manto02_") {
            Some(("Bone_sebone03", [0., -1., 0.], 0., 1.))
        } else if name.contains("_manto03_") || front {
            chain.rotation_locks[1] = true;
            Some((
                if name.contains("_L_") {
                    "Bone_ashi01_L"
                } else {
                    "Bone_ashi01_R"
                },
                [0., if front { -1. } else { 1. }, 0.],
                if front { -1. } else { 0. },
                1.,
            ))
        } else {
            None
        };
        chain.collision_plane = plane
            .map(|(anchor, normal, offset, strength)| {
                Ok::<_, anyhow::Error>(CollisionPlane {
                    anchor: bone(anchor)?,
                    normal,
                    offset,
                    strength,
                })
            })
            .transpose()?;
        chain.validate(names.len())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
