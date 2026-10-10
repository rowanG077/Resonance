use super::*;
use crate::digest;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

#[test]
#[ignore = "requires cooked ending field; checks preparation without recooking or devices"]
fn ending_script_preloads_credits_text_pictures_and_music() -> Result<()> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        PathBuf::from,
    );
    let existing: Manifest =
        serde_json::from_slice(&fs::read(root.join("fields/map-534.preload.json"))?)?;
    let shared: Shared = serde_json::from_slice(&fs::read(root.join(SHARED_PATH))?)?;
    let mut inventory = Inventory::with_shared(&root, &shared.files);
    inventory.paths(existing.files.keys().map(String::as_str))?;
    let field: FieldAssets = inventory.json(&existing.inputs.field, None, Role::Field)?;
    let rebuilt = build_field(inventory, existing.inputs, &field)?;
    let credits: resonance_content::credits::Manifest =
        serde_json::from_slice(&fs::read(root.join(resonance_content::credits::PATH))?)?;
    for path in std::iter::once(resonance_content::credits::PATH)
        .chain(credits.pictures.iter().map(|picture| picture.path.as_str()))
        .chain([credits.music.asset.path.as_str()])
    {
        assert_eq!(rebuilt.files[path].sha256, existing.files[path].sha256);
        assert_eq!(rebuilt.files[path].bytes, existing.files[path].bytes);
    }
    Ok(())
}

struct Fixture(PathBuf, std::collections::BTreeSet<String>);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn write(&self, path: &str, bytes: &[u8]) -> String {
        write_atomic(&self.0.join(path), bytes).unwrap();
        digest(bytes)
    }
    fn json(&self, path: &str, value: &impl serde::Serialize) -> String {
        self.write(path, &serde_json::to_vec(value).unwrap())
    }
    fn inputs(&self) -> Inputs {
        Inputs {
            field: "fields/test.json".into(),
            audio: Default::default(),
            movies: Default::default(),
        }
    }
}

fn cook(root: &Fixture, inputs: Inputs) -> Result<Manifest> {
    let _publications = crate::publication::Session::start_if_needed(&root.0)?;
    publish(&root.0, build(root, inputs)?)
}

fn build(root: &Fixture, inputs: Inputs) -> Result<Manifest> {
    inputs.validate()?;
    let mut inventory = Inventory::new(&root.0);
    inventory.paths(root.1.iter().map(String::as_str))?;
    close_skits(&mut inventory, &font())?;
    close_battle_audio(&mut inventory)?;
    let field: FieldAssets = inventory.json(&inputs.field, None, Role::Field)?;
    build_field(inventory, inputs, &field)
}

fn fixture() -> Fixture {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let mut fixture = Fixture(
        std::env::temp_dir().join(format!(
            "resonance-field-preload-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )),
        Default::default(),
    );
    fs::create_dir(&fixture.0).unwrap();
    let mut files = std::collections::BTreeSet::new();
    for (path, bytes) in [
        (
            "fields/test/events.ssb",
            vec![0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff],
        ),
        (
            "scripts/preview.sym",
            b"script model; pub fn idle() {}".to_vec(),
        ),
        ("fields/test/messages.json", b"[]".to_vec()),
        ("fields/test/room.glb", b"fixture mesh".to_vec()),
        ("fields/test/optional.glb", b"hidden actor mesh".to_vec()),
        ("clips/idle.motion", b"idle curves".to_vec()),
        ("clips/walk.motion", b"walk curves".to_vec()),
        ("textures/shared.ktx2", b"fixture texture".to_vec()),
        ("textures/refraction.ktx2", b"displacement texture".to_vec()),
    ] {
        fixture.write(path, &bytes);
        files.insert(path.to_string());
    }
    let hash = "0".repeat(64);
    let sprite = resonance_content::effect::SpriteRecipe {
        texture: "textures/shared.ktx2".into(),
        uv: [0., 0., 1., 1.],
        additive: false,
        frames: Vec::new(),
        repeat: false,
    };
    let effects: FieldEffects = FieldEffects {
        version: resonance_content::effect::FIELD_EFFECTS_VERSION,
        sprites: resonance_content::effect::sprite::ALL
            .into_iter()
            .map(|id| (id, sprite.clone()))
            .collect(),
        refraction: resonance_content::effect::RefractionRecipe {
            sprite: resonance_content::effect::SpriteRecipe {
                texture: "textures/refraction.ktx2".into(),
                ..sprite.clone()
            },
            displacement: [1., 1.],
        },
        air_refraction: sprite,
        palette: vec![[255; 4]; resonance_content::effect::FIELD_PALETTE_COLORS],
        rising_light_destination: [0.; 3],
        emote_texture: "textures/shared.ktx2".into(),
        status_texture: "textures/shared.ktx2".into(),
        mouth_cycle: vec![0],
    };
    fixture.json("effects/test.json", &effects);
    files.insert("effects/test.json".into());
    let part = json!({"resource":0,"mesh":"fields/test/room.glb","textures":["textures/shared.ktx2"],
        "materials":[{"color":{"texture":0,"wrap_u":"repeat","wrap_v":"clamp","nearest_min":false,"nearest_mag":true},
            "multiply":null,"blend":false,"depth_write":true,"vertex_color":true,"draw_order":0}],
        "translation":[0.,0.,0.],"clips":[{"resource_slot":12,"motion":"clips/idle.motion","duration_seconds":1.},{"resource_slot":13,"motion":"clips/walk.motion","duration_seconds":1.}],
        "autoplay":false,"texture_animations":[],"bone_names":["root"]});
    let mut actor_part = part.clone();
    actor_part["mesh"] = json!("fields/test/optional.glb");
    let field = json!({"version":resonance_content::field::FIELD_VERSION,"map_id":123,"source_sha256":hash,"doors":[],"overlays":{},"unbound_geometry":[],"resource_catalogue":null,
        "blink":{"frames":[0]},
        "script":"fields/test/events.ssb",
        "messages":"fields/test/messages.json", "parts":[part],
        "actors":[{"resource":700,"parts":[actor_part],"hidden_nodes":[0],"collision":{"floors":[],"solids":[]}}],
        "ground":[{"surface":0,"vertices":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"triangles":[[0,1,2]]}],"regions":[],
        "contact_shadow":{"texture":"textures/shared.ktx2","uv_size":[1.,1.],"half_size":1.,"height_offset":0.,"alpha":255,"anchor_node":0},
        "toon_ramp":"textures/shared.ktx2","effects":"effects/test.json"});
    let field: FieldAssets = serde_json::from_value(field).unwrap();
    fixture.json("fields/test.json", &field);
    fixture.1 = files;
    fixture
}

fn font() -> BitmapFont {
    BitmapFont {
        version: 1,
        texture: "textures/shared.ktx2".into(),
        width: 16,
        height: 16,
        line_height: 8,
        glyphs: [(
            'A',
            resonance_content::font::Glyph {
                rect: [0, 0, 1, 1],
                advance: 1,
            },
        )]
        .into(),
        source_sha256: "0".repeat(64),
        executable_sha256: "0".repeat(64),
    }
}

fn asset_root() -> PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked"))
}

#[test]
#[ignore = "requires prepared current menu data; no devices"]
fn current_menu_references_use_supplied_catalogues() -> Result<()> {
    let mut menu: resonance_content::menu_data::MenuData =
        serde_json::from_slice(&fs::read(asset_root().join("game/menu-data.json"))?)?;
    let outside = u16::try_from(menu.techniques.len())?;
    menu.validate_gameplay()?;
    menu.techniques[1].prerequisite = outside;
    assert!(menu.validate_gameplay().is_err());
    menu.techniques[1].prerequisite = 0;
    menu.techniques[1].alternatives[0] = outside;
    assert!(menu.validate_gameplay().is_err());
    menu.techniques
        .resize(usize::from(outside) + 1, menu.techniques[0].clone());
    menu.techniques[1].prerequisite = outside;
    menu.validate_gameplay()?;
    let item_count = menu.items.len();
    menu.items[1].transforms_to = u16::try_from(item_count - 1)?;
    let monsters = menu.presentation.monsters.as_mut().unwrap();
    monsters.records[0].drops[0] = Some(u16::try_from(item_count - 1)?);
    menu.validate_gameplay()?;
    menu.monsters()?.validate(item_count)?;
    menu.items[1].transforms_to = u16::try_from(item_count)?;
    assert!(menu.validate_gameplay().is_err());
    menu.items[1].transforms_to = 0;
    menu.presentation.monsters.as_mut().unwrap().records[0].drops[0] =
        Some(u16::try_from(item_count)?);
    assert!(menu.monsters()?.validate(item_count).is_err());
    menu.items.push(menu.items[0].clone());
    menu.items[1].transforms_to = u16::try_from(item_count)?;
    menu.validate_gameplay()?;
    menu.monsters()?.validate(menu.items.len())?;
    let mut art: resonance_content::menu::MenuArt =
        serde_json::from_slice(&fs::read(asset_root().join("ui/menu.json"))?)?;
    art.validate(item_count)?;
    assert!(art.validate(menu.items.len()).is_err());
    let sprites = art
        .sprites
        .rects
        .get_mut(&resonance_content::menu::Sprite::ItemImages)
        .unwrap();
    sprites.push(sprites[0]);
    art.validate(menu.items.len())?;
    let pattern = art.windows[&0].patterns[0];
    art.windows.get_mut(&0).unwrap().patterns[0] = usize::MAX;
    assert!(art.validate(menu.items.len()).is_err());
    art.windows.get_mut(&0).unwrap().patterns[0] = pattern;
    art.textures
        .remove(&(resonance_content::menu::MenuArt::PORTRAIT_TEXTURES.end - 1));
    assert!(art.validate(menu.items.len()).is_err());
    Ok(())
}

#[test]
#[ignore = "requires prepared shared inventory and descriptors; no devices"]
fn current_shared_descriptors_close_against_published_inventory() -> Result<()> {
    let root = asset_root();
    let published: Shared = serde_json::from_slice(&fs::read(root.join(SHARED_PATH))?)?;
    published.validate()?;
    let mut inventory = Inventory::new(&root);
    inventory.paths(
        ["game/skits.json", resonance_content::battle_formation::PATH]
            .into_iter()
            .filter(|path| published.files.contains_key(*path)),
    )?;
    close_shared(&mut inventory)?;
    for (path, file) in inventory.files {
        let known = published
            .files
            .get(&path)
            .with_context(|| format!("unpublished shared dependency {path}"))?;
        assert_eq!(
            (&file.sha256, file.bytes),
            (&known.sha256, known.bytes),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn complete_field_includes_hidden_actors_all_clips_and_deduplicates_files() {
    let mut root = fixture();
    let first = cook(&root, root.inputs()).unwrap();
    assert!(first.is_complete());
    assert_eq!(first.files.len(), root.1.len() + 1); // Declared paths plus the field descriptor.
    assert!(
        first.files["scripts/preview.sym"]
            .roles
            .contains(&Role::Script)
    );
    assert!(first.files.contains_key("clips/idle.motion"));
    assert!(first.files.contains_key("clips/walk.motion"));
    assert!(
        first.files["textures/refraction.ktx2"]
            .roles
            .contains(&Role::Texture)
    );
    assert!(first.files.contains_key("fields/test/optional.glb"));
    let output = root.0.join(root.inputs().manifest_path().unwrap());
    let before = fs::read(&output).unwrap();
    cook(&root, root.inputs()).unwrap();
    assert_eq!(before, fs::read(output).unwrap()); // No timestamps / self-dependency.
    let roundtrip: Manifest = serde_json::from_slice(&before).unwrap();
    roundtrip.validate().unwrap();

    // Unselected palette pages must be resident before an overlay can appear.
    let page = "textures/overlay-unused.ktx2";
    root.write(page, b"unused palette");
    let overlay = "fields/test/overlay.json";
    root.json(
        overlay,
        &json!({"textures":[{
        "images":[{"path":"textures/shared.ktx2","width":16,"height":16},
            {"path":page,"width":16,"height":16}],
        "sampler":{"wrap":["clamp","clamp"],"min_filter":"linear","mag_filter":"linear",
            "lod":{"bias":0.,"min":0,"max":0,"edge":false}}}],"caption":null}),
    );
    let mut field: Value =
        serde_json::from_slice(&fs::read(root.0.join("fields/test.json")).unwrap()).unwrap();
    // Every source image is resident, including images unused by current recipes.
    let expression = "textures/skit-unused.ktx2";
    root.write(expression, b"unused portrait expression");
    let skits = "game/skits.json";
    root.json(
        skits,
        &json!({"version":2,"skits":[],"preview_order":[],"portrait_recipes":[],"portraits":{
            "851968":{"size":[16,16],"images":[
                {"texture":"textures/shared.ktx2","size":[16,16]},
                {"texture":expression,"size":[8,8]}
            ]}
        }}),
    );
    field["overlays"] = json!({"38":overlay});
    root.1.extend([overlay, page, skits].map(str::to_owned));
    root.json("fields/test.json", &field);
    let prepared = cook(&root, root.inputs()).unwrap();
    assert_eq!(prepared.files.len(), first.files.len() + 4);
    assert!(prepared.files[page].roles.contains(&Role::Texture));
    assert!(prepared.files[expression].roles.contains(&Role::Texture));
    fs::remove_file(root.0.join(expression)).unwrap();
    assert!(cook(&root, root.inputs()).is_err());
    root.write(expression, b"unused portrait expression");
    fs::remove_file(root.0.join(page)).unwrap();
    assert!(cook(&root, root.inputs()).is_err());
}

#[test]
fn missing_media_is_explicit_then_closes_when_cooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.movies.insert("movies/test.json".into());
    inputs.audio.insert("audio/test.json".into());
    let missing = build(&root, inputs.clone()).unwrap();
    assert!(!missing.is_complete());
    assert_eq!(missing.missing_inputs.len(), 2);
    let hash = root.write("movies/test.mkv", b"movie fixture");
    root.json("movies/test.json", &json!({"version":2,"audio_track":0,"path":"movies/test.mkv","sha256":hash,
        "width":640,"height":480,"frames":30,"frame_micros":33333,"sample_rate":32000,"channels":2,"audio_frames":32000}));
    root.json(
        "audio/test.json",
        &json!({"version":resonance_content::field_audio::FieldAudio::VERSION,
            "music_reverbs":{"presets":[[0.5,0.5,1.0,0.5,0.0],[0.5,0.5,1.0,0.5,0.0]],"selectors":vec![1u8;112]},
            "voice_gains":(0..128).map(|v| v as f32 / 127.).collect::<Vec<_>>(),
            "music":{},"sounds":{},"voices":{},"recipe":{}}),
    );
    let ready = build(&root, inputs).unwrap();
    assert!(ready.is_complete());
    assert!(ready.files["movies/test.mkv"].roles.contains(&Role::Movie));
}

fn audio_package(root: &Fixture) -> String {
    use resonance_audio::{
        data::Score,
        dls, mix, modulation,
        music_voice::{Controls, Tables},
        package::{Package, SampleAsset},
        resample,
    };
    let hash = root.write("audio/sample.wav", b"instrument fixture");
    let package = Package {
        version: resonance_audio::package::VERSION,
        programs: BTreeMap::new(),
        samples: [(
            1,
            SampleAsset {
                path: "audio/sample.wav".into(),
                sha256: hash,
                key: 60,
                rate: 32000,
                first_frames: 1,
                loop_start: 0,
                loop_length: 0,
            },
        )]
        .into(),
        score: Score {
            origin: resonance_audio::data::ScoreOrigin::Sequence,
            initial_bpm_1024: 120 * 1024,
            loop_start_tick: 0,
            end_tick: 100,
            tempos: vec![],
            controls: [Controls::default(); 16],
            first_events: vec![],
            loop_events: vec![],
        },
        tables: Tables {
            mix: mix::Tables {
                volume: [1.; 129],
                alternate_volume: [1.; 129],
                pan: [1.; 4],
                volume_16_scale: 1.,
                controller_14_scale: 1.,
                pan_16_scale: 1.,
                spatial: None,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
        reverbs: [[0., 0., 1., 0., 0.]; 2],
    };
    root.json("audio/package.json", &package)
}

#[test]
fn audio_closure_includes_samples_shared_packages_and_voices() {
    let root = fixture();
    let package_hash = audio_package(&root);
    let voice_hash = root.write("audio/voice.wav", b"voice fixture");
    root.json("audio/test.json", &json!({"version":resonance_content::field_audio::FieldAudio::VERSION,
            "music_reverbs":{"presets":[[0.5,0.5,1.0,0.5,0.0],[0.5,0.5,1.0,0.5,0.0]],"selectors":vec![1u8;112]},
        "voice_gains":(0..128).map(|v| v as f32 / 127.).collect::<Vec<_>>(),
        "music":{"7":{"path":"audio/package.json","sha256":package_hash}},
        "sounds":{"80":{"path":"audio/package.json","sha256":package_hash}},
        "voices":{"42":{"path":"audio/voice.wav","sha256":voice_hash,"frames":1,"sample_rate":32000,
            "source_sample_rate":32000,"channels":1,"source_name":"42.ahx","source_sha256":"0".repeat(64)}},"recipe":{}}));
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    let manifest = build(&root, inputs.clone()).unwrap();
    assert_eq!(
        manifest
            .files
            .keys()
            .filter(|p| p.starts_with("audio/"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "audio/package.json",
            "audio/sample.wav",
            "audio/test.json",
            "audio/voice.wav"
        ]
    );
    assert!(
        manifest.files["audio/sample.wav"]
            .roles
            .contains(&Role::InstrumentSample)
    );
    assert!(
        manifest.files["audio/voice.wav"]
            .roles
            .contains(&Role::Voice)
    );
    root.write("audio/sample.wav", b"changed sample");
    assert!(
        build(&root, inputs)
            .unwrap_err()
            .to_string()
            .contains("audio/sample.wav")
    );
}

#[test]
fn hashes_current_payload_and_rejects_missing_files_and_unsafe_paths() {
    let root = fixture();
    for path in ["scripts/preview.sym", "textures/shared.ktx2"] {
        let original = fs::read(root.0.join(path)).unwrap();
        let hash = root.write(path, b"modified");
        let manifest = build(&root, root.inputs()).unwrap();
        assert_eq!(manifest.files[path].sha256, hash);
        root.write(path, &original);
    }
    fs::remove_file(root.0.join("fields/test/optional.glb")).unwrap();
    assert!(build(&root, root.inputs()).is_err());
    let mut inputs = root.inputs();
    inputs.movies.insert("../outside.json".into());
    assert!(build(&root, inputs).is_err());
}

#[test]
fn late_battle_audio_descriptor_is_verified_without_loading_its_packages() {
    let mut root = fixture();
    let marker = resonance_content::battle_formation::PATH;
    root.write(marker, b"source formation fixture");
    root.1.insert(marker.into());
    // Structural preparation can run before the selected audio publication.
    assert!(build(&root, root.inputs()).unwrap().is_complete());
    let path = resonance_content::battle_audio::PATH;
    let package_hash = audio_package(&root);
    let descriptor = json!({
        "assets": {"version":resonance_content::field_audio::FieldAudio::VERSION,
            "music_reverbs":{"presets":[[0.5,0.5,1.0,0.5,0.0],[0.5,0.5,1.0,0.5,0.0]],"selectors":vec![1u8;112]},
            "music":{},"sounds":{"60":{"path":"audio/package.json","sha256":package_hash}},"voices":{},
            "voice_gains":(0..128).map(|v| v as f32 /127.).collect::<Vec<_>>()},
        "effect_spatial":[320.,5.,64.,0.,127.],"voice_spatial":[320.,5.,64.,24.,104.],
        "files":{"audio/package.json":{"sha256":package_hash,"bytes":fs::metadata(root.0.join("audio/package.json")).unwrap().len(),"roles":["audio_package"]}}
    });
    let hash = root.json(path, &descriptor);
    let manifest = build(&root, root.inputs()).unwrap();
    assert_eq!(manifest.files[path].sha256, hash);
    assert!(manifest.files[path].roles.contains(&Role::Data));
    assert!(!manifest.files.contains_key("audio/package.json"));
    let mut broken = descriptor;
    broken["voice_spatial"][1] = json!(0.);
    root.json(path, &broken);
    assert!(build(&root, root.inputs()).is_err());
}

#[test]
fn malformed_existing_media_is_not_treated_as_uncooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    root.write("audio/test.json", b"not json");
    assert!(build(&root, inputs).is_err());
}

#[test]
fn shared_inventory_excludes_shared_assets_and_checks_field_digest() {
    let root = fixture();
    let inputs = root.inputs();
    let bytes = fs::read(root.0.join(&inputs.field)).unwrap();
    let field: FieldAssets = serde_json::from_slice(&bytes).unwrap();
    let _publications = crate::publication::Session::start_if_needed(&root.0).unwrap();
    let shared_paths: std::collections::BTreeSet<_> = root
        .1
        .iter()
        .filter(|path| !path.starts_with("fields/") && !path.starts_with("clips/"))
        .cloned()
        .collect();
    let mut inventory = Inventory::new(&root.0);
    inventory
        .paths(shared_paths.iter().map(String::as_str))
        .unwrap();
    let shared = Shared {
        version: VERSION,
        files: inventory.files,
    };
    shared.validate().unwrap();
    let local = cook_field(
        &root.0,
        inputs.clone(),
        &field,
        &digest(&bytes),
        &root.1,
        &shared,
    )
    .unwrap();
    assert!(shared.files.contains_key("textures/shared.ktx2"));
    assert!(!local.files.contains_key("textures/shared.ktx2"));
    assert!(local.files.contains_key("fields/test/optional.glb"));
    assert!(local.files.contains_key("clips/idle.motion"));
    let mut changed = bytes.clone();
    changed.push(b'\n');
    root.write(&inputs.field, &changed);
    assert!(
        cook_field(&root.0, inputs, &field, &digest(&bytes), &root.1, &shared)
            .unwrap_err()
            .to_string()
            .contains("preload asset hash differs")
    );
}
