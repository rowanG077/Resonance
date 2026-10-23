use super::*;

fn model(names: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0; 64 + names.len() * 28];
    let put = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes())
    };
    put(&mut bytes, 4, 32);
    put(&mut bytes, 32, 0x007b7960);
    bytes[38..40].copy_from_slice(&(names.len() as u16).to_be_bytes());
    put(&mut bytes, 44, 32);
    put(&mut bytes, 56, 1);
    put(&mut bytes, 60, (32 + names.len() * 28) as u32);
    for (index, name) in names.iter().enumerate() {
        if index + 1 < names.len() {
            put(
                &mut bytes,
                64 + index * 28 + 8,
                (32 + (index + 1) * 28) as u32,
            );
        }
        bytes[64 + index * 28 + 24] = 1;
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    let size = bytes.len() - 32;
    put(&mut bytes, 8, size as u32);
    bytes
}

#[test]
fn rig_decodes_only_body_artwork_attachments() -> Result<()> {
    let source = model(&["root", "at", "at0/", "Kk00", "kk01"]);
    let body = rig(&source, RigKind::Body)?;
    assert_eq!(body.attachments[&0], 3);
    assert_eq!(body.attachments[&1], 4);
    for kind in [RigKind::Weapon, RigKind::Effect] {
        assert!(rig(&source, kind)?.attachments.is_empty());
    }
    assert!(rig(&model(&["kk"]), RigKind::Body).is_err());
    assert!(rig(&model(&["kk0/"]), RigKind::Body).is_err());
    assert!(rig(&source[..source.len() - 1], RigKind::Body).is_err());
    Ok(())
}

#[test]
fn attached_models_preserve_activation_placement_and_motion_selection() -> Result<()> {
    use resonance_content::battle_model::AttachmentPlacement::{Bone, Independent};
    let mut bytes = vec![0; 0x440];
    bytes[4..6].copy_from_slice(&0x200u16.to_be_bytes());
    bytes[12..14].copy_from_slice(&0x3f0u16.to_be_bytes());
    bytes[0x3f8] = 16;
    bytes[0x1d8..0x1dc].copy_from_slice(&0x400u32.to_be_bytes());
    bytes[0x284..0x288].copy_from_slice(&2f32.to_be_bytes());
    bytes[0x3e8] = 1;
    bytes[0x400..0x404].copy_from_slice(&[0, 60, 3, 0x70]);
    for offset in [0x41c, 0x420, 0x424] {
        bytes[offset..offset + 4].copy_from_slice(&1f32.to_be_bytes());
    }
    assert!(enemy_resources::attached_models(&bytes)?.is_empty());
    bytes[0x3e6] = 1;
    let binding = enemy_resources::attached_models(&bytes)?.remove(0);
    assert_eq!(
        (binding.model_slot, binding.motion_slot, binding.placement),
        (0, Some(60), Bone(3))
    );
    assert!(binding.visible && binding.additive && binding.wing_layers && !binding.inherit_tint);
    assert_eq!(binding.transform.scale, [2.; 3]);
    bytes[0x401] = 0;
    bytes[0x403] = 0;
    let binding = enemy_resources::attached_models(&bytes)?.remove(0);
    assert!(binding.motion_slot.is_none());
    assert!(binding.visible && binding.inherit_tint && !binding.additive && !binding.wing_layers);
    bytes[0x402] = 0;
    assert!(!enemy_resources::attached_models(&bytes)?[0].visible);
    bytes[0x403] = 0x80;
    let binding = enemy_resources::attached_models(&bytes)?.remove(0);
    assert_eq!(binding.placement, Independent);
    assert_eq!(
        binding.transform.rotation,
        resonance_content::animation::euler_rotation([-90., 0., 0.])
    );
    bytes[0x403] = 1;
    assert!(enemy_resources::attached_models(&bytes).is_err());
    bytes[0x403] = 0x70;
    bytes[0x3e6] = 2;
    assert!(enemy_resources::attached_models(&bytes).is_err());
    Ok(())
}

#[test]
#[ignore = "requires extracted disc 2 and prepared common effect artwork"]
fn angel_wing_meshes_bind_their_separate_battle_motions() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let extracted = local.join("extracted/disc2");
    let sources = crate::source_assets::Sources::read(&extracted)?;
    let usual = std::fs::read(extracted.join("files").join(&sources.usual))?;
    let mut shared = crate::battle_effect::SharedPalettes::read(&usual, &local.join("all-assets"))?;
    for (id, frames, tracks, slot, bone) in [
        (215, 27., 22, 60, 3),
        (235, 27., 22, 61, 3),
        (239, 25., 39, 50, 5),
        (241, 0., 0, 0, 0),
        (242, 0., 0, 0, 0),
    ] {
        let bytes = crate::source_assets::enemy_package(
            &extracted.join("files").join(&sources.enemy),
            &usual,
            id,
        )?;
        let bindings = enemy_resources::attached_models(&bytes)?;
        if slot == 0 {
            assert!(
                bindings.is_empty(),
                "enemy {id} must not activate retained wing assets"
            );
            continue;
        }
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            (
                bindings[0].model_slot,
                bindings[0].motion_slot,
                bindings[0].placement
            ),
            (
                0,
                Some(slot),
                resonance_content::battle_model::AttachmentPlacement::Bone(bone)
            )
        );
        let members = crate::model_preview::PointerMembers::new(&bytes, 0x18..0x1e8)?;
        let body = rig(members.model(0x18)?.unwrap(), RigKind::Body)?;
        let stage = tempfile::tempdir()?;
        enemy_resources::publish(&bytes, id as u8, &body.skeleton, &mut shared, stage.path())?;
        let bank: resonance_content::battle_effect::SourceBank =
            serde_json::from_slice(&std::fs::read(stage.path().join(
                resonance_content::battle_model::enemy_effects_path(id as u8),
            ))?)?;
        let art = bank.art.unwrap();
        let wings = &art.models[&0];
        let scene = &wings.layers[0].scene;
        let clip = scene
            .clips
            .iter()
            .find(|clip| clip.resource_slot == u16::from(slot))
            .context("missing attachment motion")?;
        let motion = resonance_content::animation::Motion::decode(&std::fs::read(
            stage.path().join(&clip.motion),
        )?)?;
        motion.validate(&wings.rig.skeleton)?;
        assert_eq!(motion.duration_frames, frames, "enemy {id}");
        assert_eq!(motion.tracks.len(), tracks, "enemy {id}");
        let first = resonance_content::animation::pose::sample(&wings.rig.skeleton, &motion, 0.)?;
        let last = resonance_content::animation::pose::sample(
            &wings.rig.skeleton,
            &motion,
            motion.duration_frames,
        )?;
        let difference = first
            .iter()
            .zip(&last)
            .map(|(a, b)| {
                a.matrix()
                    .iter()
                    .flatten()
                    .zip(b.matrix().iter().flatten())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max)
            })
            .fold(0f32, f32::max);
        if id != 239 {
            assert!(
                difference < 1e-5,
                "enemy {id} wing loop must join without a pose jump"
            );
        }

        assert!(
            wings
                .rig
                .skeleton
                .bones
                .iter()
                .all(|bone| bone.name.starts_with("hane"))
        );
        for side in ["_L", "_R"] {
            assert!(
                motion
                    .tracks
                    .iter()
                    .any(|track| wings.rig.skeleton.bones[usize::from(track.bone)]
                        .name
                        .ends_with(side)),
                "enemy {id} must animate both wings"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs and prepared common effect artwork"]
fn original_attached_models_cover_all_owners_and_selected_motion() -> Result<()> {
    use resonance_content::battle_model::AttachmentPlacement;
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let mut declarations = Vec::new();
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        let sources = crate::source_assets::Sources::read(&extracted)?;
        let usual = std::fs::read(extracted.join("files").join(&sources.usual))?;
        let mut shared =
            crate::battle_effect::SharedPalettes::read(&usual, &local.join("all-assets"))?;
        let stage = tempfile::tempdir()?;
        let mut owners = Vec::new();
        for id in 0..resonance_content::monster::MONSTER_COUNT {
            let bytes = crate::source_assets::enemy_package(
                &extracted.join("files").join(&sources.enemy),
                &usual,
                id as u16,
            )?;
            let bindings = enemy_resources::attached_models(&bytes)?;
            if bindings.is_empty() {
                continue;
            }
            owners.push((id, bindings.len()));
            let members = crate::model_preview::PointerMembers::new(&bytes, 0x18..0x1e8)?;
            let body = rig(members.model(0x18)?.unwrap(), RigKind::Body)?;
            enemy_resources::publish(&bytes, id as u8, &body.skeleton, &mut shared, stage.path())?;
            let bank: resonance_content::battle_effect::SourceBank =
                serde_json::from_slice(&std::fs::read(stage.path().join(
                    resonance_content::battle_model::enemy_effects_path(id as u8),
                ))?)?;
            let art = bank.art.unwrap();
            for binding in &bindings {
                if let AttachmentPlacement::Bone(bone) = binding.placement {
                    assert!(usize::from(bone) < body.skeleton.bones.len());
                }
                let part = &art.models[&binding.model_slot];
                if let Some(slot) = binding.motion_slot {
                    let clip = part.layers[0]
                        .scene
                        .clips
                        .iter()
                        .find(|clip| clip.resource_slot == u16::from(slot))
                        .unwrap();
                    let motion = resonance_content::animation::Motion::decode(&std::fs::read(
                        stage.path().join(&clip.motion),
                    )?)?;
                    motion.validate(&part.rig.skeleton)?;
                    assert!(!motion.tracks.is_empty() && motion.duration_frames > 0.);
                }
            }
            declarations.push((disc, id, serde_json::to_value(&bindings)?));
        }
        assert_eq!(
            owners,
            [(76, 8), (200, 1), (215, 1), (233, 1), (235, 1), (239, 1)]
        );
    }
    for (a, b) in declarations[..6].iter().zip(&declarations[6..]) {
        assert_eq!((&a.1, &a.2), (&b.1, &b.2));
    }
    Ok(())
}
