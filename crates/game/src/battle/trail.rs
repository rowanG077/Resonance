//! Resolve authored weapon endpoints for optional blade trails.
use anyhow::{Result, ensure};
use resonance_content::animation::Skeleton;

/// Weapons without endpoint markers have no trail.
pub fn weapon_endpoints(skeleton: &Skeleton) -> Result<Option<Vec<u16>>> {
    // Unassigned endpoints stay at the root; later markers can replace a slot.
    let mut endpoints = [0; 3];
    let mut count = 0;
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let name = bone.name.as_bytes();
        if name.len() < 2 || !name[..2].eq_ignore_ascii_case(b"KI") {
            continue;
        }
        ensure!(
            name.len() >= 4 && (b'0'..=b'2').contains(&name[3]),
            "invalid weapon trail endpoint {}",
            bone.name
        );
        ensure!(count < endpoints.len(), "too many weapon trail endpoints");
        endpoints[usize::from(name[3] - b'0')] = u16::try_from(index)?;
        count += 1;
    }
    if count < 2 {
        return Ok(None);
    }
    Ok(Some(endpoints[..count].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::animation::{Bone, Transform, TransformChannels};

    fn skeleton(names: &[&str]) -> Skeleton {
        Skeleton {
            bones: names
                .iter()
                .map(|&name| Bone {
                    name: name.into(),
                    parent: None,
                    bind_channels: TransformChannels(0),
                    bind: Transform::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn weapon_order_comes_from_endpoint_suffix_and_ignores_case() -> Result<()> {
        assert_eq!(
            weapon_endpoints(&skeleton(&["base", "AT00", "KI01", "ki00"]))?,
            Some(vec![3, 2])
        );
        assert_eq!(
            weapon_endpoints(&skeleton(&["KI02", "KI00", "KI01"]))?,
            Some(vec![1, 2, 0])
        );
        assert!(weapon_endpoints(&skeleton(&["base", "AT00"]))?.is_none());
        assert!(weapon_endpoints(&skeleton(&["KI00"]))?.is_none());
        assert_eq!(
            weapon_endpoints(&skeleton(&["ki00_Bone01", "ki00_bone02"]))?,
            Some(vec![1, 0])
        );
        assert_eq!(
            weapon_endpoints(&skeleton(&["KI00", "KI02"]))?,
            Some(vec![0, 0])
        );
        for names in [&["KI00", "KI03"][..], &["KI00", "KI01", "KI02", "KI00"]] {
            assert!(weapon_endpoints(&skeleton(names)).is_err());
        }
        Ok(())
    }
}
