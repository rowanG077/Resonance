use super::*;
use crate::battle::effect_program::{MagicArchive, magic_member};
use std::collections::BTreeMap;

pub(super) fn from_archive(
    archive: &MagicArchive,
    trails: &mut trails::Cooker,
    output: &Path,
    kind: resonance_content::battle::unison::PowWeapon,
) -> Result<WeaponVisuals> {
    let package = magic_member(archive.package(kind.native() - 200)?, 260)?
        .context("missing Pow carried resource")?;
    let ranges = sections(package)?;
    ensure!(
        ranges.len() == 5 && ranges[2..].iter().all(Option::is_none),
        "unsupported Pow carried layers"
    );
    let at = |index: usize| -> Result<&[u8]> {
        Ok(&package[ranges
            .get(index)
            .and_then(Option::as_ref)
            .context("missing Pow carried member")?
            .clone()])
    };
    let rig = rig(at(1)?, &[], RigKind::Weapon)?;
    let bones = attachments::trail_bones(&rig)?;
    ensure!(bones.len() == 2, "invalid Pow ribbon endpoints");
    let trail = TrailVisual {
        bones,
        style: trails.cook(trails::Recipe::PartyWeapon(at(0)?), None)?,
    };
    let mut parts = Vec::new();
    crate::model_preview::layers(
        crate::model_preview::Layer {
            model: at(1)?,
            outline: None,
            animation: None,
            attached_to: None,
            additive: false,
        },
        &mut parts,
        &format!("battle/weapons/pow-{}/0", kind.native()),
        output,
    )?;
    WeaponVisuals {
        source_sha256: digest(package),
        slots: BTreeMap::from([(
            0,
            ModelPreview {
                scale: 1.,
                elevation: 0.,
                parts,
                hidden_geometry: Vec::new(),
                node_scales: Vec::new(),
            },
        )]),
        rigs: BTreeMap::from([(0, rig)]),
        trails: BTreeMap::from([(0, trail)]),
        motion_link: None,
    }
    .with_instances(
        if kind == resonance_content::battle::unison::PowWeapon::Blade {
            &[0, 0]
        } else {
            &[0]
        },
    )
}

/// The native replaces only slot zero; the equipped second package stays bound.
pub(super) fn preserve_secondary(
    original: &WeaponVisuals,
    pow: &WeaponVisuals,
) -> Result<WeaponVisuals> {
    ensure!(
        original.slots.keys().copied().eq([0, 1])
            && pow.slots.keys().copied().eq([0])
            && original.motion_link.is_none()
            && pow.motion_link.is_none(),
        "invalid Pow Devastation carried layout"
    );
    let mut weapon = original.clone();
    weapon.source_sha256 =
        digest(format!("{}:{}", original.source_sha256, pow.source_sha256).as_bytes());
    weapon.slots.insert(0, pow.slots[&0].clone());
    weapon.rigs.insert(0, pow.rigs[&0].clone());
    weapon.trails.insert(0, pow.trails[&0].clone());
    Ok(weapon)
}

#[test]
fn replacing_preseas_primary_keeps_the_equipped_secondary_package() {
    use resonance_content::battle::{
        effect_program::Blend,
        pose::{Bone, Skeleton},
    };
    let package = |name: &str| WeaponVisuals {
        source_sha256: digest(name.as_bytes()),
        slots: BTreeMap::from([(
            0,
            ModelPreview {
                scale: 1.,
                elevation: 0.,
                parts: vec![],
                hidden_geometry: vec![name.into()],
                node_scales: vec![],
            },
        )]),
        rigs: BTreeMap::from([(
            0,
            Rig {
                skeleton: Skeleton {
                    bones: vec![Bone {
                        bind_channels: Default::default(),
                        name: name.into(),
                        parent: None,
                        bind: Default::default(),
                    }],
                },
                motions: Default::default(),
                attack_groups: Default::default(),
                effect_groups: Default::default(),
                weapon_bones: Default::default(),
            },
        )]),
        trails: BTreeMap::from([(
            0,
            TrailVisual {
                bones: vec![0, 0],
                style: resonance_content::battle::visual::TrailStyle {
                    textures: BTreeMap::from([(0, None)]),
                    palette: 0,
                    rgb: [1, 2, 3],
                    uv: [0; 4],
                    blend: Blend::Alpha,
                },
            },
        )]),
        motion_link: None,
    };
    let mut equipped = package("primary");
    let secondary = package("secondary");
    equipped.slots.insert(1, secondary.slots[&0].clone());
    equipped.rigs.insert(1, secondary.rigs[&0].clone());
    equipped.trails.insert(1, secondary.trails[&0].clone());
    let replacement = package("temporary");
    let result = preserve_secondary(&equipped, &replacement).unwrap();
    assert_eq!(result.rigs[&0].skeleton.bones[0].name, "temporary");
    assert_eq!(result.rigs[&1].skeleton.bones[0].name, "secondary");
    for slot in [0, 1] {
        let (source, key) = if slot == 0 {
            (&replacement, 0)
        } else {
            (&equipped, 1)
        };
        assert_eq!(
            serde_json::to_value((
                &result.slots[&slot],
                &result.rigs[&slot],
                &result.trails[&slot]
            ))
            .unwrap(),
            serde_json::to_value((
                &source.slots[&key],
                &source.rigs[&key],
                &source.trails[&key]
            ))
            .unwrap()
        );
    }
    assert_eq!(equipped.rigs[&0].skeleton.bones[0].name, "primary");
}
