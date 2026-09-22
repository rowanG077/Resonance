use super::*;
use crate::digest;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

struct Fixture(PathBuf);
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
    fn json(&self, path: &str, value: &Value) -> String {
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

fn script() -> Vec<u8> {
    let bytes = scenario::assemble(
        r".scenario
.code_base 4
.word 4
.word 0
.word 0
.word 0
entry:
    push.s8 0
    calc 0
    branch_false alternate
    proc 0x10
    call helper
    jump finish
alternate:
    proc 0xD3
    call helper
finish:
    end
helper:
    proc 0x9B
    branch_false helper
    ret
registered_only:
    proc 0x56
    end
",
    )
    .unwrap();
    let code = &bytes[8..];
    // Add an event root pointing at the last two words, without changing code PCs.
    let mut result: Vec<_> = [10u16, 0, 0, 1]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    result.extend(
        [1u32, 77, (code.len() / 2 - 2) as u32]
            .into_iter()
            .flat_map(u32::to_be_bytes),
    );
    result.extend(code);
    result
}

fn fixture() -> Fixture {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "resonance-field-preload-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&fixture.0).unwrap();
    let mut files = BTreeMap::new();
    let window_colors = json!({"menu":vec![0;4],"dialogue":vec![0;4],"choice":vec![0;4],"popup":vec![0;4],
        "shade_top":vec![0;4],"shade_bottom":vec![0;4],"selection":vec![0;4]});
    for (path, bytes) in [
        ("fields/test/events.ssb", script()),
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
        files.insert(path.to_string(), fixture.write(path, &bytes));
    }
    let hash = "0".repeat(64);
    let ui_texture = json!({"path":"textures/shared.ktx2", "width":16, "height":16});
    let preview = json!({"scale":1.,"elevation":0.,"hidden_geometry":[],"behavior":null,"parts":[{
        "scene":{"resource":0,"mesh":"monsters/model.glb","textures":[],"materials":[],
            "translation":[0.,0.,0.],"clips":[],"autoplay":false,"texture_animations":[],"bone_names":[]},
        "attached_to":null,"additive":false}]});
    let mut menu_texture = ui_texture.clone();
    menu_texture["repeat"] = json!(true);
    menu_texture["opaque"] = json!(false);
    for (path, value) in [
        (
            "ui/menu.json",
            json!({"version":resonance_content::menu::MenuArt::VERSION,"textures":vec![menu_texture;30],
                "windows":vec![json!({"patterns":vec![0;6],"heading":null,"cursor":null,"cursor_motion":[4.0,0.1],"slices":null,"outset":4,"flourish_outset":[0,0],"foot_outset":0,"left_joins":[0,0],"left_strip":[0,0]});3],
                "fill":[48,104,120,216],"popup_fill":[24,88,80,232],
                "shade":[[24,48,48,216],[32,80,88,216]],
                "palette":vec![vec![255;4];11],
                "sprites":{"buttons":vec![[0,0,1,1];32],"item_images":vec![[0,0,1,1];528],"recipes":vec![[0,0,1,1];24],"cooking_stars":vec![[0,0,1,1];2],"item_tabs":vec![[0,0,1,1];9],"items":vec![[0,0,1,1];46],"portraits":vec![[0,0,1,1];9],"petrified_portraits":vec![[0,0,1,1];9],"condition_icons":vec![[0,0,1,1];14],"technique":vec![[0,0,1,1];13],
                    "tech_ranks":vec![[0,0,1,1];2],"elements":vec![[0,0,1,1];8],"equipment_markers":vec![[0,0,1,1];8],
                    "strategy_characters":vec![[0,0,1,1];9],
                    "numbers":[0,0,1,1],"leader":[0,0,1,1],
                    "number_colors":vec![[[255;4];2];18],"bar_colors":vec![[[255;4];4];3],"names":vec!["Name";9]},
                "labels":(["tech","unison","strategy","status","synopsis","items",
                    "ex_skill","equip","cooking","system","save","go_in","talk","shop","examine","go_out","load","customize",
                    "empty","time","encounter","combo","next","gald","play_time","encounters","max_combo",
                    "yes","no","confirm_save_a","confirm_save_b","confirm_load_a","confirm_load_b",
                    "confirm_overwrite_a","confirm_overwrite_b"]
                    .into_iter().chain(resonance_content::menu::SHOP_LABELS)
                    .map(|key|(key,key)).collect::<BTreeMap<_,_>>())}),
        ),
        (
            "ui/dialogue.json",
            json!({"version":2,"font":"ui/font.json",
            "textures":vec![ui_texture.clone();9], "cursor":ui_texture,
            "selection":{"mode":0,"color":vec![255;4],"row_offsets":vec![0;9],"bob_amplitude":0.,"bob_step":0.},
            "source_sha256":hash}),
        ),
        (
            "game/menu-data.json",
            json!({"version":resonance_content::menu_data::MenuData::VERSION,"world_map":{"names":["A","A"],"locations":{},"field_locations":{},"shops":[]},"item_categories":vec!["A";48],"inventory_categories":vec!["A";9],"items":vec![json!({"name":"A","description":"","details":"","category":0,"price":0,"transforms_to":0,"field_use":null,"equipment_stats":vec![0;7]});528],
                "item_group_prompt":{"lines":[[{"kind":"button","sprite":6},{"kind":"text","text":"A","color":9}]]},
                "item_bottle_count":{"lines":[[{"kind":"text","text":"A","color":8}]]},
                "ex_skills":{"skills":(1..=17).map(|id|(id.to_string(),json!({"name":"A","description":{"lines":[[]]},"stat_bonuses":[],"tendency":(id<17).then_some("strike"),"activation":"constant"}))).collect::<BTreeMap<_,_>>(),
                    "characters":vec![json!({"levels":[[1,2,3,4],[5,6,7,8],[9,10,11,12],[13,14,15,16]],"compounds":vec![json!({"skill":17,"required":[1,2]});24]});9],"gem_items":[40,41,42,43,496],
                    "activation_labels":(["constant","chance","battle_end","other"]).into_iter().map(|k|(k,"A")).collect::<BTreeMap<_,_>>(),
                    "labels":resonance_content::menu_data::EX_LABELS.into_iter().map(|k|(k,"A")).collect::<BTreeMap<_,_>>()},
                "figurines":{"title":"A","records":(0..resonance_content::figurine::FIGURINE_COUNT).map(|id|json!({
                    "version":resonance_content::figurine::FIGURINE_VERSION,"id":id,"name":"A","preview":preview})).collect::<Vec<_>>()},
                "manual":{"title":"A","chapters":(1..=9).map(|flag|json!({"name":"A","topics":[{
                    "name":"A","learned_flag":flag,"paragraphs":[{"lines":[[{"kind":"text","text":"A","color":9}]]}]}]})).collect::<Vec<_>>()},
                "monsters":{"records":(0..resonance_content::monster::MONSTER_COUNT).map(|id|json!({
                    "version":resonance_content::monster::MONSTER_VERSION,"id":id,"name":"A","location":"A","category":"A",
                    "statistics":[{"hp":1,"tp":0,"attack":0,"defense":0,"experience":0,"gald":0}],
                    "drops":[null,null],"steal":null,"attack_element":null,"weaknesses":[],"resistances":[],
                    "preview":preview})).collect::<Vec<_>>(),
                    "labels":(["title","number","hp","tp","attack","experience","gald","defense","drops","steal","location","attack_element","weak","strong","battle_rank","normal","hard","mania","unknown_stat","unknown_item"].into_iter().map(|k|(k,"A")).collect::<BTreeMap<_,_>>())},
                "status":{"conditions":vec!["";32],"equipment_effects":{},"technical_type":"A","strike_type":"A"},"customize":{"options":vec![json!({"name":"A","description":""});14],"difficulties":vec!["A";3],"actions":vec!["A";7],"control_buttons":[0,1,2,3,4,5,6],"color_groups":vec!["A";7],"volume_channels":vec!["A";6],"themes":vec![window_colors;3],
                    "defaults":resonance_content::menu_data::CustomizeSettings::default(),
                    "labels":(["cancel","default","cancel_help","default_help","on","off","stereo","mono","color","volume","position","position_help"]).into_iter().map(|k|(k,"A")).collect::<BTreeMap<_,_>>()},
                "cooking":{"recipes":vec![json!({"name":"A","description":"","required":[{"item":1}],"cooks":vec![json!({"base_stars":1,"grades":vec![json!({"effects":[],"recovery":0,"extras":[]});3]});9]});24],
                    "groups":vec![json!({"name":"A","category":1,"items":[1]});32],"preferences":vec![json!({"likes":[],"dislikes":[]});9],"effects":vec!["A";12],"bonus_skill":1,
                    "labels":(["cook","required","additional","success","failure","no_effect","missing","full","unknown","locked","result_join"]).into_iter().map(|k|(k,"A")).collect::<BTreeMap<_,_>>()},
                "synopsis":{"entries":vec![json!({"heading":"A","title":"A","location":null,"text":[null,null,null]});200],"months":vec!["A";12]},
                "strategy":{"groups":resonance_content::menu_data::STRATEGY_COUNTS.map(|n|vec![json!({"name":"A","description":"","details":"","characters":511});n]),"presets":vec![json!({"name":"A","members":vec![[0,0,0];9]});3],"default_positions":[0,0,0,0,0,0,0,0,0],"positions":[0,0,0,0,0,0],"keyboard":"A".repeat(90),"keys":vec!["A";9],"labels":vec!["A";3]},
                "techniques":vec![json!({"name":"A","description":"","tp":0,"tp_percent":false,"unison_usable":true,"rank":0,"element":0,"level":0,"route":0,"prerequisite":0,"alternatives":[0,0,0,0],"field_use":null});resonance_content::menu_data::TECHNIQUE_COUNT],
                "titles":vec![vec![json!({"name":"A","description":"","growth":vec![0;7]})];9],"full_names":vec!["A";9],
                "rename":{"initial_names":vec!["A";9],"defaults":vec!["A";9],"keyboard":"A".repeat(104),"heading":"A","delete":"A","default":"A","commands":["A","A","A"]},
                "labels": (["unison_title", "unison_player", "tech_unison", "tech_unison_title", "status", "next", "strength", "defense", "slash", "accuracy", "attack", "thrust", "evasion", "intelligence", "luck", "weapon", "body", "head", "arm", "accessory_1", "accessory_2", "optimal", "remove", "change_order", "optimal_selection", "optimal_slash", "optimal_thrust", "alphabetical", "parameter", "stat_arrow", "preview_loading", "item_defense", "item_accuracy", "item_evasion", "item_intelligence", "item_luck", "party_swap_target", "party_leader", "party_swap", "collectors_book", "transform_full", "transform_empty"].into_iter().map(|key|(key,"A")).collect::<BTreeMap<_,_>>())}),
        ),
        (
            "ui/font.json",
            json!({"version":1,"texture":"textures/shared.ktx2",
            "width":16,"height":16,"line_height":8,"glyphs":{"A":{"rect":[0,0,1,1],"advance":1}},
            "source_sha256":hash,"executable_sha256":hash}),
        ),
        (
            "effects/test.json",
            json!({"version":5,
            "sprites":{"0":{"texture":"textures/shared.ktx2","uv":[0.,0.,1.,1.],"additive":false},
                "10":{"texture":"textures/shared.ktx2","uv":[0.,0.,1.,1.],"additive":true}},
            "refraction":{"sprite":{"texture":"textures/refraction.ktx2","uv":[0.,0.,1.,1.],"additive":false},"displacement":[1.,1.]},
            "emote_texture":"textures/shared.ktx2","status_texture":"textures/shared.ktx2",
            "paralysis":{"anchor":"head","missing_anchor_offset":[0.,0.,0.],"rotation":{"clock":"fixed"},"intro":[],"cycle":vec![vec![json!({
                "offset":[0.,0.,0.],"size":[1.,1.],"uv":[0.,0.,1.,1.],"rotation":0.
            })];2]},"emotes":{},"mouth_cycle":[0]}),
        ),
    ] {
        files.insert(path.into(), fixture.json(path, &value));
    }
    let part = json!({"resource":0,"mesh":"fields/test/room.glb","textures":["textures/shared.ktx2"],
        "materials":[{"color":{"texture":0,"wrap_u":"repeat","wrap_v":"clamp","nearest_min":false,"nearest_mag":true},
            "multiply":null,"blend":false,"depth_write":true,"vertex_color":true,"draw_order":0}],
        "translation":[0.,0.,0.],"clips":[{"resource_slot":12,"motion":"clips/idle.motion","duration_seconds":1.},{"resource_slot":13,"motion":"clips/walk.motion","duration_seconds":1.}],
        "autoplay":false,"texture_animations":[],"bone_names":["root"]});
    let mut actor_part = part.clone();
    actor_part["mesh"] = json!("fields/test/optional.glb");
    let field = json!({"version":resonance_content::field::FIELD_VERSION,"map_id":123,"source_sha256":hash,"doors":[],"overlays":{},"unbound_geometry":[],"resource_catalogue":null,
        "blink":{"frames":[0],"initial_tick":0,"initial_spread":1},
        "script":{"path":"fields/test/events.ssb","sha256":files["fields/test/events.ssb"]},
        "messages":"fields/test/messages.json", "parts":[part],
        "actors":[{"resource":700,"parts":[actor_part],"hidden_nodes":[0]}],
        "ground":[{"surface":0,"vertices":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],"triangles":[[0,1,2]]}],"regions":[],
        "contact_shadow":{"texture":"textures/shared.ktx2","uv_size":[1.,1.],"half_size":1.,"height_offset":0.,"alpha":255,"anchor_node":0},
        "toon_ramp":"textures/shared.ktx2","effects":"effects/test.json","files":files});
    fixture.json("fields/test.json", &field);
    fixture
}

#[test]
fn field_inventory_accepts_the_full_library_without_a_fixed_file_cap() {
    let fixture = fixture();
    let mut field: FieldAssets =
        serde_json::from_slice(&fs::read(fixture.0.join("fields/test.json")).unwrap()).unwrap();
    field
        .files
        .extend((0..4097).map(|index| (format!("scripts/extra/{index}.sym"), "0".repeat(64))));
    field.validate().unwrap();
}

#[test]
fn both_branches_loops_helpers_and_registered_events_are_analyzed() {
    let script = analyze_script("test.ssb", &script(), &NativeRegistry::gqseaf()).unwrap();
    let calls: Vec<_> = script.native_calls.iter().map(|n| n.opcode).collect();
    assert_eq!(calls, [0x10, 0x56, 0x9b, 0xd3]);
    assert_eq!(script.entry_pcs.len(), 2);
    assert!(script.native_calls.iter().all(|n| n.pcs.len() == 1));
}

#[test]
fn unknown_native_is_retained_and_undecoded_code_is_rejected() {
    let bytes: Vec<_> = [4u16, 0, 0, 0, 0x2005, 0x20ff]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let report = analyze_script("unknown.ssb", &bytes, &NativeRegistry::gqseaf()).unwrap();
    assert_eq!(report.native_calls[0].opcode, 5);
    assert_eq!(report.native_calls[0].pcs, [0]);
    let mut invalid = bytes;
    invalid[8..10].copy_from_slice(&0x2105u16.to_be_bytes());
    assert!(analyze_script("invalid.ssb", &invalid, &NativeRegistry::gqseaf()).is_err());
}

#[test]
fn complete_field_includes_hidden_actors_all_clips_and_deduplicates_files() {
    let root = fixture();
    let first = cook(&root.0, root.inputs()).unwrap();
    assert!(first.is_complete());
    assert_eq!(first.scenes.len(), 2);
    assert_eq!(first.scenes[1].actor_resource, Some(700));
    assert_eq!(first.scenes[1].animation_indices, [0, 1]);
    assert_eq!(first.scenes[1].material_indices, [0]);
    assert!(first.features.contains(&Feature::Billboards));
    assert!(first.features.contains(&Feature::Choices));
    assert_eq!(first.files.len(), 15); // Shared textures, curves, scripts and menu definitions.
    assert!(
        first.files["scripts/preview.sym"]
            .roles
            .contains(&Role::Script)
    );
    assert_eq!(first.scripts.len(), 1); // Native-call analysis applies to original bytecode.
    assert!(first.files.contains_key("clips/idle.motion"));
    assert!(first.files.contains_key("clips/walk.motion"));
    assert!(
        first.files["textures/refraction.ktx2"]
            .roles
            .contains(&Role::Texture)
    );
    assert!(first.files.contains_key("fields/test/optional.glb"));
    assert_eq!(
        first.total_file_bytes,
        first.files.values().map(|f| f.bytes).sum::<u64>()
    );
    let output = root.0.join(root.inputs().manifest_path().unwrap());
    let before = fs::read(&output).unwrap();
    cook(&root.0, root.inputs()).unwrap();
    assert_eq!(before, fs::read(output).unwrap()); // No timestamps / self-dependency.
    let roundtrip: Manifest = serde_json::from_slice(&before).unwrap();
    roundtrip.validate().unwrap();
    let mut invalid = roundtrip;
    invalid.total_file_bytes += 1;
    assert!(invalid.validate().is_err());

    // Unselected palette pages must be resident before an overlay can appear.
    let page = "textures/overlay-unused.ktx2";
    let page_hash = root.write(page, b"unused palette");
    let overlay = "fields/test/overlay.json";
    let overlay_hash = root.json(
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
    let skit_hash = root.json(
        skits,
        &json!({"version":2,"skits":[],"portrait_recipes":[],"portraits":{
            "851968":{"size":[16,16],"images":[
                {"texture":"textures/shared.ktx2","size":[16,16]},
                {"texture":expression,"size":[8,8]}
            ]}
        }}),
    );
    field["overlays"] = json!({"38":overlay});
    field["files"][overlay] = json!(overlay_hash);
    field["files"][page] = json!(page_hash);
    field["files"][skits] = json!(skit_hash);
    root.json("fields/test.json", &field);
    let prepared = cook(&root.0, root.inputs()).unwrap();
    assert_eq!(prepared.files.len(), first.files.len() + 4);
    assert!(prepared.files[page].roles.contains(&Role::Texture));
    assert!(prepared.files[expression].roles.contains(&Role::Texture));
    fs::remove_file(root.0.join(expression)).unwrap();
    assert!(cook(&root.0, root.inputs()).is_err());
    root.write(expression, b"unused portrait expression");
    fs::remove_file(root.0.join(page)).unwrap();
    assert!(cook(&root.0, root.inputs()).is_err());
}

#[test]
fn missing_media_is_explicit_then_closes_when_cooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.movies.insert("movies/test.json".into());
    inputs.audio.insert("audio/test.json".into());
    let missing = build(&root.0, inputs.clone()).unwrap();
    assert!(!missing.is_complete());
    assert_eq!(missing.missing_inputs.len(), 2);
    let hash = root.write("movies/test.mkv", b"movie fixture");
    root.json("movies/test.json", &json!({"version":2,"audio_track":0,"path":"movies/test.mkv","sha256":hash,
        "width":640,"height":480,"frames":30,"frame_micros":33333,"sample_rate":32000,"channels":2,"audio_frames":32000}));
    root.json(
        "audio/test.json",
        &json!({"version":resonance_content::field_audio::FieldAudio::VERSION,
            "voice_gains":(0..128).map(|v| v as f32 / 127.).collect::<Vec<_>>(),
            "music":{},"sounds":{},"voices":{},"recipe":{}}),
    );
    let ready = build(&root.0, inputs).unwrap();
    assert!(ready.is_complete());
    assert!(ready.files["movies/test.mkv"].roles.contains(&Role::Movie));
    assert!(ready.features.contains(&Feature::Audio));
}

fn audio_package(root: &Fixture) -> String {
    use resonance_audio::{
        data::Score,
        dls, mix, modulation,
        music_voice::{Controls, Tables},
        package::{Package, SampleAsset},
        pitch, resample,
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
            has_master_track: false,
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
            pitch: pitch::Tables {
                up: [1.; 128],
                down: [1.; 128],
                semitone: 1.05946,
            },
            dls: dls::Tables {
                attenuation: [0; 194],
                inverse: [0; 1024],
                sustain: [0.; 128],
            },
            modulation: modulation::Tables {
                sine: [0; 1024],
                tremolo: [1.; 5],
            },
            coefficients: resample::Coefficients([[[0; 4]; 128]; 4]),
        },
        reverbs: [[0., 0., 1., 0., 0.]; 2],
    };
    root.json(
        "audio/package.json",
        &serde_json::to_value(package).unwrap(),
    )
}

#[test]
fn audio_closure_includes_samples_shared_packages_and_voices() {
    let root = fixture();
    let package_hash = audio_package(&root);
    let voice_hash = root.write("audio/voice.wav", b"voice fixture");
    root.json("audio/test.json", &json!({"version":resonance_content::field_audio::FieldAudio::VERSION,
        "voice_gains":(0..128).map(|v| v as f32 / 127.).collect::<Vec<_>>(),
        "music":{"7":{"path":"audio/package.json","sha256":package_hash}},
        "sounds":{"80":{"path":"audio/package.json","sha256":package_hash}},
        "voices":{"42":{"path":"audio/voice.wav","sha256":voice_hash,"frames":1,"sample_rate":32000,
            "source_sample_rate":32000,"channels":1,"source_name":"42.ahx","source_sha256":"0".repeat(64)}},"recipe":{}}));
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    let manifest = build(&root.0, inputs.clone()).unwrap();
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
        build(&root.0, inputs)
            .unwrap_err()
            .to_string()
            .contains("audio/sample.wav")
    );
}

#[test]
fn rejects_stale_inventory_missing_payload_and_unsafe_paths() {
    let root = fixture();
    for path in ["scripts/preview.sym", "textures/shared.ktx2"] {
        let original = fs::read(root.0.join(path)).unwrap();
        root.write(path, b"modified");
        let error = build(&root.0, root.inputs()).unwrap_err().to_string();
        assert!(error.contains("hash differs") && error.contains(path));
        root.write(path, &original);
    }
    fs::remove_file(root.0.join("fields/test/optional.glb")).unwrap();
    assert!(build(&root.0, root.inputs()).is_err());
    let mut inputs = root.inputs();
    inputs.movies.insert("../outside.json".into());
    assert!(build(&root.0, inputs).is_err());
}

#[test]
fn malformed_existing_media_is_not_treated_as_uncooked() {
    let root = fixture();
    let mut inputs = root.inputs();
    inputs.audio.insert("audio/test.json".into());
    root.write("audio/test.json", b"not json");
    assert!(build(&root.0, inputs).is_err());
}

#[test]
fn typed_field_handoff_preserves_manifest_and_checks_published_digest() {
    let root = fixture();
    let inputs = root.inputs();
    let bytes = fs::read(root.0.join(&inputs.field)).unwrap();
    let field: FieldAssets = serde_json::from_slice(&bytes).unwrap();
    let expected = build(&root.0, inputs.clone()).unwrap();
    let actual = cook_field(&root.0, inputs.clone(), &field, &digest(&bytes)).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let mut changed = bytes.clone();
    changed.push(b'\n');
    root.write(&inputs.field, &changed);
    assert!(
        cook_field(&root.0, inputs, &field, &digest(&bytes))
            .unwrap_err()
            .to_string()
            .contains("preload asset hash differs")
    );
}
