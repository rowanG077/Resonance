//! Resolve authored weapon endpoints for optional blade trails.
use anyhow::{Result, ensure};
use resonance_content::animation::Skeleton;

/// Weapons without endpoint markers have no trail.
pub fn weapon_endpoints(skeleton: &Skeleton) -> Result<Option<Vec<u16>>> {
    let mut endpoints = [None; 3];
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
        let endpoint = &mut endpoints[usize::from(name[3] - b'0')];
        ensure!(
            endpoint.is_none(),
            "duplicate weapon trail endpoint {}",
            bone.name
        );
        *endpoint = Some(u16::try_from(index)?);
        count += 1;
    }
    if count < 2 {
        return Ok(None);
    }
    ensure!(
        endpoints[..count].iter().all(Option::is_some),
        "noncontiguous weapon trail endpoints"
    );
    Ok(Some(
        endpoints[..count]
            .iter()
            .map(|bone| bone.unwrap())
            .collect(),
    ))
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
        for names in [&["KI00", "KI02"][..], &["KI00", "KI00"], &["KI00", "KI03"]] {
            assert!(weapon_endpoints(&skeleton(names)).is_err());
        }
        Ok(())
    }
}
