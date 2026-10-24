use super::tests::{equivalent, no_inventory};
use super::*;
use resonance_content::{
    battle_model::{self, document as models},
    diagnostics::Diagnostics,
    library::{Library, Mod},
};

#[test]
fn equipment_names_preserve_known_items_and_unidentified_battle_props() -> Result<()> {
    assert_eq!(crate::naming::equipment(135)?, "wooden-blade-0135");
    assert_eq!(crate::naming::equipment(532)?, "equipment-0532");
    let names: Mappings = serde_json::from_str(include_str!("../../naming/equipment.json"))?;
    let mut unique = BTreeSet::new();
    for (id, name) in names.iter() {
        assert!(unique.insert(name));
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        );
        assert_eq!(crate::naming::equipment(u16::try_from(id)?)?, name);
    }
    assert_eq!(unique.len(), 179);
    Ok(())
}

#[test]
#[ignore = "requires original disc 1; compares all equipment definitions and model resources"]
fn original_equipment_keeps_models_nonvisual_choices_and_independent_owners() -> Result<()> {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let raw = tempfile::tempdir()?;
    let source = raw.path().to_path_buf();
    let original = crate::battle_model::weapon::prepare(&extracted, &source)?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(&source, output.path())?;
    publisher.battle_equipment(original.clone())?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let diagnostics = Diagnostics::new(true);
    let mut digests = BTreeMap::new();
    let mut kinds = [0; 3];
    let mut clips = 0;
    for (id, mut old) in original.records {
        let named = models::capture_equipment(&mut snapshot, id, &diagnostics, &|| false)?;
        match &mut old {
            battle_model::Attachment::Weapon(weapon) | battle_model::Attachment::Shield(weapon) => {
                weapon.source_sha256.clear();
            }
            battle_model::Attachment::Nonvisual => {}
        }
        equivalent(&old, &named, &source, output.path(), &mut digests)?;
        let weapon = match named {
            battle_model::Attachment::Weapon(weapon) => {
                kinds[0] += 1;
                weapon
            }
            battle_model::Attachment::Shield(weapon) => {
                kinds[1] += 1;
                weapon
            }
            battle_model::Attachment::Nonvisual => {
                kinds[2] += 1;
                continue;
            }
        };

        for part in weapon.parts.values() {
            for layer in &part.layers {
                let skeleton = part
                    .layer_skeletons
                    .get(&layer.scene.resource)
                    .unwrap_or(&part.rig.skeleton);
                assert!(
                    skeleton
                        .bones
                        .iter()
                        .map(|bone| &bone.name)
                        .eq(layer.scene.bone_names.iter())
                );
                let mesh = resonance_content::animation::Skeleton::from_glb(
                    &snapshot.read(&layer.scene.mesh, 128 * 1024 * 1024)?,
                )?;
                assert!(
                    mesh.bones
                        .iter()
                        .map(|bone| &bone.name)
                        .eq(layer.scene.bone_names.iter())
                );
                for clip in &layer.scene.clips {
                    let motion = resonance_content::animation::Motion::decode(
                        &snapshot.read(&clip.motion, 128 * 1024 * 1024)?,
                    )?;
                    motion.validate(skeleton)?;
                    clips += 1;
                }
            }
        }
    }
    assert_eq!(kinds, [143, 11, 25]);
    assert_eq!(clips, 210);
    for path in publisher
        .published
        .keys()
        .filter(|path| path.ends_with(".json"))
    {
        no_inventory(
            &serde_json::from_slice(&fs::read(output.path().join(path))?)?,
            path,
        );
    }
    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        fs::create_dir_all(modification.path().join(path).parent().unwrap())?;
        Ok(fs::write(modification.path().join(path), bytes)?)
    };
    let old_path = snapshot.resolve(models::EQUIPMENT, 135, Some("equipment.json"))?;
    write(models::EQUIPMENT, br#"{"135":"training-blades"}"#)?;
    let path = "meshes/equipment/training-blades/equipment.json";
    write(path, &snapshot.read(&old_path, 1024 * 1024)?)?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "equipment edit".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut edited_snapshot = modified.snapshot();
    assert!(matches!(
        models::capture_equipment(&mut edited_snapshot, 135, &diagnostics, &|| false)?,
        battle_model::Attachment::Weapon(_)
    ));
    let mut definition: models::Equipment = edited_snapshot.json(path, 1024 * 1024)?;
    let models::AttachmentDefinition::Weapon(weapon) = &mut definition.attachment else {
        anyhow::bail!("expected wooden blade");
    };
    *weapon
        .parts
        .values_mut()
        .next()
        .context("missing wooden blade part")? =
        "meshes/equipment/training-blades/missing.json".into();
    write(path, &serde_json::to_vec(&definition)?)?;
    let error = models::capture_equipment(&mut modified.snapshot(), 135, &diagnostics, &|| false)
        .unwrap_err();
    assert!(format!("{error:#}").contains("missing.json"));
    assert!(models::capture_equipment(&mut edited_snapshot, 135, &diagnostics, &|| false).is_ok());
    assert!(
        models::capture_equipment(&mut modified.snapshot(), 135, &diagnostics, &|| true).is_err()
    );
    eprintln!(
        "verified 179 equipment entries, 210 clips, {} named publications",
        publisher.published.len()
    );
    Ok(())
}
