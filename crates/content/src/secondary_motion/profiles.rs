//! Character behavior applied once when a model's authored chains are bound.
use super::{Chain, CollisionPlane, Definition};
use anyhow::{Context, Result};

enum Profile {
    Generic,
    Lloyd,
    Colette,
    Raine,
    Sheena,
    Zelos,
    Presea,
    Genis,
    Kratos,
}

impl Profile {
    fn for_model(model: &str) -> Self {
        [
            ("col00", Self::Colette),
            ("llo00", Self::Lloyd),
            ("ref00", Self::Raine),
            ("shi00", Self::Sheena),
            ("zel00", Self::Zelos),
            ("pre00", Self::Presea),
            ("reg00", Self::Genis),
            ("kra00", Self::Kratos),
        ]
        .into_iter()
        .find_map(|(name, profile)| model.contains(name).then_some(profile))
        .unwrap_or(Self::Generic)
    }
}

fn uniform(chain: &mut Chain, attraction: f32, gravity: f32, damping: f32) {
    chain.attraction = attraction;
    for joint in &mut chain.joints {
        joint.gravity = gravity;
        joint.damping = damping;
    }
}

fn taper(chain: &mut Chain) {
    for index in 1..chain.joints.len() {
        chain.joints[index].gravity = (f64::from(chain.joints[index - 1].gravity) / 1.2) as f32;
        chain.joints[index].damping = (f64::from(chain.joints[index - 1].damping) / 1.2) as f32;
    }
}

fn hair(chain: &mut Chain) {
    uniform(chain, 0.116, 0.966, 0.633);
    chain.joints[0].gravity = (f64::from(chain.joints[0].gravity) * 1.66) as f32;
    chain.joints[0].damping = (f64::from(chain.joints[0].damping) * 1.66) as f32;
    taper(chain);
}

pub(super) fn prepare(definition: &Definition, names: &[String]) -> Result<Vec<Chain>> {
    let profile = Profile::for_model(&definition.model);
    let mut chains = definition.chains.clone();
    for chain in &mut chains {
        chain.validate(names.len())?;
        let root = usize::from(chain.joints[0].node);
        let name = &names[root];
        // Resolve prefixes through skeleton order, including duplicate names.
        let selected =
            |prefix: &str| names.iter().position(|n| n.starts_with(prefix)) == Some(root);
        let plane = match profile {
            Profile::Generic => continue,
            Profile::Lloyd if name.starts_with("AB_ROOT_NR_FP_01_kami") => {
                uniform(chain, 0.4165, -0.7333, 0.7666);
                None
            }
            Profile::Lloyd if name.contains("manto_") => {
                Some(("Bone_sebone02", [0., -1., 0.], 0., 1.))
            }
            Profile::Colette if name.contains("_kami01") || name.contains("_manto02_") => {
                chain.rotation_locks[1] = true;
                if name.contains("_kami01") {
                    chain.joints[0].gravity = (f64::from(chain.joints[0].gravity) * 1.66) as f32;
                    chain.joints[0].damping = (f64::from(chain.joints[0].damping) * 1.66) as f32;
                    taper(chain);
                }
                Some(("Bone_sebone02", [0., -1., 0.], 1., 0.2))
            }
            Profile::Colette if name.contains("_manto01_") => {
                chain.rotation_locks[1] = true;
                Some(("Bone_sebone02", [0., 1., 0.], 0., 1.))
            }
            Profile::Colette if name.contains("_kata_") => {
                chain.rotation_locks[0] = true;
                Some((
                    if name.contains("_kata_L_") {
                        "Bone_ude01_L"
                    } else {
                        "Bone_ude01_R"
                    },
                    [0., 1., 0.],
                    0.,
                    1.,
                ))
            }
            Profile::Raine => {
                let back = name.contains("_manto02_") || name.contains("_manto03_");
                let front = name.contains("_manto04_");
                if back {
                    uniform(chain, 0.008167, 1.6, 0.633);
                    taper(chain);
                } else if front {
                    uniform(chain, 0.04, 3.933, 0.666);
                }
                if name.contains("_manto01_") {
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
                }
            }
            Profile::Sheena
                if selected("AB_ROOT_FP_01_nuno02_L") || selected("AB_ROOT_FP_01_nuno02_R") =>
            {
                uniform(chain, 0.008167, 1.6, 0.633);
                taper(chain);
                chain.rotation_locks[1] = true;
                Some(("Bone_sebone01", [0., -1., 0.], 0., 1.))
            }
            Profile::Sheena if root == 0 => {
                // The fallback selector slots stay zero: neither is assigned a bone name.
                chain.rotation_locks = [true; 2];
                None
            }
            Profile::Zelos if selected("AB_ROOT_FP_01_kami03") => {
                hair(chain);
                Some(("Bone_sebone02", [0., -1., 0.], 0., 1.))
            }
            Profile::Zelos if selected("AB_ROOT_FP_01_koshi_") => {
                uniform(chain, 0.0665, 1.6, 0.633);
                taper(chain);
                chain.rotation_locks[1] = true;
                Some(("Bone_sebone01", [0., -1., 0.], 0., 1.))
            }
            Profile::Presea
                if selected("AB_ROOT_FP_01_osage_L") || selected("AB_ROOT_FP_01_osage_R") =>
            {
                uniform(chain, 0.04, 1.266, 0.366);
                chain.rotation_locks[1] = true;
                None
            }
            Profile::Genis if selected("AB_ROOT_FP_01_kami") => {
                hair(chain);
                Some(("Bone_kubi", [0., -1., 0.], -10., 1.))
            }
            Profile::Genis if selected("AB_ROOT_FP_NR_01_momi") => {
                uniform(chain, 0.158167, 2.566, 0.4);
                None
            }
            Profile::Kratos
                if selected("AB_ROOT_FP_01_manto02_L") || selected("AB_ROOT_FP_01_manto02_R") =>
            {
                uniform(chain, 0.0665, 1.6, 0.633);
                taper(chain);
                chain.rotation_locks[1] = true;
                None
            }
            _ => None,
        };
        chain.collision_plane = plane
            .map(|(anchor, normal, offset, strength)| {
                let anchor = names
                    .iter()
                    .position(|name| name.starts_with(anchor))
                    .with_context(|| {
                        format!("{} chain collision anchor {anchor}", definition.model)
                    })?;
                Ok::<_, anyhow::Error>(CollisionPlane {
                    anchor: anchor.try_into()?,
                    normal,
                    offset,
                    strength,
                })
            })
            .transpose()?;
        chain.validate(names.len())?;
    }
    Ok(chains)
}

#[cfg(test)]
mod tests {
    use super::super::Joint;
    use super::*;

    fn fixture(model: &str, root: &str, anchor: &str) -> (Definition, Vec<String>) {
        (
            Definition {
                model: model.into(),
                chains: vec![Chain {
                    joints: (1..4)
                        .map(|node| Joint {
                            node,
                            gravity: 0.5,
                            damping: 0.1,
                        })
                        .collect(),
                    attraction: 0.019,
                    preserve_rotation: true,
                    rotation_locks: [false; 2],
                    collision_plane: None,
                }],
            },
            ["Bone_Root", root, "joint", "tip", anchor]
                .map(str::to_owned)
                .to_vec(),
        )
    }

    #[test]
    fn recovered_policies_preserve_native_parameter_rounding_and_planes() -> Result<()> {
        let tapered_gravity = [0x3fcccccd, 0x3faaaaab, 0x3f8e38e4];
        let tapered_damping = [0x3f220c4a, 0x3f070a3e, 0x3ee11112];
        let hair_gravity = [0x3fcd4175, 0x3fab0be2, 0x3f8e89e7];
        let hair_damping = [0x3f867ff6, 0x3f602a9a, 0x3f3ace2b];
        for (model, root, attraction, gravity, damping, lock, anchor, offset) in [
            (
                "shi003",
                "AB_ROOT_FP_01_nuno02_L",
                0x3c05cee1,
                tapered_gravity,
                tapered_damping,
                true,
                "Bone_sebone01",
                0.,
            ),
            (
                "zel002",
                "AB_ROOT_FP_01_kami03",
                0x3ded9168,
                hair_gravity,
                hair_damping,
                false,
                "Bone_sebone02",
                0.,
            ),
            (
                "zel002",
                "AB_ROOT_FP_01_koshi_L",
                0x3d883127,
                tapered_gravity,
                tapered_damping,
                true,
                "Bone_sebone01",
                0.,
            ),
            (
                "pre004",
                "AB_ROOT_FP_01_osage_R",
                0x3d23d70a,
                [0x3fa20c4a; 3],
                [0x3ebb645a; 3],
                true,
                "",
                0.,
            ),
            (
                "reg001",
                "AB_ROOT_FP_01_kami",
                0x3ded9168,
                hair_gravity,
                hair_damping,
                false,
                "Bone_kubi",
                -10.,
            ),
            (
                "reg001",
                "AB_ROOT_FP_NR_01_momi",
                0x3e21f688,
                [0x40243958; 3],
                [0x3ecccccd; 3],
                false,
                "",
                0.,
            ),
            (
                "kra003",
                "AB_ROOT_FP_01_manto02_R",
                0x3d883127,
                tapered_gravity,
                tapered_damping,
                true,
                "",
                0.,
            ),
        ] {
            let (definition, names) = fixture(model, root, anchor);
            let before = serde_json::to_vec(&definition)?;
            let prepared = definition.prepare(&names)?;
            let chain = &prepared[0];
            assert_eq!(chain.attraction.to_bits(), attraction, "{model}/{root}");
            assert_eq!(
                chain
                    .joints
                    .iter()
                    .map(|j| j.gravity.to_bits())
                    .collect::<Vec<_>>(),
                gravity
            );
            assert_eq!(
                chain
                    .joints
                    .iter()
                    .map(|j| j.damping.to_bits())
                    .collect::<Vec<_>>(),
                damping
            );
            assert_eq!(chain.rotation_locks, [false, lock]);
            assert!(chain.preserve_rotation);
            match &chain.collision_plane {
                Some(plane) => {
                    assert_eq!(plane.anchor, 4);
                    assert_eq!(plane.normal, [0., -1., 0.]);
                    assert_eq!(plane.offset, offset);
                    assert_eq!(plane.strength, 1.);
                    assert!(!anchor.is_empty());
                }
                None => assert!(anchor.is_empty()),
            }
            assert_eq!(serde_json::to_vec(&definition)?, before);
            assert_eq!(
                serde_json::to_vec(&definition.prepare(&names)?)?,
                serde_json::to_vec(&prepared)?
            );
        }
        Ok(())
    }

    #[test]
    fn selectors_follow_first_prefix_match_and_require_only_active_anchors() -> Result<()> {
        for (model, root, anchor) in [
            ("shi00", "AB_ROOT_FP_01_nuno02_R", "Bone_sebone01"),
            ("pre00", "AB_ROOT_FP_01_osage_L", ""),
            ("kra00", "AB_ROOT_FP_01_manto02_L", ""),
        ] {
            let (definition, mut names) = fixture(model, root, anchor);
            assert!(definition.prepare(&names)?[0].rotation_locks[1]);
            names[0] = format!("{root}_earlier");
            names.pop();
            assert_eq!(
                serde_json::to_vec(&definition.prepare(&names)?)?,
                serde_json::to_vec(&definition.chains)?
            );
        }
        let (definition, mut names) = fixture("reg00", "AB_ROOT_FP_01_kami", "Bone_kubi");
        names.pop();
        assert!(definition.prepare(&names).is_err());
        names[1].insert_str(0, "unrelated_");
        assert!(definition.prepare(&names).is_ok());
        let (mut definition, names) = fixture("shi00", "AB_ROOT_FP_01_nuno01_L", "");
        assert_eq!(
            serde_json::to_vec(&definition.prepare(&names)?)?,
            serde_json::to_vec(&definition.chains)?
        );
        let mut zero_root = definition.clone();
        for joint in &mut zero_root.chains[0].joints {
            joint.node -= 1;
        }
        let mut zero_names = names[1..].to_vec();
        let prepared = zero_root.prepare(&zero_names)?;
        assert_eq!(prepared[0].rotation_locks, [true; 2]);
        assert_eq!(prepared[0].attraction, definition.chains[0].attraction);
        assert!(prepared[0].collision_plane.is_none());
        zero_names[0] = "AB_ROOT_FP_01_nuno02_L".into();
        *zero_names.last_mut().unwrap() = "Bone_sebone01".into();
        let prepared = zero_root.prepare(&zero_names)?;
        assert_eq!(prepared[0].rotation_locks, [false, true]);
        assert_eq!(prepared[0].attraction, 0.008167);
        assert!(prepared[0].collision_plane.is_some());
        definition.model = "col00_pre00".into();
        let (_, names) = fixture("", "AB_ROOT_FP_01_osage_L", "");
        assert_eq!(definition.prepare(&names)?[0].attraction, 0.019);
        Ok(())
    }

    #[test]
    fn preparation_preserves_authored_data_and_requires_only_used_anchors() -> Result<()> {
        let mut definition = Definition {
            model: "llo000.gpl".into(),
            chains: vec![Chain {
                joints: (0..3)
                    .map(|node| Joint {
                        node,
                        gravity: 1.2,
                        damping: 0.1,
                    })
                    .collect(),
                attraction: 0.019,
                preserve_rotation: true,
                rotation_locks: [false; 2],
                collision_plane: None,
            }],
        };
        let mut names = vec!["AB_ROOT_NR_FP_01_kami".into(), "joint".into(), "tip".into()];
        let hair = prepare(&definition, &names)?;
        assert_eq!(hair[0].attraction, 0.4165);
        assert_eq!(hair[0].joints[0].gravity, -0.7333);
        assert!(hair[0].collision_plane.is_none());
        names[0] = "AB_ROOT_FP_01_manto_L".into();
        assert!(prepare(&definition, &names).is_err());
        names.push("Bone_sebone02".into());
        assert_eq!(
            prepare(&definition, &names)?[0]
                .collision_plane
                .as_ref()
                .unwrap()
                .anchor,
            3
        );
        for (model, root) in [
            ("col002.gpl", "AB_ROOT_FP_kami01"),
            ("ref003.gpl", "AB_ROOT_FP_manto02_L_"),
        ] {
            definition.model = model.into();
            names[0] = root.into();
            names[3] = if model.starts_with("col") {
                "Bone_sebone02"
            } else {
                "Bone_sebone03"
            }
            .into();
            let before = serde_json::to_vec(&definition)?;
            let first = serde_json::to_vec(&prepare(&definition, &names)?)?;
            assert_eq!(first, serde_json::to_vec(&prepare(&definition, &names)?)?);
            assert_eq!(before, serde_json::to_vec(&definition)?);
            assert_ne!(first, serde_json::to_vec(&definition.chains)?);
        }
        Ok(())
    }
}
