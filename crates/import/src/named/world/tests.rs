use super::super::tests::{canonicalize, compare_audio_selection, first_difference};
use super::*;
use resonance_content::library::{Library, Mod};

#[test]
fn native_actor_roles_have_stable_distinct_names() {
    let names: BTreeSet<_> = (1..=9)
        .chain([99, 100, 101])
        .chain(200..=216)
        .map(actor_name)
        .collect();
    assert_eq!(names.len(), 29);
    assert_eq!(actor_name(202), "noishe");
    assert_eq!(actor_name(204), "rheaird-rider-lloyd");
    assert_eq!(actor_name(216), "world-portal");
    assert_eq!(actor_name(999), "actor-0999");
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS and both extracted discs; compares all world resources and editable overrides"]
fn original_world_roundtrips_every_variant_and_accepts_owned_edits() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to a complete cook")?;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(&source, output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let mut original_snapshot = Library::open(&source, &[])?.snapshot();
    let mut original = world::capture(&mut original_snapshot, &|| false)?.1;
    publisher.world(original.clone())?;
    let field = document::capture_scene(&mut original_snapshot, 332, || false)?.1;
    let art = FieldArt::capture(&mut original_snapshot, &field)?;
    publisher.field("iselia-village", field, art, Default::default())?;
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let (definition, restored) = world::capture(&mut snapshot, &|| false)?;
    assert_eq!(
        restored.worlds.iter().map(Vec::len).collect::<Vec<_>>(),
        [108, 108]
    );
    assert_eq!(
        restored
            .worlds
            .iter()
            .flatten()
            .filter(|t| t.alternate.is_some())
            .count(),
        12
    );
    assert_eq!(restored.visuals.cinematics.len(), 14);
    original.script.sha256.clear();
    let mut before = serde_json::to_value(&original)?;
    let mut after = serde_json::to_value(&restored)?;
    let mut digests = BTreeMap::new();
    canonicalize(&mut before, &source, output.path(), &mut digests)?;
    canonicalize(&mut after, &source, output.path(), &mut digests)?;
    if let Some(difference) = first_difference(&before, &after, "") {
        anyhow::bail!("named world differs from the original: {difference}");
    }
    let (audio, embedded) = publisher.audio_source()?.original.world()?;
    let mut checked = BTreeSet::new();
    compare_audio_selection(
        &mut snapshot,
        publisher.audio_source()?,
        audio,
        &embedded.into_keys().collect(),
        &definition.audio,
        &mut checked,
    )?;
    assert!(!output.path().join("assets").exists());
    assert!(!output.path().join("worlds").exists());

    let (_, field) = document::capture_scene(&mut snapshot, 332, || false)?;
    for actor in field
        .actors
        .iter()
        .filter(|actor| (1..=9).contains(&actor.resource))
    {
        let world = &restored.visuals.actors[&(actor.resource as u16)];
        assert_eq!(actor.parts.len(), world.len());
        for (field, world) in actor.parts.iter().zip(world) {
            assert_eq!(
                field.mesh, world.mesh,
                "one character body serves field and world"
            );
            assert_eq!(field.textures, world.textures);
            assert!(
                world
                    .clips
                    .iter()
                    .all(|clip| clip.motion.starts_with("world/actors/")),
                "world travel clips keep their native bank"
            );
        }
    }

    // Renaming a continent's entry point, its display text and a model reference
    // leaves compatibility unchanged. A mod owns only its changed files; all
    // unchanged terrain, clips and textures inherit from the base library.
    let baseline = world::gameplay(&mut snapshot, &|| false)?.unwrap();
    let modification = tempfile::tempdir()?;
    let write = |path: &str, bytes: &[u8]| -> Result<()> {
        let target = modification.path().join(path);
        fs::create_dir_all(target.parent().unwrap())?;
        Ok(fs::write(target, bytes)?)
    };
    write("world/mappings.json", br#"{"0":"renamed-continent"}"#)?;
    write(
        "world/renamed-continent/world.json",
        &snapshot.read("world/sylvarant/world.json", 16 * 1024 * 1024)?,
    )?;
    let mut landmarks = original.landmarks.clone();
    landmarks.worlds[0][0].name = "A renamed destination".into();
    write(&definition.landmarks, &serde_json::to_vec(&landmarks)?)?;
    let actor_path = snapshot.resolve(world::ACTORS, 1, Some("model.json"))?;
    let mut actor: world::Model = snapshot.json(&actor_path, 16 * 1024 * 1024)?;
    let original_mesh = actor.parts[0].mesh.clone();
    let mesh = snapshot.read(&original_mesh, 128 * 1024 * 1024)?;
    actor.parts[0].mesh = "world/actors/lloyd/edited-body.glb".into();
    write(&actor.parts[0].mesh, &mesh)?;
    write(&actor_path, &serde_json::to_vec(&actor)?)?;
    let modified = Library::open(
        output.path(),
        &[Mod {
            name: "world edits".into(),
            root: modification.path().to_owned(),
        }],
    )?;
    let mut edited_snapshot = modified.snapshot();
    assert_eq!(
        world::gameplay(&mut edited_snapshot, &|| false)?.unwrap(),
        baseline
    );
    let (_, edited) = world::capture(&mut edited_snapshot, &|| false)?;
    assert_eq!(edited.visuals.actors[&1][0].mesh, actor.parts[0].mesh);
    assert_eq!(edited.landmarks.worlds[0][0].name, "A renamed destination");
    assert_eq!(edited.worlds[1].len(), 108);
    assert!(
        edited_snapshot
            .overrides()
            .iter()
            .any(|o| o.path == "world/mappings.json" && o.id == Some(0))
    );

    // Current collision bytes participate in compatibility without an inventory
    // or hash update. An already admitted snapshot retains its earlier inputs.
    let continent: world::Continent =
        snapshot.json("world/sylvarant/world.json", 16 * 1024 * 1024)?;
    let terrain: world::Terrain = snapshot.json(&continent.terrain[0].base, 4096)?;
    let mut ground = restored.worlds[0][0].base.ground.clone();
    ground[0].surface = (ground[0].surface + 1) % 16;
    write(&terrain.collision, &serde_json::to_vec(&ground)?)?;
    assert_ne!(
        world::gameplay(&mut modified.snapshot(), &|| false)?.unwrap(),
        baseline
    );
    assert_eq!(
        world::gameplay(&mut edited_snapshot, &|| false)?.unwrap(),
        baseline
    );
    fs::remove_file(modification.path().join(&terrain.collision))?;
    let mut script = snapshot
        .read(&definition.script, 16 * 1024 * 1024)?
        .to_vec();
    *script.last_mut().context("empty world script")? ^= 1;
    write(&definition.script, &script)?;
    assert_ne!(
        world::gameplay(&mut modified.snapshot(), &|| false)?.unwrap(),
        baseline
    );
    fs::remove_file(modification.path().join(&definition.script))?;
    fs::remove_file(modification.path().join(&actor.parts[0].mesh))?;
    assert_eq!(
        &*edited_snapshot.read(&actor.parts[0].mesh, mesh.len())?,
        &*mesh
    );
    let error = world::capture(&mut modified.snapshot(), &|| false).unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains(&actor_path) && error.contains(&actor.parts[0].mesh),
        "{error}"
    );
    assert!(world::capture(&mut library.snapshot(), &|| true).is_err());
    assert!(world::gameplay(&mut library.snapshot(), &|| true).is_err());
    println!(
        "Compared 216 world tiles, 12 alternate tiles, 14 cinematics, {} audio programs and {} named publications",
        checked.len(),
        publisher.published.len()
    );
    Ok(())
}
