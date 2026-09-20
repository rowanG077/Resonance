//! Split guardian recipes: persistent body shape, carried visibility and the common defeat tail.
use super::*;

const BINDINGS: [((usize, usize), (usize, usize)); 3] = [
    ((DATA, 0x553c), (1, 0x7f480)),
    ((DATA, 0x5da8), (1, 0x7f174)),
    ((DATA, 0x5dac), (1, 0x7f150)),
];
const NAMES: [(usize, &str); 5] = [
    (0x63e0, "kk06_Hane"),
    (0x63f0, "Bone21_BArm_L"),
    (0x6400, "Bone21_BArm_R"),
    (0x6410, "Bone11_FArm_L"),
    (0x6420, "Bone11_FArm_R"),
];

pub(super) fn read(rel: &Rel) -> Result<f32> {
    let entrance = (DATA, 0x54cc);
    ensure!(
        !rel.pointers.contains_key(&entrance) && word(rel.at(entrance)?, 0)? == 0,
        "unexpected guardian entrance callback"
    );
    for (address, target) in BINDINGS {
        ensure!(
            rel.pointer(address.0, address.1)? == target,
            "changed guardian dispatcher"
        );
    }
    // Recognize the initializer, terminal callback and dispatcher before decoding the recipe.
    let code = rel
        .at((1, 0x7f150))?
        .get(..0x368)
        .context("truncated guardian controller")?;
    ensure!(
        crate::digest(code) == "f9e0bfeb9912c1565b194a7e50f96ed9451fc93bcb13378bd51f6a4501586942",
        "changed guardian body or defeat controller"
    );
    for (offset, name) in NAMES {
        let bytes = rel.at((4, offset))?;
        ensure!(
            bytes.get(..name.len()) == Some(name.as_bytes()) && bytes.get(name.len()) == Some(&0),
            "changed guardian bone selector"
        );
    }
    let values = rel.at((4, 0x63c8))?;
    let wing_scale = float(values, 0)?;
    ensure!(
        (1..3).all(|axis| float(values, axis * 4).is_ok_and(|value| value == wing_scale))
            && (3..6).all(|axis| word(values, axis * 4).is_ok_and(|value| value == 0))
            && word(rel.at((4, 0x63ec))?, 0)? == 0,
        "unsupported guardian scale interval or arm scale"
    );
    ensure!(
        wing_scale.is_finite() && wing_scale > 0.,
        "invalid guardian wing scale"
    );
    Ok(wing_scale)
}

pub(super) fn policy(
    wing_scale: f32,
    metadata: &ActorSettings,
    lengths: &VoiceDurations,
) -> Result<EnemyNativePolicy> {
    ensure!(
        metadata.model.attachment_count == 8,
        "missing guardian carried parts"
    );
    let policy = EnemyNativePolicy::GuardianParts {
        wing_scale,
        defeat_ticks: defeat_only::duration(metadata, lengths)?,
    };
    policy.validate()?;
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn original_guardians_keep_all_variants_actions_parts_and_defeat_durations() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
        let durations = VoiceDurations::original(&usual).unwrap();
        let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
        let directory = word(&usual, 0x2c).unwrap() as usize;
        let mut owners = Vec::new();
        for id in 0..251 {
            let start = word(&usual, directory + id * 4).unwrap() as usize;
            let end = word(&usual, directory + (id + 1) * 4).unwrap() as usize;
            let bytes = compression::decode(&archive[start..end]).unwrap();
            let table = usize::from(half(&bytes, 12).unwrap());
            if bytes[table + 8] != 13 {
                continue;
            }
            owners.push(id);
            let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
            let settings = ActorSettings::read(metadata).unwrap();
            assert_eq!(metadata[0x1e7], 0);
            let lengths = member(&usual, 12).unwrap();
            let death = half(metadata, 0xf4).unwrap();
            let death = if death != 0 {
                death
            } else {
                word(metadata, 0x104).unwrap() as u16 + 6
            };
            assert_eq!(
                native_policy(13, &rel, &settings, &durations).unwrap(),
                EnemyNativePolicy::GuardianParts {
                    wing_scale: 0.5,
                    defeat_ticks: half(lengths, usize::from(death & 0x7fff) * 2).unwrap(),
                }
            );
            assert_eq!(bytes[table + 0x17], if id == 210 { 7 } else { 8 });
            let skeleton =
                crate::battle::pose::rig_skeleton(&bytes[word(&bytes, 0x18).unwrap() as usize..])
                    .unwrap();
            for (_, name) in NAMES {
                let bone = skeleton.bone(name).unwrap();
                assert_eq!(skeleton.bones[usize::from(bone)].bind.scale, [1.; 3]);
            }
            assert_eq!(half(lengths, usize::from(death & 0x7fff) * 2).unwrap(), 29);
            for (address, target) in BINDINGS {
                rel.pointers.remove(&address);
                assert!(native_policy(13, &rel, &settings, &durations).is_err());
                rel.pointers.insert(address, target);
            }
            // Changing a named part, scale duration or branch fails before rendering.
            for (section, offset) in [(4, 0x63e0), (4, 0x63ec), (1, 0x7f1c8)] {
                let at = rel.sections[section].0 + offset;
                rel.bytes[at] ^= 1;
                assert!(native_policy(13, &rel, &settings, &durations).is_err());
                rel.bytes[at] ^= 1;
            }
        }
        assert_eq!(owners, [208, 209, 210]);
        let table = member(&usual, 1).unwrap();
        let formations = table
            .chunks_exact(crate::battle::formations::RECORD_SIZE)
            .enumerate()
            .filter_map(|(id, row)| {
                (0..usize::from(row[5]))
                    .any(|slot| (208..=210).contains(&half(row, 8 + slot * 2).unwrap()))
                    .then_some((id, row))
            })
            .map(|(id, row)| {
                assert_eq!((row[4], row[5], row[0x10], row[0x20]), (1, 1, 0, 0));
                (id, half(row, 8).unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(formations, [(76, 209), (77, 208), (78, 210)]);
        assert!(
            EnemyNativePolicy::Unsupported { id: 13 }
                .validate()
                .is_err()
        );
    }
}
