use super::*;
use resonance_content::battle::pose::{Bone, Skeleton, Transform};

#[test]
#[ignore = "requires both original discs; parses authored rigs and clips without cooking"]
fn original_package_models_decode_all_266_authored_rigs_without_conversion() -> Result<()> {
    use crate::battle::{actions::Rel, all::physical_ranges, pose};
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for disc in 1..=2 {
        let files = project.join(format!("local/extracted/disc{disc}/files"));
        let rel = Rel::read(&files.join("US_r_Top2Btl.rel"))?;
        let mut counts = [0; 3];
        let mut mismatches = Vec::new();
        for (kind, table, rows) in [
            ("magic", 0xfe0, 121),
            ("skill", 0x11c4, crate::battle::all::SKILL_ARCHIVE_ROWS),
        ] {
            let archive = std::fs::read(files.join(format!("BTL/BTL{kind}.dat")))?;
            let offsets = (0..rows)
                .map(|index| word(rel.at((5, table))?, index * 4))
                .collect::<Result<Vec<_>>>()?;
            for (package, range) in physical_ranges(&offsets, 1, archive.len() as u64)? {
                let bytes = &archive[range];
                for index in 0..10 {
                    let Some(model) = magic_member(bytes, 12 + usize::from(index) * 4)? else {
                        continue;
                    };
                    (|| -> Result<()> {
                        let clips = model_clips(bytes, index)?;
                        let rig = authored_rig(model, &clips)?;
                        assert_eq!(rig.motions.len(), clips.len());
                        assert!(!rig.skeleton.bones.is_empty());
                        counts[0] += 1;
                        counts[1] += usize::from(!clips.is_empty());
                        if let Some(outline) = magic_member(bytes, 52 + usize::from(index) * 4)? {
                            let outline = pose::skeleton(outline)?;
                            assert!(!outline.bones.is_empty());
                            counts[2] += 1;
                            let differences: Vec<_> = rig
                                .skeleton
                                .bones
                                .iter()
                                .zip(&outline.bones)
                                .enumerate()
                                .filter_map(|(joint, (a, b))| (a.bind != b.bind).then_some(joint))
                                .collect();
                            if !differences.is_empty() {
                                mismatches.push((kind, package, index, differences));
                            }
                        }
                        Ok(())
                    })()
                    .with_context(|| format!("disc {disc} {kind} model {package}/{index}"))?;
                }
            }
        }
        assert_eq!(
            counts,
            [266, 18, 17],
            "disc {disc}: models, animated models, outline pairs"
        );
        assert_eq!(mismatches, [("magic", 86, 0, vec![17, 62])]);
    }
    Ok(())
}

#[test]
fn outline_joint_mapping_requires_matching_topology_and_bind_transforms() {
    let primary = Skeleton {
        bones: vec![
            Bone {
                bind_channels: Default::default(),
                name: "root".into(),
                parent: None,
                bind: Transform::default(),
            },
            Bone {
                bind_channels: Default::default(),
                name: "joint".into(),
                parent: Some(0),
                bind: Transform::default(),
            },
        ],
    };
    let mut outline = primary.clone();
    outline.bones[1].name = "JOINT".into();
    assert_eq!(
        pose_joints(&primary, Some(&outline)).unwrap(),
        [vec![0, 1], vec![0, 1]]
    );
    outline.bones[1].parent = None;
    assert!(pose_joints(&primary, Some(&outline)).is_err());
    outline.bones[1].parent = Some(0);
    outline.bones[1].bind.translation[0] = 1.;
    assert!(pose_joints(&primary, Some(&outline)).is_err());
    outline.bones.pop();
    assert!(pose_joints(&primary, Some(&outline)).is_err());
}

#[test]
#[ignore = "requires original extracted disc; audits every currently external effect model without cooking"]
fn original_external_effect_outlines_copy_pose_indices_with_matching_local_hierarchies() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = std::fs::read(extracted.join("sys/main.dol")).unwrap();
    let draw = crate::dol::slice(&dol, 0x8012abfc, 0x214).unwrap();
    assert_eq!(
        crate::digest(draw),
        "1aadd343c09127ba34a6901d888e889c97c693df6db8980e911288655a241981"
    );
    // The two model node tables are loaded with the same scaled ordinal register.
    assert_eq!(word(draw, 0x7c).unwrap(), 0x7fe4002e);
    assert_eq!(word(draw, 0x80).unwrap(), 0x7f03002e);
    let archive = MagicArchive::read(&extracted).unwrap();
    let mut differences = Vec::new();
    let mut checked = 0;
    for (package, models, count, primary_hash, outline_hash) in [
        (
            37,
            0..4,
            54,
            "35c63f5e06389b804a54f3a12829042e6d5d7c7773b1a4ad7d10f412a86be31c",
            "51ff94556782a14e33ad5fe9339574668f823ae8acc5fdef539ec19e9cd9c6bf",
        ),
        (
            90,
            0..1,
            74,
            "3df1de9bf7ebf7e80b6c54b79c07e0c92fdc9865bcca0824bad30ce985df9ab2",
            "d1115e941fb56e17ebd288c20c0efed9d23541af8f17e3b9541a5ad2010e94a3",
        ),
        (
            92,
            0..1,
            63,
            "9fc39097fe1c6bbca6b20fff9050008785c3e293cff3647516e9fc932217de31",
            "5804c12e8322ff8fa37429e2b129ea9b48030b29b13ed8b1159f8af708001704",
        ),
        (
            86,
            0..1,
            70,
            "3aa6c7ee4877fe8ee0cb103b8eef0544cf3969c2110bf30b99f5178d770edc5e",
            "daee4690b55dd446abbafdced470f8067b0677dd6cabef391ce0924e4ce03abd",
        ),
        (
            86,
            1..2,
            64,
            "8e15e29d088955ce0b6298230ad4cb28f8db7247951f8f20bd2c516cee1b0a0a",
            "b3efa517545d6afc02c1eb87d3fdc854ae1c72868fd5b7817008b7907e18a9b6",
        ),
        (
            86,
            2..3,
            70,
            "cebba1a3ef5550f41ddb6fefd68ec0d91c2a4e30c2d79ccbc0ba00d8b05840d6",
            "c475800d34ebf9eb1d6f2d59c89dd4cc3c41b8d8ce70a2da0f22345c2034b021",
        ),
        (
            88,
            0..1,
            55,
            "83f86a289ef54e5d68cf0080fefddaca3fa39a1b7904fac8f79250e15a54adc2",
            "ff8f53665bcb9c204f0e61393230e0ea4b38c6ff3b8f9d4957f3efca6317603f",
        ),
    ] {
        let bytes = archive.package(package).unwrap();
        for index in models {
            let primary = magic_member(bytes, 12 + index * 4).unwrap().unwrap();
            let outline = magic_member(bytes, 52 + index * 4).unwrap().unwrap();
            assert_eq!(crate::digest(primary), primary_hash);
            assert_eq!(crate::digest(outline), outline_hash);
            let primary_resource = primary;
            let normalized = normalize_outline(primary_resource, outline).unwrap();
            let primary = crate::battle::pose::skeleton(primary).unwrap();
            let authored = crate::battle::pose::skeleton(outline).unwrap();
            let cooked = crate::battle::pose::skeleton(&normalized).unwrap();
            if (package, index) == (86, 0) {
                assert!(pose_joints(&primary, Some(&authored)).is_err());
                let changed: Vec<_> = primary
                    .bones
                    .iter()
                    .zip(&authored.bones)
                    .enumerate()
                    .filter_map(|(i, (a, b))| (a.bind != b.bind).then_some(i))
                    .collect();
                assert_eq!(changed, [17, 62]);
                let bytes = crate::battle::pose::model(outline).unwrap();
                let model = crate::model::Model::parse(bytes).unwrap();
                let offset = crate::battle::pose::model_range(outline).unwrap().start;
                let transforms: Vec<_> = changed
                    .iter()
                    .map(|&joint| {
                        let start = offset + model.nodes[joint].data_offset as usize;
                        start..start + 44
                    })
                    .collect();
                for (i, (&before, &after)) in outline.iter().zip(normalized.iter()).enumerate() {
                    if !transforms.iter().any(|range| range.contains(&i)) {
                        assert_eq!(
                            before, after,
                            "outline geometry or metadata changed at {i:#x}"
                        );
                    }
                }
                // A valid but different hierarchy must still fail, even with identical binds.
                let mut detached = normalized.to_vec();
                let at = offset
                    + model.nodes[usize::from(authored.bones[17].parent.unwrap())].source_offset
                    + 16;
                detached[at..at + 4].fill(0);
                assert!(normalize_outline(primary_resource, &detached).is_err());
            } else {
                assert_eq!(
                    normalized.as_ref(),
                    outline,
                    "unchanged {package}/{index} outline"
                );
            }
            assert_eq!(
                primary.bind_pose().unwrap().global,
                cooked.bind_pose().unwrap().global,
                "outline inverse binds must derive from the primary global binds"
            );
            for (a, b) in authored.bones.iter().zip(&cooked.bones) {
                assert_eq!(a.name, b.name);
                assert_eq!(a.parent, b.parent);
            }
            let outline = cooked;
            assert_eq!(primary.bones.len(), count);
            let expected: Vec<_> = (0..count as u16).collect();
            assert_eq!(
                pose_joints(&primary, Some(&outline)).unwrap(),
                [expected.clone(), expected]
            );
            for (joint, (a, b)) in primary.bones.iter().zip(&outline.bones).enumerate() {
                if a.name != b.name {
                    differences.push((package, index, joint, a.name.clone(), b.name.clone()));
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 10);
    assert_eq!(
        differences,
        [(92, 0, 49, "kk00_Tue".into(), "KK00_Tue".into())]
    );
}
