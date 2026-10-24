use super::*;
use resonance_content::library::{Library, Mod};

pub(super) fn field_fixture(source: &Path, id: u32) -> Result<(FieldAssets, FieldArt)> {
    fs::create_dir_all(source.join("original"))?;
    fs::create_dir_all(source.join("fields"))?;
    for (name, bytes) in [
        ("script.ssb", &b"script"[..]),
        ("messages.json", &b"[\"Hello\"]"[..]),
        ("mesh.glb", &b"mesh fixture"[..]),
    ] {
        fs::write(source.join("original").join(name), bytes)?;
    }
    for (name, width, height, color) in [
        ("texture.ktx2", 1, 1, [13, 47, 221, 127]),
        ("overlay.ktx2", 8, 8, [211, 77, 31, 255]),
    ] {
        fs::write(
            source.join("original").join(name),
            resonance_asset_writer::ktx2::encode_rgba8(
                width,
                height,
                &color.repeat((width * height) as usize),
            )?,
        )?;
    }
    let overlay = serde_json::from_value(serde_json::json!({
        "textures": [{
            "images": [{"path": "original/overlay.ktx2", "width": 8, "height": 8}],
            "sampler": {"wrap": ["clamp", "clamp"], "min_filter": "linear", "mag_filter": "linear",
                "lod": {"bias": 0, "min": 0, "max": 0, "edge": false}}
        }]
    }))?;
    let sprite = resonance_content::effect::SpriteRecipe {
        texture: "original/texture.ktx2".to_owned(),
        uv: [0., 0., 1., 1.],
        additive: false,
        frames: Vec::new(),
        repeat: false,
    };
    let effects = resonance_content::effect::FieldEffects {
        version: resonance_content::effect::FIELD_EFFECTS_VERSION,
        emote_texture: sprite.texture.clone(),
        status_texture: sprite.texture.clone(),
        sprites: resonance_content::effect::sprite::ALL
            .into_iter()
            .map(|id| (id, sprite.clone()))
            .collect(),
        palette: vec![[255; 4]; resonance_content::effect::FIELD_PALETTE_COLORS],
        refraction: resonance_content::effect::RefractionRecipe {
            sprite: sprite.clone(),
            displacement: [1., 1.],
        },
        air_refraction: sprite,
        mouth_cycle: vec![0],
        rising_light_destination: [0.; 3],
    };
    let field = FieldAssets {
        version: field::FIELD_VERSION,
        map_id: id,
        script: "original/script.ssb".into(),
        messages: "original/messages.json".into(),
        parts: serde_json::from_value(serde_json::json!([{
            "resource": 0, "mesh": "original/mesh.glb", "textures": ["original/texture.ktx2"],
            "materials": [], "translation": [0, 0, 0], "clips": [], "autoplay": false,
            "texture_animations": [], "bone_names": []
        }]))?,
        panorama: None,
        water: None,
        environment: Default::default(),
        ground: vec![],
        regions: vec![],
        doors: vec![],
        camera_tracks: BTreeMap::new(),
        texture_animations: vec![],
        actors: vec![],
        unbound_geometry: vec![],
        resource_catalogue: None,
        contact_shadow: field::ContactShadow {
            texture: "original/texture.ktx2".into(),
            uv_size: [1., 1.],
            half_size: 1.,
            height_offset: 1.,
            alpha: 128,
            anchor_node: 0,
        },
        toon_ramp: "original/texture.ktx2".into(),
        effects: "effects/field.json".into(),
        blink: resonance_content::effect::BlinkCycle { frames: vec![0] },
        particles: BTreeMap::new(),
        overlays: [(24, "original/overlay.json".into())].into(),
        save_point_tutorial: vec![],
        save_point_unlock: vec![],
        save_point_no_gem: vec![],
    };
    field.validate()?;
    Ok((
        field,
        FieldArt {
            effects: std::sync::Arc::new(effects),
            overlays: [(24, overlay)].into(),
            unbound: BTreeMap::new(),
        },
    ))
}

pub(super) fn audio_fixture() -> resonance_content::field_audio::document::Definition {
    use resonance_content::field_audio::{MusicReverbs, document};
    document::Definition {
        version: document::VERSION,
        music: vec![],
        effects: vec![],
        speech: vec![],
        music_reverbs: MusicReverbs {
            presets: [[0., 0., 1., 0., 0.]; 2],
            selectors: vec![],
        },
        voice_gains: (0..128).map(|v| v as f32 / 127.).collect(),
        reverbs: Some([[0., 0., 1., 0., 0.]; 2]),
    }
}

fn read_field(snapshot: &mut library::Snapshot, id: u32) -> Result<FieldAssets> {
    Ok(document::capture_scene(snapshot, id, || false)?.1)
}

#[test]
fn fields_are_editable_without_preloads_or_source_hashes() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let (field, art) = field_fixture(source.path(), 330)?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.field_with_audio(
        "iselia-0330",
        field,
        art,
        BTreeSet::from([2, 7]),
        audio_fixture(),
    )?;
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut snapshot = library.snapshot();
    let path = snapshot.field(330)?;
    let definition: Definition = snapshot.json(&path, 4096)?;
    assert_eq!(definition.movies, BTreeSet::from([2, 7]));
    assert_eq!(
        snapshot.mappings(library::FIELDS)?.target(330)?,
        "iselia-0330"
    );
    let field = read_field(&mut snapshot, 330)?;
    assert_eq!(field.map_id, 330);
    assert_eq!(field.parts[0].mesh, "fields/iselia-0330/meshes/part-0.glb");
    let overlay: resonance_content::effect::OverlayArt =
        snapshot.json(&field.overlays[&24], 4096)?;
    let overlay_path = &overlay.textures[0].images[0].path;
    assert_eq!(
        overlay_path,
        "fields/iselia-0330/overlays/overlay-24/texture-0-palette-0.png"
    );
    assert!(
        snapshot.digest(overlay_path).is_ok(),
        "overlay dependency was not captured"
    );
    assert_eq!(&*snapshot.read(&field.messages, 4096)?, br#"["Hello"]"#);
    fs::write(
        output.path().join(&field.messages),
        br#"["Edited in a text editor"]"#,
    )?;
    fs::write(output.path().join(&field.script), b"edited script")?;
    let mut refreshed = library.snapshot();
    let field = read_field(&mut refreshed, 330)?;
    assert_eq!(
        &*refreshed.read(&field.messages, 4096)?,
        br#"["Edited in a text editor"]"#
    );
    assert_eq!(&*refreshed.read(&field.script, 4096)?, b"edited script");
    assert!(!output.path().join("shared.preload.json").exists());
    assert!(!fs::read_to_string(output.path().join(path))?.contains("sha256"));
    Ok(())
}

#[test]
fn field_local_copies_are_independent_and_single_file_mods_inherit_the_field() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    for (id, name) in [(330, "iselia-0330"), (331, "iselia-0331")] {
        let (field, art) = field_fixture(source.path(), id)?;
        publisher.field_with_audio(name, field, art, BTreeSet::new(), audio_fixture())?;
    }
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let first = output
        .path()
        .join("fields/iselia-0330/textures/part-0-texture-0.png");
    let second = output
        .path()
        .join("fields/iselia-0331/textures/part-0-texture-0.png");
    let original = fs::read(&second)?;
    fs::write(&first, b"edited texture")?;
    assert_eq!(fs::read(second)?, original);
    let overlay = tempfile::tempdir()?;
    fs::create_dir_all(overlay.path().join("fields/iselia-0330"))?;
    fs::write(
        overlay.path().join("fields/iselia-0330/collision.json"),
        br#"{"ground":[],"regions":[]}"#,
    )?;
    let mut effects: resonance_content::effect::FieldEffects =
        serde_json::from_slice(&fs::read(output.path().join("effects/field/effects.json"))?)?;
    effects.emote_texture = "effects/field/textures/new-emotes.png".into();
    fs::create_dir_all(overlay.path().join("effects/field/textures"))?;
    fs::write(
        overlay.path().join(&effects.emote_texture),
        b"new emote artwork",
    )?;
    fs::write(
        overlay.path().join("effects/field/effects.json"),
        serde_json::to_vec(&effects)?,
    )?;
    let library = Library::open(
        output.path(),
        &[Mod {
            name: "collision".into(),
            root: overlay.path().into(),
        }],
    )?;
    let mut snapshot = library.snapshot();
    let field = read_field(&mut snapshot, 330)?;
    assert_eq!(field.parts.len(), 1);
    assert!(snapshot.overrides().iter().any(|entry| entry.path.ends_with("collision.json") && entry.replacement == "collision"));
    assert!(snapshot.digest(&effects.emote_texture).is_ok());
    let files = resonance_content::prepared::Files::from_snapshot(
        snapshot,
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?;
    assert_eq!(&*files.read(&effects.emote_texture)?, b"new emote artwork");
    assert!(
        files
            .overrides()
            .iter()
            .any(|entry| entry.path == "effects/field/effects.json")
    );
    // Preparation must discover changed nested references, and a bad mod must
    // identify its descriptor instead of silently substituting base artwork.
    fs::remove_file(overlay.path().join(&effects.emote_texture))?;
    let error = read_field(&mut library.snapshot(), 330).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("effects/field/effects.json"), "{message}");
    assert!(message.contains("new-emotes.png"), "{message}");
    Ok(())
}

#[test]
fn named_publication_rejects_case_collisions_and_conflicting_shared_owners() -> Result<()> {
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    let mut publisher = Publisher::new(source.path(), output.path())?;
    publisher.write("meshes/lloyd/body.glb", b"first")?;
    publisher.write("meshes/lloyd/body.glb", b"first")?;
    assert!(
        publisher
            .write("meshes/lloyd/body.glb", b"different")
            .is_err()
    );
    assert!(
        publisher
            .write("meshes/Lloyd/outline.glb", b"first")
            .is_err()
    );
    assert!(publisher.write("meshes/lloyd/BODY.glb", b"first").is_err());
    assert_eq!(party_owner(5).as_deref(), Some("meshes/characters/sheena"));
    assert_eq!(party_owner(9).as_deref(), Some("meshes/characters/kratos"));
    assert_eq!(
        party_owner(resonance_content::appearance::costume_resource(1, 3)).as_deref(),
        Some("meshes/characters/lloyd/costume-3")
    );
    Ok(())
}

#[test]
#[ignore = "requires current converted field assets; reads originals without modifying the library"]
fn original_fields_roundtrip_through_named_files_and_shared_character_models() -> Result<()> {
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to current named content")?;
    let output = tempfile::tempdir()?;
    let ids = [330, 332, 340, 372];
    let names = crate::naming::fields(ids)?;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut publisher = Publisher::new(&source, output.path())?.with_audio_source(
        AudioSource::read(&extracted.join("disc1"), Some(&extracted.join("disc2")))?,
    );
    let mut originals = Vec::new();
    for id in ids {
        let mut snapshot = Library::open(&source, &[])?.snapshot();
        let (definition, field) = document::capture_scene(&mut snapshot, id, || false)?;
        let art = FieldArt::capture(&mut snapshot, &field)?;
        publisher.field(names.target(id)?, field.clone(), art, definition.movies)?;
        originals.push(field);
    }
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let mut snapshot = Library::open(output.path(), &[])?.snapshot();
    let mut digests = BTreeMap::new();
    let mut audio_checked = BTreeSet::new();
    for original in originals {
        let restored = read_field(&mut snapshot, original.map_id)?;
        let entry = snapshot.field(original.map_id)?;
        let definition: Definition = snapshot.json(&entry, 4096)?;
        compare_audio(
            &mut snapshot,
            publisher.audio_source()?,
            original.map_id,
            &definition.audio,
            &mut audio_checked,
        )?;
        let id = original.map_id;
        let mut before = serde_json::to_value(original)?;
        let mut after = serde_json::to_value(restored)?;
        canonicalize(&mut before, &source, output.path(), &mut digests)?;
        canonicalize(&mut after, &source, output.path(), &mut digests)?;
        if let Some(difference) = first_difference(&before, &after, "") {
            anyhow::bail!("field {id} differs after splitting named documents: {difference}");
        }
    }
    let first: Vec<Actor> = snapshot.json("fields/iselia-0330/actors.json", 16 * 1024 * 1024)?;
    let second: Vec<Actor> = snapshot.json("fields/iselia-0332/actors.json", 16 * 1024 * 1024)?;
    let model = |actors: &[Actor]| {
        actors
            .iter()
            .find(|actor| actor.resource == 1)
            .unwrap()
            .model
            .clone()
    };
    assert_eq!(model(&first), "meshes/characters/lloyd/model.json");
    assert_eq!(model(&first), model(&second));
    println!(
        "Round-tripped {} fields through {} named publications and {} audio programs",
        ids.len(),
        publisher.published.len(),
        audio_checked.len(),
    );
    Ok(())
}

#[test]
#[ignore = "requires both extracted discs; publishes every scripted-field audio scope directly"]
fn all_original_field_audio_keeps_ids_programs_and_pcm() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir()?;
    // The phase catalogue also declares native scenes without a VM program and
    // resources absent from both discs. Derive this test's complete field-audio
    // scope from original scripted archives, independently of cooked output.
    let mut declared = BTreeSet::new();
    for disc in [1, 2] {
        let extracted = local.join(format!("extracted/disc{disc}"));
        for phase in
            crate::field_catalogue::read(&fs::read(extracted.join("sys/main.dol"))?)?.records
        {
            if let Some(resource) = phase.resource
                && let Some(path) = crate::field_resources::find_path(
                    &extracted.join("files"),
                    &format!("MAP/{resource}"),
                )?
                && crate::field::MapArchive::open(&extracted.join("files").join(path))?
                    .optional_section(6)
                    .is_some()
            {
                declared.insert(u32::try_from(phase.id)?);
            }
        }
    }
    let ids = declared;
    let names = crate::naming::fields(ids.iter().copied())?;
    let mut publisher =
        Publisher::new(source.path(), output.path())?.with_audio_source(AudioSource::read(
            &local.join("extracted/disc1"),
            Some(&local.join("extracted/disc2")),
        )?);
    for (index, &id) in ids.iter().enumerate() {
        publisher.field_audio(id, &format!("fields/{}/audio.json", names.target(id)?))?;
        if index % 100 == 99 {
            println!("Published audio for {} / {} fields", index + 1, ids.len());
        }
    }
    publisher.finish_fields()?;
    publisher.descriptor("GQSEAF", 0)?;
    let library = Library::open(output.path(), &[])?;
    let mut checked = BTreeSet::new();
    for &id in &ids {
        compare_audio(
            &mut library.snapshot(),
            publisher.audio_source()?,
            id,
            &format!("fields/{}/audio.json", names.target(id)?),
            &mut checked,
        )?;
    }
    println!(
        "Validated {} field audio selections, {} distinct programs, {} named publications",
        ids.len(),
        checked.len(),
        publisher.published.len()
    );
    Ok(())
}

fn compare_audio(
    snapshot: &mut library::Snapshot,
    original_audio: &AudioSource,
    id: u32,
    audio_path: &str,
    checked: &mut BTreeSet<String>,
) -> Result<()> {
    let original = original_audio.field_selection(id)?;
    compare_audio_selection(
        snapshot,
        original_audio.field_source(id)?,
        original,
        &BTreeSet::new(),
        audio_path,
        checked,
    )
}

pub(super) fn compare_audio_selection(
    snapshot: &mut library::Snapshot,
    original_audio: &AudioSource,
    original: resonance_content::battle_audio::Selection,
    native_clocks: &BTreeSet<u32>,
    audio_path: &str,
    checked: &mut BTreeSet<String>,
) -> Result<()> {
    let audio: resonance_content::field_audio::document::Definition =
        snapshot.json(audio_path, 16 * 1024 * 1024)?;
    let resolved = audio.resolve(snapshot)?;
    ensure!(
        serde_json::to_value(&audio.music_reverbs)?
            == serde_json::to_value(&original_audio.original.music_reverbs()?)?
    );
    ensure!(audio.voice_gains == original_audio.original.voice_gains()?);
    for (music, before, after) in [
        (true, &original.music, &resolved.music),
        (false, &original.sounds, &resolved.effects),
    ] {
        ensure!(
            before.iter().map(|id| *id as i16).eq(after.keys().copied()),
            "audio IDs changed in {audio_path}"
        );
        for original_id in before {
            let id = i16::try_from(*original_id)?;
            let path = &after[&id];
            if !checked.insert(path.clone()) {
                continue;
            }
            let environment = audio.reverbs.context("missing native field environment")?;
            let before = if music {
                original_audio.prepare_music(*original_id, environment)?
            } else {
                original_audio.prepare_sound(*original_id, environment)?
            };
            if music {
                let battle = audio.reverbs.is_some_and(|r| {
                    audio.music_reverbs.presets[0] != audio.music_reverbs.presets[1]
                        && r[0] == audio.music_reverbs.presets[1]
                });
                compare_music_prefix(snapshot, path, before, battle)?;
                continue;
            }
            let program = snapshot
                .json::<resonance_audio::effect::Definition>(path, 16 * 1024 * 1024)?
                .into_program()?;
            ensure!(!serde_json::to_string(&program)?.contains("\"sha256\""));
            let after = program.prepare_in(
                audio.program_reverbs(music.then_some(id))?,
                &mut |path, limit| snapshot.read(path, limit),
                &mut Default::default(),
            )?;
            ensure!(
                serde_json::to_value(&before.resources().programs)?
                    == serde_json::to_value(&after.resources().programs)?
            );
            ensure!(serde_json::to_value(before.score())? == serde_json::to_value(after.score())?);
            ensure!(
                serde_json::to_value(before.tables())? == serde_json::to_value(after.tables())?
            );
            ensure!(before.reverbs() == after.reverbs());
            ensure!(
                before
                    .resources()
                    .samples
                    .keys()
                    .eq(after.resources().samples.keys())
            );
            for (id, before) in &before.resources().samples {
                let after = &after.resources().samples[id];
                ensure!(
                    (
                        before.key,
                        before.rate,
                        before.loop_start,
                        before.loop_length
                    ) == (after.key, after.rate, after.loop_start, after.loop_length)
                );
                ensure!(
                    before.pcm == after.pcm && before.loop_pcm == after.loop_pcm,
                    "changed audio PCM in {path} sample {id}"
                );
            }
        }
    }
    ensure!(original.voices.iter().eq(resolved.voices.keys()));
    for id in original.voices {
        let selected = &resolved.voices[&id];
        let bytes = snapshot.read(&selected.path, 129 * 1024 * 1024)?;
        let (encoded, playback) = original_audio.speech_source(id)?;
        let temporary = tempfile::NamedTempFile::new()?;
        let format = crate::media::library::stream_format(&encoded)
            .context("unsupported native speech encoding")?;
        let (channels, rate, frames) =
            crate::media::field_audio::decode_voice_wave(temporary.path(), &encoded, format)?;
        ensure!(&*bytes == fs::read(temporary.path())?);
        let wave = hound::WavReader::new(std::io::Cursor::new(&*bytes))?;
        ensure!(wave.duration() == frames && wave.spec().channels == channels);
        let expected = if native_clocks.contains(&id) {
            rate
        } else {
            playback.sample_rate.unwrap_or(rate)
        };
        ensure!(selected.sample_rate.unwrap_or(wave.spec().sample_rate) == expected);
    }
    Ok(())
}

/// Full-source performance and loop seams are covered by the dedicated native
/// recording check. Every selected song also verifies its initial native PCM,
/// current WAV bounds and one-shot/loop semantics in the complete layout audit.
pub(super) fn compare_music_prefix(
    snapshot: &mut library::Snapshot,
    path: &str,
    original: resonance_audio::package::Loaded,
    battle: bool,
) -> Result<()> {
    let captured = resonance_content::music::capture(snapshot, path, &|| false)?;
    let (file, repeat) = if battle {
        captured
            .battle
            .zip(captured.definition.battle)
            .map(|(file, variant)| (file, variant.repeat))
            .unwrap_or((captured.audio, captured.definition.repeat))
    } else {
        (captured.audio, captured.definition.repeat)
    };
    let track = resonance_audio::recording::Prepared::new(
        file,
        repeat.map(|r| r.start_frame..r.end_frame),
    )?;
    ensure!(
        track.repeat().is_some() == !original.score().loop_events.is_empty(),
        "music repetition changed in {path}"
    );
    let frames = track.frames().min(resonance_audio::SOURCE_RATE);
    let reverbs = original.reverbs();
    let reference =
        resonance_audio::sequence::render_preview(std::sync::Arc::new(original), reverbs, frames)?;
    let mut player = track.start(resonance_audio::SOURCE_RATE)?;
    for (frame, pcm) in reference.pcm.chunks_exact(2).enumerate() {
        ensure!(
            player.next_frame()? == Some([f32::from(pcm[0]), f32::from(pcm[1])]),
            "music prefix changed in {path} at frame {frame}"
        );
    }
    Ok(())
}

pub(super) fn first_difference(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
) -> Option<String> {
    use serde_json::Value;
    if a == b {
        return None;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                let next = format!("{path}/{key}");
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => {
                        if let Some(difference) = first_difference(a, b, &next) {
                            return Some(difference);
                        }
                    }
                    _ => return Some(format!("{next}: missing property")),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(i, (a, b))| first_difference(a, b, &format!("{path}/{i}"))),
        _ => Some(format!("{path}: {a} != {b}")),
    }
}

pub(super) fn canonicalize(
    value: &mut serde_json::Value,
    source: &Path,
    output: &Path,
    digests: &mut BTreeMap<String, String>,
) -> Result<()> {
    match value {
        serde_json::Value::String(path) => {
            if let Some(digest) = digests.get(path) {
                *path = digest.clone();
                return Ok(());
            }
            if let Some((root, file)) = [source, output]
                .into_iter()
                .map(|root| (root, root.join(&*path)))
                .find(|(_, path)| path.is_file())
            {
                let digest = if path.ends_with(".png")
                    || path.ends_with(".ktx2")
                    || path.ends_with(".texture.json")
                {
                    use sha2::{Digest, Sha256};
                    let (color, levels) = crate::texture::image_levels(root, path)?;
                    let mut hash = Sha256::new();
                    hash.update([u8::from(
                        color == resonance_content::texture::ColorSpace::Srgb,
                    )]);
                    for level in levels {
                        hash.update(level.width.to_le_bytes());
                        hash.update(level.height.to_le_bytes());
                        hash.update(level.rgba);
                    }
                    format!("{:x}", hash.finalize())
                } else if path.ends_with(".motion.json") {
                    crate::digest(
                        &resonance_content::animation::Motion::decode(&fs::read(&file)?)?
                            .encode()?,
                    )
                } else if path.ends_with(".json") {
                    let mut document = serde_json::from_slice(&fs::read(&file)?)?;
                    canonicalize(&mut document, source, output, digests)?;
                    crate::digest(&serde_json::to_vec(&document)?)
                } else {
                    crate::media::hash_file(&file)?
                };
                digests.insert(path.clone(), digest.clone());
                *path = digest;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                canonicalize(value, source, output, digests)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                canonicalize(value, source, output, digests)?;
            }
        }
        _ => {}
    }
    Ok(())
}
