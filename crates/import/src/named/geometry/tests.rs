use super::*;
use resonance_content::library::{Library, Mod};
use serde_json::json;

fn physical_scene() -> Result<Scene> {
    Ok(serde_json::from_value(json!({
        "mesh": "original/mesh.glb", "bone_names": ["root"],
        "textures": {"kind": "caller"},
        "draws": [{
            "source_index": 0, "mesh": 0, "model_node": 0,
            "draw_order": 0, "name": "object", "index_count": 3,
            "color": [1, 1, 1, 1], "preview_blend": false,
            "recipe": {"context": "default_gpl_ordinary", "source_mode": 4,
                "source_texture_count": 1, "stage_count": 1,
                "operations": [{"kind": "replace_texture", "texture": 0}]},
            "textures": [{"stage": 0, "table": "caller.tpl", "image": 0,
                "wrap": ["repeat", "clamp"], "min_filter": "linear",
                "mag_filter": "linear"}]
        }]
    }))?)
}

fn textures() -> Result<TextureCatalogue> {
    Ok(serde_json::from_value(json!({"textures": [null, {
        "dimensions": [1, 1], "format": "ci4",
        "sampler": {"wrap": ["repeat", "clamp"], "min_filter": "linear",
            "mag_filter": "linear", "lod": {"bias": 0, "min": 0, "max": 0, "edge": false}},
        "palette": {"format": "rgb5a3", "colors": [[0,0,0,255], [255,0,0,255]]},
        "images": ["original/texture.ktx2", "original/overlay.ktx2"]
    }]}))?)
}

#[test]
fn unbound_geometry_owns_files_and_captures_edited_nested_dependencies() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let (field, mut art) = super::super::tests::field_fixture(source.path(), 330)?;
    let original = physical_scene()?;
    art.unbound.insert(
        0xffee0000,
        vec![PhysicalScene {
            scene: original.clone(),
            textures: None,
        }],
    );
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.field_with_audio(
        "iselia-0330",
        field,
        art,
        BTreeSet::new(),
        super::super::tests::audio_fixture(),
    )?;
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut retained = library.snapshot();
    let (_, first) = document::capture_scene(&mut retained, 330, || false)?;
    let path = &first.unbound_geometry[0].scenes[0];
    let before: Scene = retained.json(path, 1024 * 1024)?;
    assert!(
        before
            .mesh
            .starts_with("fields/iselia-0330/models/unbound-ffee0000/")
    );
    assert_eq!(
        serde_json::to_value(&before.draws)?,
        serde_json::to_value(&original.draws)?
    );
    assert_eq!(before.bone_names, original.bone_names);
    assert_ne!(
        before.mesh, first.parts[0].mesh,
        "independent owners must have independent files"
    );
    let bytes = retained.read(&before.mesh, 1024)?;
    assert_eq!(&*bytes, b"mesh fixture");
    assert!(!output.path().join("original").exists());
    assert!(!output.path().join("assets").exists());

    // A mod introduces a local texture bank and changes the mesh. There is no
    // dependency inventory to update, and its unchanged image inherits the base.
    let modification = tempfile::tempdir()?;
    let mut edited = before.clone();
    edited.mesh = "meshes/replacement.glb".into();
    edited.textures = TextureSource::Local {
        catalogue: "meshes/replacement/textures.json".into(),
    };
    edited.draws[0].textures[0].image = 1;
    let mut bank = textures()?;
    bank.textures[1].as_mut().unwrap().images = vec![
        first.parts[0].textures[0].clone(),
        "meshes/replacement/added.png".into(),
    ];
    for (path, bytes) in [
        (path.as_str(), serde_json::to_vec(&edited)?),
        ("meshes/replacement.glb", b"new geometry".to_vec()),
        (
            "meshes/replacement/textures.json",
            serde_json::to_vec(&bank)?,
        ),
        ("meshes/replacement/added.png", b"new palette".to_vec()),
    ] {
        let target = modification.path().join(path);
        fs::create_dir_all(target.parent().unwrap())?;
        fs::write(target, bytes)?;
    }
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "geometry".into(),
            root: modification.path().to_path_buf(),
        }],
    )?;
    let mut snapshot = modified.snapshot();
    document::capture_scene(&mut snapshot, 330, || false)?;
    // Remove the replacement after capture: the admitted preparation retains it.
    fs::remove_file(modification.path().join("meshes/replacement/added.png"))?;
    assert_eq!(
        &*snapshot.read("meshes/replacement/added.png", 1024)?,
        b"new palette"
    );
    assert_eq!(&*snapshot.read(&edited.mesh, 1024)?, b"new geometry");
    assert_eq!(&*retained.read(&before.mesh, 1024)?, b"mesh fixture");
    assert_eq!(
        &*snapshot.read(&first.parts[0].mesh, 1024)?,
        b"mesh fixture"
    );
    let error = match document::capture_scene(&mut modified.snapshot(), 330, || false) {
        Ok(_) => anyhow::bail!("missing nested replacement was accepted"),
        Err(error) => format!("{error:#}"),
    };
    assert!(
        error.contains(path) && error.contains("meshes/replacement/added.png"),
        "{error}"
    );
    assert!(
        resonance_content::physical_scene::capture(&mut modified.snapshot(), path, &|| true)
            .is_err()
    );
    let mut disabled = library.snapshot();
    document::capture_scene(&mut disabled, 330, || false)?;
    assert!(matches!(
        disabled.json::<Scene>(path, 1024 * 1024)?.textures,
        TextureSource::Caller
    ));
    Ok(())
}

#[test]
fn physical_publication_preserves_local_palette_pages_and_material_recipes() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    super::super::tests::field_fixture(source.path(), 330)?;
    let mut original = physical_scene()?;
    original.textures = TextureSource::Local {
        catalogue: "original/textures.json".into(),
    };
    original.draws[0].textures[0].image = 1;
    let original_textures = textures()?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    let path = publisher.physical_scene(
        PhysicalScene {
            scene: original.clone(),
            textures: Some(original_textures.clone()),
        },
        "meshes/props/test",
    )?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = Library::open(output.path(), &[])?.snapshot();
    resonance_content::physical_scene::capture(&mut snapshot, &path, &|| false)?;
    let published: Scene = snapshot.json(&path, 1024 * 1024)?;
    let TextureSource::Local { catalogue } = &published.textures else {
        anyhow::bail!("lost local palette")
    };
    let mut bank: TextureCatalogue = snapshot.json(catalogue, 1024 * 1024)?;
    for (before, after) in original_textures.textures.iter().zip(&mut bank.textures) {
        let (Some(before), Some(after)) = (before, after) else {
            continue;
        };
        for (old, new) in before.images.iter().zip(&after.images) {
            crate::texture::compare_images(source.path(), old, output.path(), new)?;
        }
        after.images.clone_from(&before.images);
    }
    assert_eq!(
        serde_json::to_value(bank)?,
        serde_json::to_value(original_textures)?
    );
    assert_eq!(
        serde_json::to_value(published.draws)?,
        serde_json::to_value(original.draws)?
    );
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and both extracted discs; compares physical recipes in both affected fields"]
fn original_unbound_geometry_has_named_owners_and_complete_capture() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to a completed cook")?;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(&source, output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let names = crate::naming::fields([125, 499])?;
    let mut originals = Vec::new();
    for id in [125, 499] {
        let mut snapshot = Library::open(&source, &[])?.snapshot();
        let (_, original) = document::capture_scene(&mut snapshot, id, || false)?;
        let art = FieldArt::capture(&mut snapshot, &original)?;
        assert_eq!(original.unbound_geometry.len(), 1);
        publisher.field(names.target(id)?, original.clone(), art, BTreeSet::new())?;
        originals.push(original);
    }
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    for original in originals {
        let mut snapshot = library.snapshot();
        let (_, prepared) = document::capture_scene(&mut snapshot, original.map_id, || false)?;
        for (old, new) in original
            .unbound_geometry
            .iter()
            .zip(&prepared.unbound_geometry)
        {
            assert_eq!(old.resource, new.resource);
            assert_eq!(old.scenes.len(), new.scenes.len());
            for (old, new) in old.scenes.iter().zip(&new.scenes) {
                let before: Scene = serde_json::from_slice(&fs::read(source.join(old))?)?;
                let mut after: Scene = snapshot.json(new, 1024 * 1024)?;
                assert_eq!(
                    fs::read(source.join(&before.mesh))?,
                    &*snapshot.read(&after.mesh, 64 * 1024 * 1024)?
                );
                after.mesh = before.mesh.clone();
                assert_eq!(serde_json::to_value(after)?, serde_json::to_value(before)?);
            }
        }
        let files = resonance_content::prepared::Files::from_snapshot(
            snapshot,
            &mut Default::default(),
            || false,
            resonance_content::diagnostics::Diagnostics::new(true),
        )?;
        assert!(files.keys().all(|path| !path.starts_with("assets/")
            && !path.starts_with("geometry/")
            && !path.ends_with(".preload.json")));
        println!(
            "Named field {} preserves its physical geometry and captured {} files",
            original.map_id,
            files.len()
        );
    }
    Ok(())
}
