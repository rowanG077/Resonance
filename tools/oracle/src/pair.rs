//! One invocation verifies fixtures, replays both engines, and runs named gates.
mod resource_waits;
use super::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    process::{Command as Process, Stdio},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    version: u32,
    revision: u8,
    description: String,
    /// Explain semantic equivalence and animation/audio origin registration.
    registration: String,
    disc_sha256: String,
    native_save: Fixture,
    native_replay: Fixture,
    #[serde(default)]
    resource_wait_source: Option<resource_waits::Source>,
    dolphin: Dolphin,
    start: Start,
    replay: ReplayCase,
    frames: Vec<Frame>,
    #[serde(default)]
    audio: Vec<Audio>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    path: PathBuf,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dolphin {
    state: Fixture,
    prefix_sha256: String,
    start_poll: u32,
    version: String,
    binary_sha256: String,
    configs: BTreeMap<String, String>,
    game_settings: BTreeMap<String, String>,
    /// Opt into timestamped video; explicitly register its first frame to a VI.
    #[serde(default)]
    video_first_vi: Option<i64>,
    /// Select a recording explicitly when Dolphin creates multiple files.
    #[serde(default)]
    video_segment: Option<usize>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    map_id: u32,
    story: i32,
    position: [f32; 3],
    heading: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    name: String,
    native: String,
    #[serde(default)]
    dolphin: Option<u32>,
    dolphin_vi: u32,
    #[serde(default)]
    save_prompt: bool,
    #[serde(default)]
    action_prompt: bool,
    #[serde(default)]
    skit_prompt: bool,
    #[serde(default)]
    slot_confirmation: bool,
    /// Check popup content and opacity during entry, dismissal and settled frames.
    #[serde(default)]
    slot_popup: bool,
    /// Compare formation, saved leader, controlled member and the script lock.
    #[serde(default)]
    party_state: bool,
    /// Compare Strategy navigation, all current instructions and all three presets.
    #[serde(default)]
    strategy_state: bool,
    /// Compare Synopsis navigation, visible entries and their saved metadata.
    #[serde(default)]
    synopsis_state: bool,
    /// Effect phase and random stream continue independently of field animation.
    #[serde(default)]
    effect_state: bool,
    /// Clock-only check for older recordings without effect RNG observations.
    #[serde(default)]
    effect_clock_state: bool,
    #[serde(default)]
    eye_actors: Vec<i32>,
    #[serde(default)]
    ambient_actors: Vec<i32>,
    /// Compare Start's party display and the primary/secondary Status page.
    #[serde(default)]
    statistics_state: bool,
    #[serde(default)]
    status_state: bool,
    #[serde(default)]
    rename_state: bool,
    /// Compare the active option panel and every draft preference, including offsets.
    #[serde(default)]
    customize_state: bool,
    /// Compare the applied mixer settings with committed field preferences.
    #[serde(default)]
    audio_state: bool,
    #[serde(default)]
    collection_state: bool,
    #[serde(default)]
    world_map_state: bool,
    #[serde(default)]
    monster_state: bool,
    #[serde(default)]
    manual_state: bool,
    #[serde(default)]
    figurine_state: bool,
    #[serde(default)]
    ex_state: bool,
    #[serde(default)]
    unison_state: bool,
    #[serde(default)]
    tech_state: bool,
    /// Navigation/transitions only, for recordings predating equipment/save-point observations.
    #[serde(default)]
    tech_navigation_state: bool,
    #[serde(default)]
    equipment_state: bool,
    /// Compare shop navigation, baskets, transactions and the equipment handoff.
    #[serde(default)]
    shop_state: bool,
    #[serde(default)]
    inventory_state: bool,
    /// Compare persistent cooking choices, training, inventory and party vitals.
    #[serde(default)]
    cooking_state: bool,
    /// Compare gameplay draws from a source-verified MT19937 origin.
    #[serde(default)]
    gameplay_random_state: bool,
    /// Source particle-pool slots containing field poison puffs.
    #[serde(default)]
    poison_particles: Option<Vec<u16>>,
    /// Compare the visible paralysis symbol's atlas frame, including menu pauses.
    #[serde(default)]
    paralysis_state: bool,
    /// Compare the main menu's slide/fade phase; a removed menu is fully faded.
    #[serde(default)]
    main_menu_state: bool,
    /// Native actor to observed controller prefix; cooked samples use two ticks per frame.
    #[serde(default)]
    ambient_animations: BTreeMap<i32, String>,
    #[serde(default)]
    regions: Vec<[u32; 4]>,
    /// Exclude only the original Figurine Book's DVD transfer notice/bar.
    /// Full images remain diagnostic; source memory must confirm an active notice.
    #[serde(default)]
    disc_loading_overlay: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audio {
    name: String,
    registration: String,
    window: audio::Window,
}

pub(super) struct References<'a> {
    pub dolphin: Option<&'a Path>,
    pub native: Option<&'a Path>,
}

pub(super) fn run(
    case: &Path,
    disc: &Path,
    cooked: &Path,
    output: &Path,
    native: &Path,
    dolphin: &str,
    references: References<'_>,
) -> Result<()> {
    let bytes = fs::read(case)?;
    let pair: Pair = serde_json::from_slice(&bytes)?;
    ensure!(
        pair.version == 1
            && pair.revision == 0
            && pair.replay.game_id == "GQSEAF"
            && !pair.description.is_empty()
            && !pair.registration.is_empty()
            && !pair.frames.is_empty(),
        "invalid paired case"
    );
    ensure!(!output.exists(), "paired output already exists");
    ensure!(
        pair.dolphin.video_segment.is_none() || pair.dolphin.video_first_vi.is_some(),
        "video segment selection requires a first-VI registration"
    );
    let base = case.parent().unwrap_or(Path::new("."));
    let save = pair.native_save.verify(base)?;
    let replay = pair.native_replay.verify(base)?;
    let state = pair.dolphin.state.verify(base)?;
    let prefix = PathBuf::from(format!("{}.dtm", state.display()));
    check_hash(&prefix, &pair.dolphin.prefix_sha256)?;
    check_hash(disc, &pair.disc_sha256)?;
    let native_spec: Value = serde_json::from_slice(&fs::read(&replay)?)?;
    let source_script = append_dtm(&pair.replay, &fs::read(&prefix)?, pair.dolphin.start_poll)?;
    let input_hash = format!("{:x}", Sha256::digest(&source_script));
    resource_waits::verify(&pair, base, &native_spec, &input_hash)?;
    let reference = references.dolphin.map(Path::canonicalize).transpose()?;
    let native_reference = references.native.map(Path::canonicalize).transpose()?;
    if let Some(path) = &reference {
        let capture: Value = serde_json::from_slice(&fs::read(path.join("capture.json"))?)?;
        verify_capture(&pair, &capture, &input_hash)?;
        check_hash(
            &path.join("memory.jsonl"),
            capture["memory_watch"]["sha256"]
                .as_str()
                .context("reference has no state-observation hash")?,
        )?;
        verify_observations(&pair, &path.join("memory.jsonl"), &capture)?;
    }
    let mut names = BTreeSet::new();
    for frame in &pair.frames {
        validate_name(&frame.name)?;
        validate_name(&frame.native)?;
        ensure!(names.insert(&frame.name), "duplicate paired frame name");
        ensure!(
            !frame.disc_loading_overlay || frame.figurine_state,
            "disc-loading exclusion requires Figurine Book state observations"
        );
        ensure!(
            frame.regions.iter().all(|&[x, y, w, h]| w > 0
                && h > 0
                && u64::from(x) + u64::from(w) <= 640
                && u64::from(y) + u64::from(h) <= 480),
            "invalid paired image region"
        );
        ensure!(
            match pair.dolphin.video_first_vi {
                Some(first) => frame.dolphin.is_none() && i64::from(frame.dolphin_vi) >= first,
                None => frame
                    .dolphin
                    .is_some_and(|index| index > 0 && index < pair.replay.polls),
            } && frame.dolphin_vi < pair.replay.polls
                && native_spec["captures"]
                    .as_object()
                    .is_some_and(|captures| captures
                        .values()
                        .any(|v| v.as_str() == Some(&frame.native))),
            "unbound paired frame {}",
            frame.name
        );
    }
    names.clear();
    for audio in &pair.audio {
        validate_name(&audio.name)?;
        ensure!(names.insert(&audio.name), "duplicate paired audio name");
        ensure!(
            !audio.registration.is_empty(),
            "audio window needs an origin registration"
        );
    }
    let native_hash = if let Some(path) = &native_reference {
        let previous: Pair = serde_json::from_slice(&fs::read(path.join("case.json"))?)?;
        ensure!(
            pair.native_save.sha256 == previous.native_save.sha256
                && pair.native_replay.sha256 == previous.native_replay.sha256,
            "native reference save/replay differs from the paired fixture"
        );
        let report: Value = serde_json::from_slice(&fs::read(path.join("report.json"))?)?;
        // Older incomplete reports did not preserve the renderer hash. Keep it unknown.
        report["native_binary_sha256"].as_str().map(str::to_owned)
    } else {
        Some(file_hash(native)?)
    };
    fs::create_dir_all(output)?;
    let output = output.canonicalize()?;
    fs::write(output.join("case.json"), &bytes)?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "complete":false,"native_binary_sha256":native_hash,
            "native_reference":native_reference,
        }))?,
    )?;
    fs::write(output.join("input.dtm"), source_script)?;
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR"));
    run_logged(
        Process::new("python3")
            .arg(scripts.join("state.py"))
            .arg(&state)
            .arg("--output")
            .arg(output.join("source-state.json")),
        &output.join("state.log"),
    )?;
    let observed: Value = serde_json::from_slice(&fs::read(output.join("source-state.json"))?)?;
    if let Some(prompt) = native_spec["ambient_origin"].get("action_prompt") {
        verify_action_prompt_origin(prompt, &observed)?;
    }
    if let Some(camera) = native_spec["ambient_origin"].get("camera") {
        let saved: Value = serde_json::from_slice(&fs::read(&save)?)?;
        verify_camera_origin(camera, &observed, &saved["state"]["camera"])?;
    }
    if let Some(actors) = native_spec["ambient_origin"]["actors"].as_object() {
        for (id, origin) in actors {
            let id: i64 = id.parse()?;
            let actor = std::iter::once(&observed["controlled_actor"])
                .chain(
                    observed["actors"]
                        .as_array()
                        .context("missing source actors")?,
                )
                .find(|a| a["id"].as_i64() == Some(id))
                .context("ambient origin actor is missing")?;
            let ai = &actor["autonomy"];
            let behavior = match ai["kind"].as_u64() {
                Some(0) => "stationary",
                Some(1) => "wander",
                Some(2) => "wander_near_home",
                Some(4) => "watch_player",
                Some(5) => "approach_player",
                Some(10) => "player",
                Some(12) => "chase_player",
                _ => anyhow::bail!("unsupported source ambient behavior"),
            };
            let state = actor["behavior"]
                .as_u64()
                .context("missing source behavior")? as u32;
            let expected = json!({"behavior":behavior,"speed":ai["speed"],"home":ai["home"],
                "radius":ai["radius"],"conversing":false,
                "activity":ambient_activity(state)?,"initialized":state & 0x8000 != 0,
                "remaining":ai["timer"],"floor_available":ai["floor_attributes"] != 0});
            ensure!(
                same_values(&origin["autonomy"], &expected)
                    && ai["override"] == 65535
                    && same_values(&origin["position"], &actor["position"])
                    && same_values(&origin["heading"], &actor["heading_current"])
                    && same_values(&origin["target_heading"], &actor["heading_target"]),
                "ambient origin differs from source actor {id}"
            );
            if let Some(slot) = origin["animation_slot"].as_u64() {
                ensure!(
                    (actor["script_animation_slots"]
                        .as_array()
                        .context("missing scripted animation slots")?
                        .contains(&json!(slot))
                        || slot == if state & 0x3fff == 2 { 36 } else { 12 })
                        && origin["animation_repeat"].as_bool()
                            == actor["animation_tracks"][0]["flags"]
                                .as_u64()
                                .map(|v| v & 8 == 0)
                        && origin["animation_sample"].as_f64()
                            == actor["animation_tracks"][0]["time"]
                                .as_f64()
                                .map(|v| v * 2.),
                    "ambient origin differs from source actor {id}'s animation"
                );
            } else {
                ensure!(
                    origin["animation_slot"].is_null()
                        && actor["resource_address"] == "801e73e0"
                        && origin["animation_sample"] == 0
                        && origin["animation_repeat"] == false,
                    "only a source scene locator can omit an animation origin"
                );
            }
        }
    }
    if let Some(eyes) = native_spec["ambient_origin"]["eyes"].as_object() {
        for (id, origin) in eyes {
            let id: i64 = id.parse()?;
            let actor = std::iter::once(&observed["controlled_actor"])
                .chain(
                    observed["actors"]
                        .as_array()
                        .context("missing source actors")?,
                )
                .find(|actor| actor["id"].as_i64() == Some(id))
                .context("eye origin actor is missing from the source")?;
            let bytes = actor["appearance_channels"]
                .as_array()
                .context("missing eye state")?;
            let word = |start, length| -> Result<u32> {
                bytes
                    .get(start..start + length)
                    .context("truncated eye state")?
                    .iter()
                    .try_fold(0, |word, byte| {
                        Ok(word << 8 | u32::try_from(byte.as_u64().context("invalid eye state")?)?)
                    })
            };
            ensure!(
                *origin == observed_blink(&observed["blink_sequence"], word(0, 4)?, word(4, 2)?)?,
                "eye origin differs from the source blink phase"
            );
        }
    }
    let effect_tick = native_spec["ambient_origin"]["effect_tick"].as_u64();
    if let Some(random) = native_spec["ambient_origin"].get("gameplay_random") {
        ensure!(
            random == &observed["gameplay_random"],
            "gameplay random origin differs from the source state"
        );
    }
    ensure!(
        !pair.frames.iter().any(|frame| frame.gameplay_random_state)
            || native_spec["ambient_origin"]
                .get("gameplay_random")
                .is_some(),
        "gameplay random comparison requires an observed origin"
    );
    if let Some(seed) = native_spec["ambient_origin"]["random_state"].as_u64() {
        ensure!(
            Some(seed) == observed["random_state"].as_u64(),
            "random origin differs from the source state"
        );
    }
    if let Some(tick) = effect_tick {
        ensure!(
            Some(tick) == observed["title"]["presentation_counter"].as_u64(),
            "effect origin differs from the source draw counter"
        );
    }
    if let Some(sparks) = native_spec["ambient_origin"]["save_sparks"].as_array() {
        verify_spark_origin(sparks, &observed, effect_tick)?;
    }
    if let Some(puffs) = native_spec["ambient_origin"]["poison_puffs"].as_array() {
        let source = observed["particles"]
            .as_array()
            .context("missing particle observations")?;
        let source = source
            .iter()
            .filter(|p| p["recipe_address"] == "8020a4e4")
            .map(|p| {
                json!({"age":20 - p["timer"].as_u64().unwrap(),"position":p["position"],
                "size":p["size"][0],"speed_sixteenths":p["velocity"][2].as_f64().unwrap() * 16.})
            })
            .collect::<Vec<_>>();
        ensure!(
            same_values(&json!(puffs), &json!(source)),
            "poison origin differs from the source particles"
        );
    }
    if let Some(leaves) = native_spec["ambient_origin"]["flutters"].as_array() {
        let particles: Vec<_> = observed["particles"]
            .as_array()
            .context("missing source particles")?
            .iter()
            .filter(|p| p["callback_address"] == "80086fc4")
            .collect();
        ensure!(
            leaves.len() == particles.len(),
            "leaf origin count differs from source"
        );
        for (leaf, particle) in leaves.iter().zip(particles) {
            let timer = particle["timer"].as_u64().context("missing leaf timer")?;
            let expected = json!({"kind":25,"age":149u64.checked_sub(timer).context("leaf timer exceeds script lifetime")?,
                "lifetime":150,"position":particle["position"],"size":particle["size"][0],"rgba":particle["rgba"],
                "motion":{"rotation":particle["rotation"],"fall_speed":particle["velocity"][2],
                    "spin":0.2,"heading":particle["angular_velocity"][1],"turn_after":particle["size_delta"]}});
            ensure!(
                particle["recipe_address"] == "8020a584"
                    && particle["rgba"][3] == 255
                    && same_values(leaf, &expected),
                "leaf origin differs from observed motion"
            );
        }
    }
    if let Some(waits) = native_spec["ambient_origin"]["background_waits"].as_array() {
        let entries = observed["script_entries"]
            .as_array()
            .context("missing source script registry")?;
        let instances = observed["script_instances"]
            .as_array()
            .context("missing source script instances")?;
        for wait in waits {
            let pc = wait["pc"].as_u64().context("missing ambient wait PC")?;
            let entry = entries
                .iter()
                .filter(|e| e["pc"].as_u64().is_some_and(|p| p <= pc))
                .max_by_key(|e| e["pc"].as_u64())
                .context("ambient wait has no source entry")?;
            ensure!(
                entry["kind"] == 2 && entry["key"] == wait["key"],
                "ambient wait is outside its source event"
            );
            let matches: Vec<_> = instances
                .iter()
                .filter(|i| i["kind"] == 4 && i["pc"] == wait["pc"])
                .collect();
            ensure!(
                matches.len() == 1,
                "ambient wait source is missing or ambiguous"
            );
            let source = matches[0];
            ensure!(
                source["wait_mode"] == 0
                    && source["wait_value"].as_u64().and_then(|n| n.checked_add(1))
                        == wait["remaining"].as_u64()
                    && source["flags"]
                        == if wait["require_control"] == true {
                            64
                        } else {
                            0
                        },
                "ambient wait differs from source timer or control gate"
            );
        }
    }
    ensure!(
        observed["movie"]["input_count"].as_u64() == Some(u64::from(pair.dolphin.start_poll)),
        "Dolphin input origin differs from the manifest"
    );
    verify_start(
        &pair.start,
        &observed["field"]["map_id"],
        &observed["progress"]["story"],
        &observed["controlled_actor"]["position"],
        &observed["controlled_actor"]["heading_current"],
    )?;
    let saved: Value = serde_json::from_slice(&fs::read(&save)?)?;
    let mut native_start = pair.start;
    if let Some(origin) = native_spec.get("story_origin") {
        ensure!(
            saved["state"]["progress"]["script_globals"][16] == origin["from"]
                && origin["to"] == pair.start.story,
            "controlled story origin differs from saved/source progress"
        );
        native_start.story = origin["from"]
            .as_i64()
            .context("invalid story origin")?
            .try_into()?;
    }
    verify_start(
        &native_start,
        &saved["state"]["map_id"],
        &saved["state"]["progress"]["script_globals"][16],
        &saved["state"]["position"],
        &saved["state"]["heading"],
    )?;
    if let Some(path) = &native_reference {
        fs::create_dir(output.join("native"))?;
        for entry in fs::read_dir(path.join("native"))? {
            let entry = entry?;
            ensure!(
                entry.file_type()?.is_file(),
                "unexpected native reference entry"
            );
            fs::copy(entry.path(), output.join("native").join(entry.file_name()))?;
        }
    } else {
        run_logged(
            Process::new(native)
                .arg(&save)
                .arg(&replay)
                .arg(output.join("native"))
                .arg(cooked),
            &output.join("native.log"),
        )?;
    }
    let native_record: Value =
        serde_json::from_slice(&fs::read(output.join("native/recording.json"))?)?;
    ensure!(
        native_record["output_stage"] == "framebuffer",
        "Dolphin frame dumps require native framebuffer captures, before display positioning"
    );
    ensure!(
        native_record["complete"] == true
            && native_record["audio_device"] == false
            && native_record["keyboard_input"] == true
            && native_record["late_reads"] == 0
            && native_record["width"] == 640
            && native_record["height"] == 480,
        "native replay did not meet recording invariants"
    );
    ensure!(
        native_record["updates"] == native_spec["updates"]
            && native_spec["captures"].as_object().is_some_and(|expected| {
                native_record["captures"].as_array().is_some_and(|actual| {
                    expected.len() == actual.len()
                        && expected.iter().all(|(update, name)| {
                            actual.iter().any(|capture| {
                                capture["name"] == *name
                                    && capture["update"].as_u64() == update.parse().ok()
                            })
                        })
                })
            }),
        "native recording differs from the pinned replay schedule"
    );
    let initialized = &native_record["initial"];
    verify_start(
        &native_start,
        &initialized["map_id"],
        &initialized["progress"]["script_globals"][16],
        &initialized["position"],
        &initialized["heading"],
    )?;
    let dolphin_output = reference.clone().unwrap_or_else(|| output.join("dolphin"));
    if reference.is_none() {
        let mut command = Process::new("python3");
        command
            .arg(scripts.join("capture.py"))
            .arg("--disc")
            .arg(disc)
            .arg("--movie")
            .arg(output.join("input.dtm"))
            .arg("--initial-state")
            .arg(&state)
            .arg("--output")
            .arg(output.join("dolphin"))
            .arg("--dolphin")
            .arg(dolphin)
            .arg("--xvfb");
        if pair.frames.iter().any(|f| f.synopsis_state) {
            command.arg("--watch-synopsis");
        }
        let actors: std::collections::BTreeSet<_> = pair
            .frames
            .iter()
            .flat_map(|f| f.eye_actors.iter().chain(&f.ambient_actors))
            .collect();
        ensure!(actors.len() <= 8, "too many observed actors");
        for actor in actors {
            command.arg("--watch-actor").arg(actor.to_string());
        }
        let particles: BTreeSet<_> = pair
            .frames
            .iter()
            .filter_map(|f| f.poison_particles.as_ref())
            .flatten()
            .collect();
        ensure!(
            particles.len() <= 4 && particles.iter().all(|&&slot| slot < 2048),
            "too many or invalid particle slots"
        );
        for slot in particles {
            command.arg("--watch-particle").arg(slot.to_string());
        }
        if let Some(first_vi) = pair.dolphin.video_first_vi {
            let last_vi = i64::from(pair.frames.iter().map(|f| f.dolphin_vi).max().unwrap());
            let observations = last_vi
                .checked_sub(first_vi.min(0))
                .and_then(|vi| vi.checked_add(1))
                .and_then(|count| u32::try_from(count).ok())
                .context("video observation count exceeds the capture limit")?;
            command
                .arg("--video")
                .arg("--watch-vis")
                .arg(observations.to_string())
                // Long catalogue sweeps need time for lossless capture as well as emulation.
                .arg("--timeout")
                .arg((observations / 15 + 60).max(240).to_string());
        } else {
            command
                .arg("--frame")
                .arg(
                    pair.frames
                        .iter()
                        .filter_map(|f| f.dolphin)
                        .max()
                        .unwrap()
                        .to_string(),
                )
                .args(["--keep-frames", "--watch-state"]);
        }
        run_logged(&mut command, &output.join("capture.log"))?;
    }
    let capture: Value = serde_json::from_slice(&fs::read(dolphin_output.join("capture.json"))?)?;
    verify_capture(&pair, &capture, &input_hash)?;
    if let Some(first_vi) = pair.dolphin.video_first_vi {
        let videos = capture["video"]
            .as_array()
            .context("missing timestamped video")?;
        ensure!(
            videos.len() == 1 || pair.dolphin.video_segment.is_some(),
            "multiple video recordings require explicit segment selection"
        );
        let segment = videos
            .get(pair.dolphin.video_segment.unwrap_or(0))
            .context("selected video segment is missing")?;
        let video = dolphin_output.join(segment["path"].as_str().context("missing video path")?);
        check_hash(
            &video,
            segment["sha256"].as_str().context("missing video hash")?,
        )?;
        let requested: Vec<_> = pair.frames.iter().map(|f| f.dolphin_vi).collect();
        let cached = reference
            .as_ref()
            .map(|p| p.parent().unwrap().join("video-frames"));
        if !cached
            .as_ref()
            .is_some_and(|p| p.join("frames.json").is_file())
            || !super::video::reuse(
                cached.as_ref().unwrap(),
                &output.join("video-frames"),
                segment["sha256"].as_str().unwrap(),
                first_vi,
                &requested,
            )?
        {
            super::video::run(&video, &output.join("video-frames"), first_vi, &requested)?;
        }
    }
    let mut results = Vec::new();
    let mut popup_content = Value::Null;
    let alias = save_point_alias(&capture);
    let memory: BTreeMap<u32, Value> = fs::read_to_string(dolphin_output.join("memory.jsonl"))?
        .lines()
        .map(|line| -> Result<_> {
            let mut value = source_observation(line, alias)?;
            let vi = u32::try_from(value["vi_sample"].as_u64().context("missing VI index")?)?;
            // Dismissal returns to the slot list before its old message fades.
            // Retain content from observed menu state, never the native output.
            if let Some(screen) = value["save_screen_word"].as_u64().map(|v| v >> 16)
                && matches!(screen, 6 | 13 | 14) {
                popup_content = json!({"confirmation":{
                    "kind":match screen { 6 => "save", 13 => "overwrite", _ => "load" },
                    "bank":value["save_selection_word"].as_u64().context("missing bank")? >> 24,
                    "yes":(value["save_mode_word"].as_u64().context("missing choice")? >> 8) & 255 == 0,
                }});
            }
            if value["save_popup_alpha_word"].as_u64().is_some_and(|v| v >> 24 == 0) {
                popup_content = Value::Null;
            }
            value["save_popup_content"] = popup_content.clone();
            Ok((vi, value))
        })
        .collect::<Result<_>>()?;
    let mut states = Vec::new();
    let mut passed = true;
    for frame in &pair.frames {
        let source_state = memory
            .get(&frame.dolphin_vi)
            .context("missing registered VI")?;
        let native_state = native_record["captures"]
            .as_array()
            .context("missing native captures")?
            .iter()
            .find(|c| c["name"] == frame.native)
            .context("missing native state")?;
        for (section, location, value) in [
            ("field", "8035a768 10d0", "map_id"),
            ("progress", "8035a578 40", "story"),
        ] {
            let address = u32::from_str_radix(
                observed[section]["address"]
                    .as_str()
                    .context("missing session address")?,
                16,
            )?;
            let current = source_state[format!("{section}_address")].as_u64();
            let follows_pointer =
                capture["memory_watch"]["locations"][location].as_str() == Some(value);
            ensure!(
                current.is_some_and(|p| (0x80000000..0x81800000).contains(&p) && p % 4 == 0),
                "invalid source {section} address"
            );
            ensure!(
                follows_pointer || current == Some(u64::from(address)),
                "source {section} storage moved; observed words are stale"
            );
        }
        let location = json!({"native_map":native_state["map_id"],"dolphin_map":source_state["map_id"],
            "native_story":native_state["story"],"dolphin_story":source_state["story"],
            "passed":native_state["map_id"] == source_state["map_id"]
                && native_state["story"] == source_state["story"]});
        let presentation: Value = serde_json::from_slice(&fs::read(
            output.join(format!("native/{}.json", frame.native)),
        )?)?;
        let position_error = vector_error(&native_state["position"], source_state, "controlled")?;
        let camera_position_error =
            vector_error(&presentation["camera"]["position"], source_state, "camera")?;
        let camera_target_error = vector_error(
            &presentation["camera"]["target"],
            source_state,
            "camera_target",
        )?;
        let heading_error = ((native_state["heading"]
            .as_f64()
            .context("missing native heading")?
            - observed_float(source_state, "controlled_heading_bits")?
            + 180.)
            .rem_euclid(360.)
            - 180.)
            .abs();
        let prompt = |enabled: bool, kind| {
            enabled
                .then(|| field_prompt(kind, native_state, source_state))
                .transpose()
        };
        let save = prompt(frame.save_prompt, Prompt::Save)?;
        let action = prompt(frame.action_prompt, Prompt::Action)?;
        let skit = prompt(frame.skit_prompt, Prompt::Skit)?;
        let menu = (frame.slot_confirmation || frame.slot_popup)
            .then(|| slot_confirmation(native_state, source_state, frame.slot_popup))
            .transpose()?;
        let audio_settings = frame.audio_state.then(|| -> Result<Value> {
            let expected = audio_settings(source_state)?;
            let actual = &native_state["audio_settings"];
            Ok(json!({"expected":expected,"actual":actual,"passed":same_values(actual,&expected)}))
        }).transpose()?;
        let tech = (frame.tech_state || frame.tech_navigation_state)
            .then(|| tech_state(native_state, source_state, frame.tech_state))
            .transpose()?;
        let poison = frame
            .poison_particles
            .as_ref()
            .map(|slots| poison_state(native_state, source_state, slots))
            .transpose()?;
        let paralysis = frame
            .paralysis_state
            .then(|| -> Result<_> {
                let uv = observed_word(source_state, "field_symbol_uv_word")
                    .context("missing paralysis atlas observation")?;
                let frame = match uv {
                    0x8910b81f => 0,
                    0x8900b80f => 1,
                    _ => anyhow::bail!("unexpected paralysis atlas rectangle {uv:08x}"),
                };
                let actual = &native_state["paralysis"]["frame"];
                Ok(json!({"passed":actual == frame,"native":actual,"source":frame}))
            })
            .transpose()?;
        let main_menu = frame
            .main_menu_state
            .then(|| -> Result<_> {
                let fade = observed_word(source_state, "main_menu_fade_word")
                    .context("missing main menu fade observation")?
                    >> 24;
                let mut expected = json!({"fade":fade});
                let mut actual =
                    json!({"fade":native_state["main_menu_fade"].as_u64().unwrap_or(255)});
                let menu = &native_state["menu"];
                let page = &menu["page"];
                if page.as_str().is_some_and(|page| {
                    matches!(page, "Main" | "Party" | "System") || page.starts_with("Character(")
                }) {
                    expected["first"] = json!(
                        observed_word(source_state, "party_menu_first_word")
                            .context("missing Main party viewport")?
                            >> 16
                    );
                    expected["character"] = json!(
                        observed_word(source_state, "party_menu_display_word")
                            .context("missing Main selected character")?
                            >> 24
                    );
                    actual["first"] = menu["first_character"].clone();
                    actual["character"] = menu["character"].clone();
                }
                Ok(json!({"passed":actual == expected,"native":actual,"source":expected}))
            })
            .transpose()?;
        let gameplay_random = frame
            .gameplay_random_state
            .then(|| -> Result<_> {
                let pointer = observed_word(source_state, "gameplay_random_pointer")
                    .context("missing gameplay random pointer")?;
                let remaining = observed_word(source_state, "gameplay_random_remaining")
                    .context("missing gameplay random cursor")?;
                let index = if remaining == u64::from(u32::MAX) {
                    624
                } else {
                    ensure!(
                        (0x802ce560..=0x802cef20).contains(&pointer) && pointer % 4 == 0,
                        "invalid source gameplay random pointer"
                    );
                    let index = (pointer - 0x802ce560) / 4;
                    ensure!(
                        remaining == 624 - index,
                        "inconsistent source gameplay random cursor"
                    );
                    index
                };
                let actual = native_state["gameplay_random_index"]
                    .as_u64()
                    .context("missing native gameplay random cursor")?;
                Ok(json!({"passed":actual == index,"native":actual,"source":index}))
            })
            .transpose()?;
        let effects = (frame.effect_state || frame.effect_clock_state).then(|| -> Result<_> {
            let mut actual = json!({"clock":native_state["effect_counter"]});
            let mut expected = json!({"clock":observed_word(source_state, "presentation_counter").context("missing source effect clock")?});
            if frame.effect_state {
                actual["random_state"] = native_state["random_state"].clone();
                expected["random_state"] = json!(observed_word(source_state, "random_state").context("missing source random state")?);
            }
            Ok(json!({"passed":actual == expected,"native":actual,"source":expected}))
        }).transpose()?;
        let ambient = frame
            .ambient_animations
            .iter()
            .map(|(actor, prefix)| -> Result<_> {
                let actual = &native_state["ambient_animations"][actor.to_string()];
                let sample_error = (actual["sample"]
                    .as_f64()
                    .context("missing ambient sample")?
                    - 2. * observed_float(source_state, &format!("{prefix}_time_bits"))?)
                .abs();
                let rate_error = (actual["rate"].as_f64().context("missing ambient rate")?
                    - 2. * observed_float(source_state, &format!("{prefix}_speed_bits"))?)
                .abs();
                Ok(
                    json!({"actor":actor,"sample_error":sample_error,"rate_error":rate_error,
                "passed":sample_error <= 0.01 && rate_error <= 0.01}),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let eyes = frame.eye_actors.iter().map(|actor| -> Result<_> {
            ensure!(observed_actor_id(source_state, *actor)? == i64::from(*actor),
                "observed eye actor slot was reused");
            let word = |suffix| -> Result<u32> {
                Ok(u32::try_from(observed_word(source_state, &format!("actor_{actor}_{suffix}")).context("missing eye observation")?)?)
            };
            let expected = observed_blink(&observed["blink_sequence"], word("eye_mode_word")?, word("eye_mouth_mode_word")? >> 16)?;
            let actual = &native_state["eyes"][actor.to_string()];
            Ok(json!({"actor":actor,"native":actual,"source":expected,"passed":*actual == expected}))
        }).collect::<Result<Vec<_>>>()?;
        let actors = frame.ambient_actors.iter().map(|id| -> Result<_> {
            ensure!(observed_actor_id(source_state, *id)? == i64::from(*id),
                "observed ambient actor slot was reused");
            let word = |suffix| -> Result<u32> {
                Ok(u32::try_from(observed_word(source_state, &format!("actor_{id}_{suffix}"))
                    .context("missing ambient actor observation")?)?)
            };
            let state = word("behavior_word")? & 0xffff;
            let expected = json!({"activity":ambient_activity(state)?,"initialized":state & 0x8000 != 0,
                "remaining":word("decision_timer")? as i32,"floor_available":word("floor_attributes")? != 0});
            let actual = &native_state["actors"][id.to_string()];
            let position_error = vector_error(&actual["position"], source_state, &format!("actor_{id}"))?;
            let heading_error = (actual["heading"].as_f64().context("missing ambient heading")?
                - observed_float(source_state, &format!("actor_{id}_heading_bits"))? + 180.).rem_euclid(360.) - 180.;
            let decision = matching_fields(&actual["autonomy"], &expected);
            Ok(json!({"actor":id,"source":expected,"native":actual,"position_error":position_error,
                "heading_error":heading_error.abs(),"passed":decision && position_error <= 0.01 && heading_error.abs() <= 0.01}))
        }).collect::<Result<Vec<_>>>()?;
        let mut gates = json!({
            "save_prompt":save,"action_prompt":action,"skit_prompt":skit,
            "slot_confirmation":menu,"audio_settings":audio_settings,"tech":tech,
            "gameplay_random":gameplay_random,"poison_puffs":poison,"paralysis":paralysis,
            "main_menu":main_menu,"effects":effects
        });
        type CompareState = fn(&Value, &Value) -> Result<Value>;
        for (name, enabled, compare) in [
            ("party", frame.party_state, party_state as CompareState),
            ("strategy", frame.strategy_state, strategy_state),
            ("synopsis", frame.synopsis_state, synopsis_state),
            ("statistics", frame.statistics_state, statistics_state),
            ("status", frame.status_state, status_state),
            ("rename", frame.rename_state, rename_state),
            ("customize", frame.customize_state, customize_state),
            ("collection", frame.collection_state, collection_state),
            ("world_map", frame.world_map_state, world_map_state),
            ("monster", frame.monster_state, monster_state),
            ("manual", frame.manual_state, manual_state),
            ("figurine", frame.figurine_state, figurine_state),
            ("ex_skills", frame.ex_state, ex_state),
            ("unison", frame.unison_state, unison_state),
            ("equipment", frame.equipment_state, equipment_state),
            ("shop", frame.shop_state, shop_state),
            ("inventory", frame.inventory_state, inventory_state),
            ("cooking", frame.cooking_state, cooking_state),
        ] {
            gates[name] = enabled
                .then(|| compare(native_state, source_state))
                .transpose()?
                .unwrap_or(Value::Null);
        }
        let good = [
            position_error,
            camera_position_error,
            camera_target_error,
            heading_error,
        ]
        .into_iter()
        .all(|error| error <= 0.01)
            && gates
                .as_object()
                .unwrap()
                .values()
                .all(|gate| gate.is_null() || gate["passed"] == true)
            && ambient.iter().all(|a| a["passed"] == true)
            && eyes.iter().all(|a| a["passed"] == true)
            && actors.iter().all(|a| a["passed"] == true)
            && location["passed"] == true;
        passed &= good;
        let mut state = json!({"name":frame.name,"dolphin_vi":frame.dolphin_vi,
            "location":location,
            "position_error":position_error,"camera_position_error":camera_position_error,
            "camera_target_error":camera_target_error,"heading_error":heading_error,
            "eyes":eyes,"actors":actors,"ambient_animations":ambient,
            "tolerance":0.01,"passed":good});
        let Value::Object(gates) = gates else {
            unreachable!()
        };
        state.as_object_mut().unwrap().extend(gates);
        states.push(state);
        let source = match frame.dolphin {
            Some(index) => dolphin_output.join(format!("user/Dump/Frames/framedump_{index}.png")),
            None => output.join(format!("video-frames/vi-{:06}.png", frame.dolphin_vi)),
        };
        let actual = output.join(format!("native/{}.png", frame.native));
        if frame.disc_loading_overlay {
            ensure!(
                source_state["figurine_model_load_word"]
                    .as_u64()
                    .is_some_and(|v| v & 255 == 1)
                    && source_state["figurine_opacity_word"]
                        .as_u64()
                        .is_some_and(|v| v >> 16 & 255 != 0),
                "disc-loading exclusion requires a visible source transfer notice"
            );
        }
        let mut regions = vec![None];
        regions.extend(frame.regions.iter().copied().map(Some));
        for (i, region) in regions.into_iter().enumerate() {
            let name = format!("{}-{i}", frame.name);
            let full = compare(
                &source,
                &actual,
                &output.join("images").join(&name),
                8,
                0.01,
                region,
            )?;
            let exclusions = if frame.disc_loading_overlay {
                // Authored text at (360,220), 16x20 glyphs, and the transfer line
                // at y=244. Bounds include shadows and 448-to-480 presentation scaling.
                &[[359, 235, 145, 24], [358, 258, 260, 7]][..]
            } else {
                &[]
            };
            let good = if exclusions.is_empty() {
                full
            } else {
                compare_excluding(
                    &source,
                    &actual,
                    &output
                        .join("images")
                        .join(&name)
                        .join("without-disc-loading"),
                    8,
                    0.01,
                    region,
                    exclusions,
                )?
            };
            passed &= good;
            results.push(json!({"name":name,"passed":good,"full_image_passed":full,
                "excluded_regions":exclusions,
                "exclusion_reason":if exclusions.is_empty() { None } else {
                    Some("Original DVD transfer telemetry; native shows loading text only during actual asset preparation.")
                }}));
        }
    }
    let mut audio_results = Vec::new();
    if !pair.audio.is_empty() {
        let recordings = capture["audio"]["recordings"]
            .as_array()
            .context("missing Dolphin audio evidence")?;
        let dsp: Vec<_> = recordings
            .iter()
            .filter(|r| {
                r["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("_dspdump.wav"))
            })
            .collect();
        ensure!(
            dsp.len() == 1 && dsp[0]["finalized"] == true,
            "expected one finalized DSP recording"
        );
        for window in &pair.audio {
            let report = audio::compare(
                &dolphin_output.join(dsp[0]["path"].as_str().unwrap()),
                &output.join("native/audio.wav"),
                None,
                &window.window,
            )?;
            passed &= report.passed;
            audio_results.push(
                json!({"name":window.name,"registration":window.registration,"report":report}),
            );
        }
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "complete":true,"passed":passed,"case_sha256":format!("{:x}",Sha256::digest(&bytes)),
            "native_binary_sha256":native_hash,
            "native_reference":native_reference,
            "native_recording_sha256":file_hash(&output.join("native/recording.json"))?,
            "reference":reference,
            "reference_capture_sha256":file_hash(&dolphin_output.join("capture.json"))?,
            "reference_memory_sha256":file_hash(&dolphin_output.join("memory.jsonl"))?,
            "description":pair.description,"registration":pair.registration,"content_identity":native_record["identity"],
            "images":results,"states":states,"audio":audio_results,"dolphin":capture,"native":native_record,
        }))?,
    )?;
    ensure!(
        passed,
        "paired oracle gates failed; see {}",
        output.join("report.json").display()
    );
    Ok(())
}
fn save_point_alias(capture: &Value) -> bool {
    let names = ["field_control_flags_word", "field_save_point_word"];
    let mut addresses = Vec::new();
    for (address, name) in capture["memory_watch"]["locations"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if name.as_str().is_some_and(|name| names.contains(&name)) {
            addresses.push(address);
        }
    }
    for (address, aliases) in capture["memory_watch"]["aliases"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if aliases.as_array().is_some_and(|aliases| {
            aliases
                .iter()
                .any(|name| name.as_str().is_some_and(|name| names.contains(&name)))
        }) {
            addresses.push(address);
        }
    }
    // Hex case is immaterial; pointer chains and other addresses are not aliases.
    // Independently recorded fields at other addresses remain available as recorded.
    !addresses.is_empty()
        && addresses
            .iter()
            .all(|address| address.eq_ignore_ascii_case("8035a73c"))
}

fn source_observation(line: &str, alias: bool) -> Result<Value> {
    let mut source: Value = serde_json::from_str(line)?;
    if alias {
        let keys = ["field_control_flags_word", "field_save_point_word"];
        let mut observed = None;
        for key in keys {
            if let Some(value) = source.get(key) {
                let word = value
                    .as_u64()
                    .filter(|&word| word <= u64::from(u32::MAX))
                    .with_context(|| format!("invalid source u32 {key}"))?;
                ensure!(
                    observed.is_none_or(|previous| previous == word),
                    "conflicting source values for field_save_point_word"
                );
                observed = Some(word);
            }
        }
        if let Some(word) = observed {
            for key in keys {
                source[key] = json!(word);
            }
        }
    }
    Ok(source)
}

// Fail before rendering when a reused recording lacks observations for these gates.
fn verify_observations(pair: &Pair, path: &Path, capture: &Value) -> Result<()> {
    let alias = save_point_alias(capture);
    let memory: BTreeMap<u64, Value> = fs::read_to_string(path)?
        .lines()
        .map(|line| -> Result<_> {
            let value = source_observation(line, alias)?;
            Ok((
                value["vi_sample"].as_u64().context("missing VI index")?,
                value,
            ))
        })
        .collect::<Result<_>>()?;
    for frame in &pair.frames {
        let source = memory
            .get(&u64::from(frame.dolphin_vi))
            .with_context(|| format!("missing registered VI {}", frame.dolphin_vi))?;
        verify_frame_observations(frame, source)?;
    }
    Ok(())
}

fn verify_frame_observations(frame: &Frame, source: &Value) -> Result<()> {
    let require = |key: &str| -> Result<()> {
        ensure!(
            source[key].as_u64().is_some(),
            "frame {} at VI {} requires source observation {key}",
            frame.name,
            frame.dolphin_vi
        );
        Ok(())
    };
    require("presentation_counter")?;
    if frame.effect_state {
        require("random_state")?;
    }
    if frame.tech_state {
        require("field_save_point_word")?;
    }
    if frame.main_menu_state {
        for key in [
            "main_menu_fade_word",
            "party_menu_first_word",
            "party_menu_display_word",
        ] {
            require(key)?;
        }
    }
    if frame.manual_state {
        for key in [
            "manual_mode_chapter_word",
            "manual_topic_paragraph_word",
            "manual_fade_word",
            "ui_clock",
        ] {
            require(key)?;
        }
    }
    if frame.ex_state {
        for key in [
            "ui_clock",
            "ex_mode_slot_word",
            "ex_skill_gem_word",
            "ex_compound_scroll_word",
            "ex_scroll_character_word",
            "ex_gem_inventory_word",
            "ex_max_inventory_word",
            "ex_compound_count_word",
        ] {
            require(key)?;
        }
        if matches!(source["ex_mode_slot_word"].as_u64().unwrap() >> 16, 5 | 7) {
            require("ex_counts_confirm_word")?;
        }
        for character in 0..9 {
            for suffix in ["gems", "skills", "compounds", "recent_compounds"] {
                require(&format!("ex_character_{character}_{suffix}_word"))?;
            }
        }
        let count = source["ex_compound_count_word"].as_u64().unwrap() >> 16;
        ensure!(count <= 24, "invalid EX compound list size");
        for index in 0..count.div_ceil(2) {
            require(&format!("ex_compound_ids_{index}_word"))?;
        }
    }
    for (enabled, prefix) in [
        (frame.figurine_state, "figurine"),
        (frame.monster_state, "monster"),
    ] {
        if enabled {
            require("ui_clock")?;
            for suffix in [
                "fade_word",
                "opacity_word",
                "yaw_bits",
                "distance_bits",
                "animation_time_bits",
                "animation_end_bits",
                "animation_rate_bits",
            ] {
                require(&format!("{prefix}_{suffix}"))?;
            }
        }
    }
    if frame.figurine_state {
        for key in [
            "figurine_mode_row_word",
            "figurine_selection_scroll_word",
            "figurine_scroll_count_word",
            "figurine_model_load_word",
        ] {
            require(key)?;
        }
    }
    if frame.monster_state {
        for key in [
            "monster_mode_count_word",
            "monster_selection_variant_word",
            "monster_list_scroll_word",
        ] {
            require(key)?;
        }
        if source["monster_mode_count_word"].as_u64().unwrap() >> 16 == 1 {
            require("monster_list_selection_word")?;
        }
    }
    if frame.save_prompt || frame.action_prompt || frame.skit_prompt {
        require("field_control_flags_word")?;
        require("scene_flags_word")?;
        let suppressed = source["field_control_flags_word"].as_u64().unwrap() >> 24 != 0;
        for (enabled, prefix, id) in [
            (
                frame.save_prompt || frame.action_prompt,
                "action_prompt",
                "action_prompt",
            ),
            (frame.skit_prompt, "skit_prompt", "skit_id"),
        ] {
            if enabled {
                let alpha = format!("{prefix}_alpha");
                require(&alpha)?;
                require(&format!("{prefix}_remaining"))?;
                if (frame.save_prompt && prefix == "action_prompt")
                    || (!suppressed && source[&alpha].as_u64().unwrap() != 0)
                {
                    require(id)?;
                }
            }
        }
    }
    for (enabled, prefix, last) in [
        (
            frame.tech_state || frame.tech_navigation_state,
            "tech",
            0x20,
        ),
        (frame.equipment_state, "equipment", 0x14),
        (frame.customize_state, "customize", 0x4c),
    ] {
        if enabled {
            require("ui_clock")?;
            for offset in (0..=last).step_by(4) {
                require(&format!("{prefix}_menu_{offset:02x}_word"))?;
            }
        }
    }
    if frame.inventory_state {
        require("ui_clock")?;
        for offset in [0x00, 0x10, 0x14, 0x18, 0x1c, 0x24, 0x28, 0x30] {
            require(&format!("inventory_menu_{offset:02x}_word"))?;
        }
        if (6..=8).contains(&(source["inventory_menu_14_word"].as_u64().unwrap() & 65535)) {
            require("inventory_menu_0c_word")?;
        }
    }
    if frame.inventory_state || frame.equipment_state {
        observed_inventory(source)?;
    }
    if frame.shop_state {
        ShopState::observe(source).with_context(|| {
            format!(
                "frame {} at VI {} requires Shop observations",
                frame.name, frame.dolphin_vi
            )
        })?;
    }
    if frame.inventory_state || frame.equipment_state || frame.tech_state {
        for character in 0..9 {
            observed_equipment(source, character)?;
            if frame.inventory_state || frame.tech_state {
                for suffix in ["vitals", "conditions"] {
                    require(&format!("tech_character_{character}_{suffix}_word"))?;
                }
            }
            if frame.tech_state {
                require(&format!("ex_character_{character}_skills_word"))?;
                for suffix in [
                    "shortcuts_0",
                    "shortcuts_1",
                    "assists",
                    "assist_owners",
                    "known_hi",
                    "known_lo",
                    "enabled_hi",
                    "enabled_lo",
                ] {
                    require(&format!("tech_character_{character}_{suffix}_word"))?;
                }
            }
        }
    }
    if frame.tech_state || frame.tech_navigation_state {
        require("party_control_types_word")?;
        let controls = source["party_control_types_word"].as_u64().unwrap();
        let character = (source["tech_menu_0c_word"].as_u64().unwrap() >> 8) & 255;
        let owner = if source["tech_menu_00_word"].as_u64().unwrap() >> 16 >= 13 {
            source["tech_menu_04_word"].as_u64().unwrap() & 65535
        } else {
            character
        };
        ensure!(owner < 8, "invalid Tech list owner {owner}");
        let counts = format!("tech_counts_{}_word", owner / 2);
        require(&counts)?;
        let count = (source[&counts].as_u64().unwrap() >> (16 - owner % 2 * 16)) & 65535;
        ensure!(count <= 36, "invalid Tech list length {count}");
        for index in 0..count.div_ceil(2) {
            require(&format!("tech_{owner}_choices_{index}_word"))?;
        }
        if character < 4 && (controls >> (24 - character * 8)) & 255 == 2 {
            require(&format!("controller_{character}_status_word"))?;
        }
    }
    for (actors, suffixes) in [
        (
            &frame.eye_actors,
            &["eye_mode_word", "eye_mouth_mode_word"][..],
        ),
        (
            &frame.ambient_actors,
            &[
                "behavior_word",
                "decision_timer",
                "floor_attributes",
                "x_bits",
                "y_bits",
                "z_bits",
                "heading_bits",
            ][..],
        ),
    ] {
        for actor in actors {
            observed_actor_id(source, *actor)?;
            for suffix in suffixes {
                require(&format!("actor_{actor}_{suffix}"))?;
            }
        }
    }
    Ok(())
}

fn verify_capture(pair: &Pair, capture: &Value, input_hash: &str) -> Result<()> {
    ensure!(
        capture["complete"] == true
            && capture["audio"]["muted"] == true
            && capture["audio"]["backend"] == "No Audio Output"
            && capture["timing"]["diagnostic_override"] == false
            && capture["memory_watch"]["complete"] == true
            && capture["memory_watch"]["game_memory_modified"] == false
            && capture["memory_watch"]["vi_samples"]
                .as_u64()
                .is_some_and(|n| pair.frames.iter().all(|f| u64::from(f.dolphin_vi) < n)),
        "Dolphin did not meet recording invariants"
    );
    ensure!(
        capture["movie_sha256"] == input_hash
            && capture["disc_sha256"] == pair.disc_sha256
            && capture["initial_state_sha256"] == pair.dolphin.state.sha256,
        "Dolphin reference input/disc/state differs from the paired fixture"
    );
    ensure!(
        capture["dolphin_version"] == pair.dolphin.version
            && capture["dolphin_binary_sha256"] == pair.dolphin.binary_sha256
            && capture["configs"] == serde_json::to_value(&pair.dolphin.configs)?
            && capture["game_settings"] == serde_json::to_value(&pair.dolphin.game_settings)?,
        "Dolphin build/profile differs from the paired fixture"
    );
    Ok(())
}
fn observed_word(source: &Value, key: &str) -> Result<u64> {
    source[key]
        .as_u64()
        .with_context(|| format!("missing {key}"))
}

fn observed_actor_id(source: &Value, actor: i32) -> Result<i64> {
    let key = format!("actor_{actor}_id");
    source[&key]
        .as_i64()
        .with_context(|| format!("missing actor identity observation {key}"))
}

fn matching_fields(actual: &Value, expected: &Value) -> bool {
    expected
        .as_object()
        .unwrap()
        .iter()
        .all(|(key, value)| actual[key] == *value)
}

// JSON writers differ on integer-valued floats (0 versus 0.0).
fn same_values(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_values(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, a)| b.get(k).is_some_and(|b| same_values(a, b)))
        }
        _ => a == b,
    }
}

fn ambient_activity(state: u32) -> Result<&'static str> {
    Ok(match state & 0x3fff {
        0 => "select",
        1 => "idle",
        2 => "walk",
        _ => anyhow::bail!("source actor is not running an ambient activity"),
    })
}

fn observed_blink(sequence: &Value, mode: u32, timer: u32) -> Result<Value> {
    let rows = sequence
        .as_array()
        .context("missing source blink sequence")?;
    let index = (mode >> 16 & 255) as usize;
    ensure!(
        mode >> 24 == 129 && (mode >> 8 & 255) != 255 && index < rows.len(),
        "source actor is not running the blink sequence"
    );
    let lengths = rows
        .iter()
        .map(|row| row["ticks"].as_u64().context("missing blink duration"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        u64::from(timer) < lengths[index],
        "source blink timer exceeds its frame"
    );
    let tick = lengths[..index].iter().sum::<u64>() + u64::from(timer);
    let frame = &rows[if timer == 0 {
        (index + rows.len() - 1) % rows.len()
    } else {
        index
    }]["frame"];
    Ok(json!({"tick":tick,"frame":frame}))
}
fn synopsis_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| -> Result<u32> {
        Ok(observed_word(source, key)
            .with_context(|| format!("missing Synopsis observation {key}"))?
            .try_into()?)
    };
    let frequency = u64::from(word("synopsis_bus_clock")? / 4);
    ensure!(frequency != 0, "invalid source calendar clock");
    let mut records = BTreeMap::new();
    for id in 0..200 {
        let [value, extra, level, _] = word(&format!("synopsis_{id}_record_word"))?.to_be_bytes();
        if value != 0 {
            let ticks = u64::from(word(&format!("synopsis_{id}_time_hi_word"))?) << 32
                | u64::from(word(&format!("synopsis_{id}_time_lo_word"))?);
            records.insert(
                id.to_string(),
                json!({"value":value,"extra":extra,"level":level,
                "recorded_at":946_684_800 + ticks / frequency}),
            );
        }
    }
    let menu = &native["menu"];
    let actual_records: BTreeMap<_, _> = menu["synopsis_records"]
        .as_object()
        .context("missing native Synopsis records")?
        .iter()
        .filter(|(_, r)| r["value"] != 0)
        .map(|(id, r)| {
            (
                id,
                json!({"value":r["value"],"extra":r["extra"],
            "level":r["level"],"recorded_at":r["recorded_at"]}),
            )
        })
        .collect();
    let position = word("synopsis_row_first_word")?;
    let mode = word("synopsis_reading_count_word")?;
    let count = (mode & 65535) as usize;
    ensure!(
        count <= 200 && mode >> 16 <= 1,
        "invalid source Synopsis navigation"
    );
    let ids = (0..count.div_ceil(2))
        .map(|i| word(&format!("synopsis_ids_{i}_word")))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flat_map(|w| [w >> 16, w & 65535])
        .take(count)
        .collect::<Vec<_>>();
    let motion = word("synopsis_scroll_word")?;
    let observed_scroll = |key: &str| -> Result<i64> {
        let pose = menu["synopsis"][key]
            .as_i64()
            .context("missing Synopsis scroll pose")?;
        Ok((pose + pose.signum()) % 5)
    };
    let expected = json!({"row":position >> 16,"first":position & 65535,"reading":mode >> 16 == 1,
        "ui_clock":source["ui_clock"],
        "list_scroll":((motion >> 16) as u16) as i16,"text_scroll":(motion as u16) as i16,
        "text_opacity":(word("synopsis_fade_word")? >> 8) & 255,
        "page_fade":word("synopsis_fade_word")? >> 24,
        "ids":ids,"records":records});
    let actual = json!({"row":menu["synopsis"]["row"],"first":menu["synopsis"]["first"],
        "ui_clock":native["presentation_counter"],
        "list_scroll":observed_scroll("list_scroll")?,"text_scroll":observed_scroll("text_scroll")?,
        "text_opacity":menu["synopsis"]["text_opacity"],
        "page_fade":menu["synopsis"]["page_fade"],
        "reading":menu["synopsis"]["reading"],"ids":menu["synopsis_ids"],"records":actual_records});
    Ok(
        json!({"expected":expected,"actual":actual,"passed":menu["page"] == "Synopsis" && actual == expected}),
    )
}

fn strategy_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| -> Result<u32> {
        Ok(observed_word(source, key)
            .with_context(|| format!("missing Strategy observation {key}"))?
            .try_into()?)
    };
    let triples = |prefix: &str| -> Result<Vec<[u8; 3]>> {
        (0..9)
            .map(|i| {
                Ok(word(&format!("{prefix}_character_{i}_word"))?.to_be_bytes()[..3].try_into()?)
            })
            .collect()
    };
    let presets = (0..3)
        .map(|i| -> Result<_> {
            let bytes: Vec<_> = (0..2)
                .map(|j| word(&format!("strategy_preset_{i}_name_{j}_word")))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .take_while(|b| *b != 0)
                .collect();
            Ok(json!({"name":String::from_utf8(bytes)?,
                "members":triples(&format!("strategy_preset_{i}"))?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let fg = word("strategy_focus_group_word")?;
    let op = word("strategy_option_preset_word")?;
    let focus = [
        "Character",
        "Setting",
        "Options",
        "Presets",
        "Rename",
        "PresetCharacter",
        "PresetSetting",
        "PresetOptions",
    ]
    .get((fg >> 16) as usize)
    .context("unknown Strategy focus")?;
    let menu = &native["menu"];
    let state = &menu["strategy"];
    let mut expected = json!({"focus":focus,"first":word("strategy_first_scroll_word")? >> 16,
        "character":(word("strategy_character_word")? >> 16) & 255});
    let mut actual =
        json!({"focus":state["focus"],"first":state["first"],"character":state["character"]});
    expected["page_fade"] = json!(word("strategy_page_transition_word")? >> 24);
    actual["page_fade"] = state["page_fade"].clone();
    expected["preset_opacity"] = json!((word("strategy_page_transition_word")? >> 8) & 255);
    actual["preset_opacity"] = state["preset_opacity"].clone();
    expected["rename_opacity"] = json!(word("strategy_rename_opacity_word")? >> 24);
    actual["rename_opacity"] = state["rename_opacity"].clone();
    expected["ui_clock"] = source["ui_clock"].clone();
    actual["ui_clock"] = native["presentation_counter"].clone();
    if *focus == "Rename" || state["rename_opacity"].as_u64().unwrap_or(0) != 0 {
        let cell = word("strategy_rename_cell_word")?;
        let bytes: Vec<_> = [
            word("strategy_rename_text_first_word")?,
            word("strategy_rename_text_last_word")?,
        ]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .take_while(|b| *b != 0)
        .collect();
        expected["rename"] = json!({"column":cell >> 16,"row":cell & 65535,
            "position":word("strategy_rename_position_word")? >> 16,
            "value":String::from_utf8(bytes)?});
        actual["rename"] = state["rename"].clone();
    }
    let scroll = state["scroll"]
        .as_i64()
        .context("missing Strategy scroll pose")?;
    // The source advances its row-scroll pose after drawing.
    expected["scroll"] = json!((word("strategy_first_scroll_word")? as u16) as i16);
    actual["scroll"] = json!((scroll + scroll.signum()) % 10);
    let previous = word("strategy_description_previous_word")?;
    let previous = if previous == 0 {
        None
    } else {
        let mut row = None;
        for (group, count) in [9, 9, 7].into_iter().enumerate() {
            let base = word(&format!("strategy_description_group_{group}_word"))?;
            if previous >= base && previous < base + count * 16 && (previous - base) % 16 == 0 {
                row = Some([group, ((previous - base) / 16) as usize]);
            }
        }
        Some(row.context("unknown Strategy description")?)
    };
    expected["description_previous"] = json!(previous);
    actual["description_previous"] = state["description_previous"].clone();
    expected["description_fade"] = json!(word("strategy_description_fade_word")? >> 24);
    actual["description_fade"] = state["description_fade"].clone();
    if matches!(
        *focus,
        "Setting" | "Options" | "PresetSetting" | "PresetOptions"
    ) {
        expected["group"] = json!(fg & 65535);
        actual["group"] = state["group"].clone();
    }
    if matches!(*focus, "Options" | "PresetOptions") {
        expected["option"] = json!(op >> 16);
        actual["option"] = state["option"].clone();
    }
    if matches!(
        *focus,
        "Presets" | "Rename" | "PresetCharacter" | "PresetSetting" | "PresetOptions"
    ) {
        expected["preset"] = json!(op & 65535);
        actual["preset"] = state["preset"].clone();
    }
    let current = triples("strategy")?;
    let actual_current: Vec<_> = menu["party"]["members"]
        .as_array()
        .context("missing Strategy party")?
        .iter()
        .map(|m| m["strategy"].clone())
        .collect();
    Ok(
        json!({"passed":menu["page"] == "Strategy" && actual == expected
            && json!(current) == json!(actual_current) && json!(presets) == menu["strategy_presets"],
        "navigation":{"expected":expected,"actual":actual},
        "current":{"expected":current,"actual":actual_current},
        "presets":{"expected":presets,"actual":menu["strategy_presets"]}}),
    )
}

fn party_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key| {
        observed_word(source, key).with_context(|| format!("missing party observation {key}"))
    };
    let formation: Vec<_> = [
        word("party_formation_first_word")?,
        word("party_formation_last_word")?,
    ]
    .into_iter()
    .flat_map(|v| (v as u32).to_be_bytes())
    .filter(|&id| id != 0)
    .collect();
    let leaders = word("party_leaders_word")?;
    let expected = json!({
        "formation":formation,
        "field_leader":((leaders >> 8) & 255) + 1,
        "leader_locked":word("party_restrictions_word")? & 0x0008_0000 != 0
    });
    let controlled = ((leaders >> 16) & 255) + 1;
    Ok(json!({"expected":expected,"actual":native["party"],
        "expected_controlled_actor":controlled,"actual_controlled_actor":native["controlled_actor"],
        "passed":native["party"] == expected && native["controlled_actor"] == controlled}))
}

fn statistics_state(native: &Value, source: &Value) -> Result<Value> {
    let status = matches!(native["menu"]["page"].as_str(), Some("Status" | "Titles"));
    let word = if status {
        "status_menu_page_word"
    } else {
        "party_menu_display_word"
    };
    let expected =
        (observed_word(source, word).context("missing statistics display observation")? >> 8) & 255
            != 0;
    let actual = &native["menu"]["statistics"][if status { "status" } else { "party" }];
    Ok(json!({"expected":expected,"actual":actual,"passed":actual == expected}))
}

fn status_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let selection = word("status_menu_00_word")?;
    let transition = word("status_menu_50_word")?;
    let portrait = word("status_menu_60_word")?;
    let previous = (portrait >> 16) as u16 as i16;
    let expected = json!({
        "details":word("status_menu_page_word")? & 0xff00 != 0,
        "title_focus":selection & 255 != 0,
        "row":word("status_menu_08_word")? >> 24,
        "page_fade":transition >> 24,
        "closing":matches!((transition >> 16) & 255, 2 | 4),
        "previous":(previous >= 0).then_some(previous),
        "portrait_fade":(portrait >> 8) & 255,
        "title_opacity":(transition >> 8) & 255,
        "title_closing":transition & 255 == 6
    });
    let actual = &native["menu"]["status"];
    let page = if selection & 0xff00 == 0 {
        "Status"
    } else {
        "Titles"
    };
    let passed = actual == &expected
        && native["menu"]["page"] == page
        && native["menu"]["character"] == (selection >> 16) & 255
        && native["presentation_counter"] == source["ui_clock"];
    Ok(json!({"expected":expected,"actual":actual,"passed":passed}))
}

fn audio_settings(source: &Value) -> Result<Value> {
    let committed = u32::try_from(
        observed_word(source, "preferences_audio_word")
            .context("missing source committed audio preferences")?,
    )?
    .to_be_bytes();
    let voice = u32::try_from(
        observed_word(source, "preferences_voice_word")
            .context("missing source committed voice preferences")?,
    )? >> 24;
    Ok(json!({"stereo":committed[1] & 2 != 0,
        "levels":[committed[2], committed[3], if committed[1] & 64 != 0 { voice } else { 0 }]}))
}

fn customize_state(native: &Value, source: &Value) -> Result<Value> {
    let mut bytes = Vec::with_capacity(0x50);
    for offset in (0..0x50).step_by(4) {
        let key = format!("customize_menu_{offset:02x}_word");
        bytes.extend(u32::try_from(observed_word(source, &key)?)?.to_be_bytes());
    }
    let signed = |at| i16::from_be_bytes([bytes[at], bytes[at + 1]]);
    let enabled = |mask| bytes[0x21] & mask != 0;
    let focus = ["Options", "Colors", "Volume", "Position", "Controls"]
        .get(usize::from(bytes[2]))
        .context("unknown source Customize panel")?;
    let mut audio = audio_settings(source)?;
    audio["levels"][0] = json!(bytes[0x22]);
    let expected = json!({
        "row":bytes[0], "defaults":bytes[1] != 0, "focus":focus,
        "component":bytes[3], "color_group":bytes[4], "channel":bytes[5],
        "button":bytes[7], "first":signed(8),
        "scroll":signed(0x0a), "color_scroll":bytes[6] as i8,
        "preview_wait":bytes[0x0c], "preview_shown":bytes[0x0d],
        "page_fade":bytes[0x18], "page_closing":bytes[0x19] == 4,
        "audio":audio,
        "draft":{
            "message_speed":bytes[0x20], "battle_rank":bytes[0x2f],
            "window":bytes[0x27] >> 4 & 3, "background":bytes[0x27] & 15,
            "battle_voiceover":enabled(128), "event_voiceover":enabled(64),
            "skit_notifications":enabled(32), "movie_subtitles":enabled(16),
            "battle_auto_zoom":enabled(8), "rumble":enabled(4), "stereo":enabled(2),
            "button_map":bytes[0x28..0x2f], "screen_position":[signed(0x4c), signed(0x4e)],
            "volumes":{"music":bytes[0x22], "effects":bytes[0x23], "voice":bytes[0x24],
                "battle_effects":bytes[0x25], "battle_voice":bytes[0x26]},
            "colors":{"menu":bytes[0x30..0x34], "dialogue":bytes[0x34..0x38],
                "choice":bytes[0x38..0x3c], "popup":bytes[0x3c..0x40],
                "shade_top":bytes[0x40..0x44], "shade_bottom":bytes[0x44..0x48],
                "selection":bytes[0x48..0x4c]}
        }
    });
    let mut actual = native["menu"]["customize"].clone();
    for (key, steps) in [("scroll", 5), ("color_scroll", 20)] {
        let value = actual[key]
            .as_i64()
            .with_context(|| format!("missing Customize {key}"))?;
        actual[key] = json!((value + value.signum()) % steps);
    }
    actual["page_closing"] = json!(actual["page_closing"] == true && actual["page_fade"] != 255);
    actual["audio"] = native["audio_settings"].clone();
    let passed = native["menu"]["page"] == "Customize" && same_values(&actual, &expected);
    Ok(json!({"expected":expected,"actual":actual,"passed":passed}))
}

fn rename_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| -> Result<u32> { Ok(observed_word(source, key)?.try_into()?) };
    let string = |keys: Vec<String>| -> Result<String> {
        let bytes = keys
            .iter()
            .map(|key| word(key))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .take_while(|b| *b != 0)
            .collect();
        Ok(String::from_utf8(bytes)?)
    };
    let names = (0..9)
        .map(|i| {
            string(
                (0..4)
                    .map(|w| format!("character_{i}_name_{w}_word"))
                    .collect(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    // The fully transparent handoff frame has no interactive editor.
    let active = (word("status_menu_5c_word")? & 255 != 0
        || word("inventory_menu_1c_word")? >> 16 == 1006)
        && word("rename_menu_1c_word")? >> 24 != 255;
    let mut expected =
        json!({"active":active,"names":names,"gems":word("ex_max_inventory_word")? & 255});
    let menu = &native["menu"];
    let mut actual = json!({
        "active":menu["page"] == "Rename"
            && !(menu["rename"]["closing"] == true && menu["rename"]["fade"] == 255),
        "names":menu["names"],"gems":menu["rename_gems"]});
    if active {
        let selection = word("rename_menu_00_word")?;
        let keyboard = word("rename_menu_04_word")?;
        let command = word("rename_menu_08_word")?;
        let transition = word("rename_menu_1c_word")?;
        expected["editor"] = json!({"focus":match selection >> 16 {0=>"Name",1=>"Keyboard",2=>"Commands",_=>anyhow::bail!("unknown name editor focus")},
            "position":selection & 65535,"column":keyboard >> 16,"row":keyboard & 65535,
            "character":((command >> 8) & 255).checked_sub(1).context("missing name target")?,
            "fade":transition >> 24,"closing":(transition >> 16) & 255 == 4,
            "value":string([0xc,0x10,0x14,0x18].map(|at|format!("rename_menu_{at:02x}_word")).into())?});
        // The source retains an unused command index across editor openings.
        if selection >> 16 == 2 {
            expected["editor"]["command"] = (command >> 16).into();
        }
        actual["editor"] = expected["editor"]
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), menu["rename"][key].clone()))
            .collect();
    }
    Ok(json!({"passed":expected == actual,"expected":expected,"actual":actual}))
}

fn collection_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let selection = word("collection_selection_word")?;
    let mode = word("collection_mode_word")?;
    let mut expected = json!({"category":selection >> 24,"row":selection & 65535,
        "first":word("collection_scroll_word")? >> 16,"categories":mode & 65535 == 0});
    let page = word("collection_fade_word")?;
    expected["page_fade"] = json!(page >> 24);
    expected["page_closing"] = json!((page >> 16) & 255 == 4);
    let scroll = word("collection_scroll_word")? as u16 as i16;
    // Source observations follow drawing, which advances the scroll pose.
    let native_scroll = native["menu"]["collection"]["scroll"].as_i64().unwrap_or(0);
    let description = word("collection_description_word")?;
    let previous = (description >> 16) as u16 as i16;
    expected["description_previous"] = match previous {
        0 => json!("None"),
        ..0 => json!({"Category":-previous - 1}),
        _ => json!({"Item":previous}),
    };
    expected["description_fade"] = json!((description >> 8) & 255);
    let actual = &native["menu"]["collection"];
    let passed = matching_fields(actual, &expected)
        && i64::from(scroll) == (native_scroll + native_scroll.signum()) % 5
        && native["presentation_counter"] == source["ui_clock"]
        && native["menu"]["page"] == "Collection";
    Ok(
        json!({"expected":expected,"actual":actual,"count":mode >> 16,
        "scroll":{"expected":scroll,"actual":(native_scroll + native_scroll.signum()) % 5},
        "passed":passed}),
    )
}

fn world_map_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode = word("world_map_mode_word")?;
    let selection = word("world_map_selection_word")?;
    let scroll = word("world_map_scroll_word")?;
    let fade = word("world_map_fade_word")?;
    let description = word("world_map_description_word")?;
    let expected = json!({"world":(mode >> 8) & 255,
        "focus": match mode >> 16 { 0 => "locations", 1 => "shops", 2 => "items", _ => "invalid" },
        "location": selection >> 16, "shop": selection & 65535,
        "item": scroll >> 16, "first_location": scroll & 65535,
        "first_item": word("world_map_item_scroll_word")? & 65535,
        "page_fade":fade >> 24,"page_closing":(fade >> 16) & 255 == 4,
        "shops_opacity":(fade >> 8) & 255,"items_opacity":fade & 255,
        "description_previous":description >> 16,"description_fade":(description >> 8) & 255});
    let actual = &native["menu"]["world_map"];
    let mut passed = native["menu"]["page"] == "WorldMap"
        && native["presentation_counter"] == source["ui_clock"]
        && matching_fields(actual, &expected);
    let mut scrolls = json!({});
    for (key, word) in [
        ("location_scroll", word("world_map_item_scroll_word")?),
        ("item_scroll", word("world_map_count_word")?),
    ] {
        let expected = (word >> 16) as u16 as i16;
        let actual = actual[key].as_i64().context("missing map scroll")?;
        passed &= i64::from(expected) == (actual + actual.signum()) % 5;
        scrolls[key] = json!({"expected":expected,"actual":actual});
    }
    Ok(
        json!({"expected":expected,"actual":actual,"count":word("world_map_count_word")? & 65535,
        "scroll":scrolls,"passed":passed}),
    )
}

fn manual_state(native: &Value, source: &Value) -> Result<Value> {
    let mode = observed_word(source, "manual_mode_chapter_word")
        .context("missing manual chapter state")?;
    let topic = observed_word(source, "manual_topic_paragraph_word")
        .context("missing manual topic state")?;
    let fade = observed_word(source, "manual_fade_word").context("missing manual fade state")?;
    let expected = json!({"reading":mode >> 16 == 1,"chapter":mode & 65535,
        "topic":topic >> 16,"paragraph":topic & 65535,
        "page_fade":fade >> 24,"page_closing":(fade >> 16) & 255 == 4});
    let actual = &native["menu"]["manual"];
    let expected_clock = observed_word(source, "ui_clock").context("missing UI clock")?;
    let actual_clock = native["presentation_counter"]
        .as_u64()
        .context("missing native presentation clock")?;
    Ok(json!({"expected":expected,"actual":actual,
            "clock":{"expected":expected_clock,"actual":actual_clock},
            "passed":actual == &expected && native["menu"]["page"] == "Manual"
                && actual_clock == expected_clock}))
}

fn monster_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode = word("monster_mode_count_word")?;
    let selection = word("monster_selection_variant_word")?;
    let listing = mode >> 16 == 1;
    let fade = word("monster_fade_word")?;
    let mut expected = json!({"listing":listing,"row":selection >> 24,
        "variant":(selection >> 16) & 255,
        "page_fade":fade >> 24,"page_closing":(fade >> 16) & 255 == 4,
        "model_opacity":(word("monster_opacity_word")? >> 16) & 255,
        "yaw":observed_float(source, "monster_yaw_bits")?,
        "distance":observed_float(source, "monster_distance_bits")?});
    if listing {
        expected["list_row"] = json!(word("monster_list_selection_word")? & 65535);
        expected["first"] = json!(word("monster_list_scroll_word")? >> 16);
    }
    let actual = &native["menu"]["monsters"];
    let animation = &native["menu"]["monster_animation"];
    let sample = observed_float(source, "monster_animation_time_bits")? * 2.;
    let duration = observed_float(source, "monster_animation_end_bits")? * 2.;
    let rate = observed_float(source, "monster_animation_rate_bits")? * 2.;
    let scroll_word = word("monster_list_scroll_word")?;
    let source_scroll = scroll_word as i16;
    let scroll = actual["scroll"]
        .as_i64()
        .context("missing monster scroll")?;
    let drawn_scroll = if listing {
        (scroll + scroll.signum()) % 5
    } else {
        scroll
    };
    let started = actual["model_started"] == true;
    let passed = native["menu"]["page"] == "Monsters"
        && native["presentation_counter"] == source["ui_clock"]
        && drawn_scroll == i64::from(source_scroll)
        && native["menu"]["party"]["monsters"]
            .as_object()
            .is_some_and(|m| m.len() as u64 == mode & 65535)
        && expected
            .as_object()
            .unwrap()
            .iter()
            .all(|(key, value)| match key.as_str() {
                "yaw" | "distance" => actual[key]
                    .as_f64()
                    .is_some_and(|v| (v - value.as_f64().unwrap()).abs() < 0.01),
                _ => actual[key] == *value,
            })
        && (!started
            || (animation["sample"]
                .as_f64()
                .is_some_and(|s| (s - sample).abs() < 0.01)
                && animation["duration"].as_f64() == Some(duration)
                && rate == 1.));
    Ok(
        json!({"expected":expected,"actual":actual,"count":mode & 65535,
        "animation":animation,"expected_sample":sample,"expected_duration":duration,
        "expected_rate":rate,"scroll":{"actual":drawn_scroll,"expected":source_scroll},
        "clock":{"actual":native["presentation_counter"],"expected":source["ui_clock"]},
        "passed":passed}),
    )
}

fn figurine_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode = word("figurine_mode_row_word")?;
    let selection = word("figurine_selection_scroll_word")?;
    let count = word("figurine_scroll_count_word")? & 65535;
    let fade = word("figurine_fade_word")?;
    let expected = json!({"row":mode & 65535,"first":selection & 65535,
        "page_fade":fade >> 24,"page_closing":(fade >> 16) & 255 == 4,
        "model_opacity":(word("figurine_opacity_word")? >> 16) & 255,
        "figurine":word("figurine_model_load_word")? >> 16,
        "sample":observed_float(source, "figurine_animation_time_bits")? * 2.,
        "duration":observed_float(source, "figurine_animation_end_bits")? * 2.,
        "yaw":observed_float(source, "figurine_yaw_bits")?,
        "distance":observed_float(source, "figurine_distance_bits")?});
    let menu = &native["menu"];
    let actual = json!({"row":menu["figurines"]["row"],"first":menu["figurines"]["first"],
        "page_fade":menu["figurines"]["page_fade"],"page_closing":menu["figurines"]["page_closing"],
        "model_opacity":menu["figurines"]["model_opacity"],
        "figurine":menu["figurine_selected"],
        "sample":menu["figurine_animation"]["sample"],
        "duration":menu["figurine_animation"]["duration"],
        "yaw":menu["figurine_animation"]["yaw"],
        "distance":menu["figurine_animation"]["distance"]});
    let rate = observed_float(source, "figurine_animation_rate_bits")? * 2.;
    let scroll = menu["figurines"]["scroll"]
        .as_i64()
        .context("missing figurine scroll")?;
    let source_scroll = (word("figurine_scroll_count_word")? >> 16) as i16;
    let drawn_scroll = (scroll + scroll.signum()) % 5;
    let started = menu["figurines"]["model_started"] == true;
    let passed = mode >> 16 == 0
        && menu["page"] == "Figurines"
        && menu["party"]["figurines"]
            .as_array()
            .is_some_and(|ids| ids.len() as u64 == count)
        && expected.as_object().unwrap().iter().all(|(key, value)| {
            if key == "page_closing" {
                actual[key] == *value
            } else if !started && matches!(key.as_str(), "sample" | "duration") {
                true
            } else {
                actual[key]
                    .as_f64()
                    .is_some_and(|v| (v - value.as_f64().unwrap()).abs() < 0.01)
            }
        })
        && native["presentation_counter"] == source["ui_clock"]
        && drawn_scroll == i64::from(source_scroll)
        && (!started || rate == 1.);
    Ok(
        json!({"expected":expected,"actual":actual,"count":count,"rate":rate,
        "scroll":{"actual":drawn_scroll,"expected":source_scroll},"passed":passed}),
    )
}

fn observed_equipment(source: &Value, character: usize) -> Result<Vec<u64>> {
    [0, 1, 2, 4, 5, 3]
        .into_iter()
        .map(|slot| {
            let key = format!("tech_character_{character}_equipment_{}_word", slot / 2);
            Ok((observed_word(source, &key)? >> (16 - slot % 2 * 16)) & 65535)
        })
        .collect()
}

fn observed_inventory(source: &Value) -> Result<Value> {
    let mut counts = BTreeMap::new();
    for index in 0..132 {
        let key = match index {
            10 => "ex_gem_inventory_word".into(),
            124 => "ex_max_inventory_word".into(),
            _ => format!("inventory_{index}_word"),
        };
        let packed = observed_word(source, &key)? as u32;
        for (byte, count) in packed.to_be_bytes().into_iter().enumerate() {
            let id = index * 4 + byte;
            if id != 0 && count != 0 {
                counts.insert(id.to_string(), count);
            }
        }
    }
    Ok(serde_json::to_value(counts)?)
}

fn poison_state(native: &Value, source: &Value, slots: &[u16]) -> Result<Value> {
    let mut expected = Vec::new();
    for slot in slots {
        let prefix = format!("particle_{slot}");
        let word = |suffix: &str| -> Result<u64> {
            observed_word(source, &format!("{prefix}_{suffix}"))
                .context("missing poison particle observation")
        };
        let timer = word("timer_flags")? >> 16;
        if timer == 65535 || word("recipe")? != 0x8020a4e4 {
            continue;
        }
        ensure!(
            timer <= 20 && word("rgba")? == 0x0d3f04ff,
            "unexpected poison particle state"
        );
        let size = observed_float(source, &format!("{prefix}_size_x_bits"))?;
        ensure!(
            size == observed_float(source, &format!("{prefix}_size_y_bits"))?,
            "non-square poison particle"
        );
        expected.push(json!({"age":20 - timer,"size":size,
            "speed_sixteenths":observed_float(source, &format!("{prefix}_fall_bits"))? * 16.,
            "position":[observed_float(source, &format!("{prefix}_x_bits"))?,
                observed_float(source, &format!("{prefix}_y_bits"))?,
                observed_float(source, &format!("{prefix}_z_bits"))?]}));
    }
    let actual = native["poison_puffs"]
        .as_array()
        .context("missing native poison particles")?;
    let passed = expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(e, a)| {
            ["age", "size", "speed_sixteenths"]
                .iter()
                .all(|key| same_values(&e[key], &a[key]))
                && (0..3).all(|axis| {
                    a["position"][axis]
                        .as_f64()
                        .is_some_and(|v| (v - e["position"][axis].as_f64().unwrap()).abs() <= 0.001)
                })
        });
    Ok(json!({"expected":expected,"actual":actual,"passed":passed}))
}

fn cooking_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| -> Result<u32> { Ok(observed_word(source, key)?.try_into()?) };
    let [full, recipe, chef, _] = word("cooking_settings_word")?.to_be_bytes();
    let party = if native["menu"].is_null() {
        // Older captures only recorded party progress when a restart was allowed.
        native
            .get("persistent_party")
            .unwrap_or(&native["checkpoint"]["progress"]["party"])
    } else {
        &native["menu"]["party"]
    };
    ensure!(
        party.is_object(),
        "cooking check needs persistent party state"
    );
    let expected =
        json!({"known":word("cooking_known_word")?,"full":full != 0,"recipe":recipe,"chef":chef});
    let items = observed_inventory(source)?;
    let mut passed = party["cooking"] == expected && party["items"] == items;
    let mut members = Vec::new();
    for character in 0..9 {
        let training = (0..6)
            .map(|index| {
                word(&format!(
                    "cooking_character_{character}_training_{index}_word"
                ))
                .map(u32::to_be_bytes)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let vitals = word(&format!("tech_character_{character}_vitals_word"))?;
        let expected = json!({"hp":vitals >> 16,"tp":vitals & 65535,
            "conditions":word(&format!("tech_character_{character}_conditions_word"))?,"training":training});
        let member = &party["members"][character];
        let actual = json!({"hp":member["hp"],"tp":member["tp"],
            "conditions":member["conditions"].as_u64().unwrap_or(0),"training":member["cooking"]});
        passed &= expected == actual;
        members.push(json!({"character":character,"expected":expected,"actual":actual}));
    }
    Ok(json!({"expected":expected,"actual":party["cooking"],
        "items":{"expected":items,"actual":party["items"]},"members":members,"passed":passed}))
}

fn inventory_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let selection = word("inventory_menu_14_word")?;
    let mode = selection & 65535;
    let focus = match mode {
        0 => "Categories".to_owned(),
        1 | 5 | 10 => "List".to_owned(),
        2..=4 => "Target".to_owned(),
        6..=8 => "Transform(22)".to_owned(),
        9 => format!("Discard({})", word("inventory_menu_18_word")? >> 16 == 0),
        _ => anyhow::bail!("unsupported Items observation mode {mode}"),
    };
    let menu = &native["menu"];
    let mut expected = json!({"category":word("inventory_menu_00_word")? >> 24,
        "row":selection >> 16,"first":word("inventory_menu_10_word")? >> 16,"focus":focus});
    expected["target_ticks"] = json!(word("inventory_menu_1c_word")? & 255);
    let transition = word("inventory_menu_28_word")?;
    expected["target_opacity"] = json!(transition >> 24);
    expected["target_closing"] = json!((transition >> 16) & 255 == 6);
    let page = word("inventory_menu_24_word")?;
    expected["page_fade"] = json!(page >> 24);
    expected["page_closing"] = json!((page >> 16) & 255 == 4);
    let description = word("inventory_menu_30_word")?;
    let previous = (description >> 16) as u16 as i16;
    expected["description_previous"] = match previous {
        0 => json!("None"),
        ..0 => json!({"Category":-previous - 1}),
        _ => json!({"Item":previous}),
    };
    expected["description_fade"] = json!((description >> 8) & 255);
    if (6..=8).contains(&mode) {
        let original = word("inventory_menu_0c_word")?;
        expected["transform_original_row"] = json!(original >> 16);
        expected["transform_original_first"] = json!(original & 65535);
    }
    if focus == "Target" {
        expected["target_all"] = json!(mode == 3);
        expected["target_equipment"] = json!(mode == 4);
    }
    let target = word("inventory_menu_18_word")? & 65535;
    let scroll = word("inventory_menu_10_word")? as u16 as i16;
    let native_scroll = menu["inventory"]["scroll"].as_i64().unwrap_or(0);
    let mut passed = menu["page"] == "Items"
        && i64::from(scroll) == (native_scroll + native_scroll.signum()) % 5
        && native["presentation_counter"] == source["ui_clock"]
        && menu["inventory"]["notice"].is_string() == matches!(mode, 5 | 7 | 8 | 10)
        && menu["inventory"]["transform"]["result"].is_number() == (mode == 8)
        && (focus != "Target" || menu["inventory"]["target"] == target)
        && expected.as_object().unwrap().iter().all(|(k, v)| {
            let actual = if let Some(key) = k.strip_prefix("transform_") {
                &menu["inventory"]["transform"][key]
            } else {
                &menu["inventory"][k]
            };
            actual == v
        });
    let counts = observed_inventory(source)?;
    passed &= menu["party"]["items"] == counts;
    let mut members = Vec::new();
    for character in 0..9 {
        let vitals = word(&format!("tech_character_{character}_vitals_word"))?;
        let expected = json!({"hp":vitals >> 16,"tp":vitals & 65535,
            "conditions":word(&format!("tech_character_{character}_conditions_word"))?,
            "equipment":observed_equipment(source,character)?});
        let member = &menu["party"]["members"][character];
        let actual = json!({"hp":member["hp"],"tp":member["tp"],"conditions":member["conditions"].as_u64().unwrap_or(0),
            "equipment":member["equipment"]});
        passed &= expected == actual;
        members.push(json!({"character":character,"expected":expected,"actual":actual}));
    }
    Ok(json!({"expected":expected,"actual":menu["inventory"],
        "scroll":{"expected":scroll,"actual":(native_scroll + native_scroll.signum()) % 5},
        "target":{"expected":target,"actual":menu["inventory"]["target"]},"members":members,
        "items":{"expected":counts,"actual":menu["party"]["items"]},"passed":passed}))
}

struct ShopState {
    expected: Value,
    compare_list: bool,
    compare_description: bool,
}

impl ShopState {
    fn observe(source: &Value) -> Result<Self> {
        let observed = |key: &str| -> Result<u32> {
            u32::try_from(observed_word(source, key)?)
                .with_context(|| format!("invalid Shop observation {key}"))
        };
        let word = |offset: u8| observed(&format!("shop_menu_{offset:02x}_word"));
        ensure!(
            observed("scene_flags_word")? >> 24 == 2,
            "source shop is not active"
        );
        let party = word(4)?;
        let navigation = word(16)?;
        let mode = navigation & 65535;
        let focus = match mode {
            0 => json!("root"),
            1 => json!("categories"),
            2 => json!("items"),
            3 => json!("characters"),
            4 => json!("equipment"),
            5 => {
                ensure!(navigation >> 16 <= 1, "invalid shop confirmation choice");
                json!({"confirm":{"yes":navigation >> 16 == 0}})
            }
            6 => json!("empty"),
            _ => anyhow::bail!("invalid shop focus {mode}"),
        };
        let selection = word(20)?;
        let choice = match selection >> 16 {
            0 => "buy",
            1 => "sell",
            2 => "equip",
            3 => "leave",
            other => anyhow::bail!("invalid shop choice {other}"),
        };
        ensure!(selection & 65535 < 52, "invalid shop ID");
        ensure!(
            matches!(party & 255, 0 | 2),
            "invalid shop equipment handoff"
        );
        let equipment = party & 255 == 2;
        let visited = [
            observed("visited_shops_first_word")?,
            observed("visited_shops_last_word")?,
        ];
        let visited: Vec<_> = (0..52)
            .filter(|id| visited[id / 32] & (1 << (id % 32)) != 0)
            .collect();
        let mut expected = json!({
            "id":selection & 65535,"choice":choice,"focus":focus,
            "fade":word(28)? >> 24,"total":word(24)?,"equipment":equipment,
            "gald":observed("party_gald_word")?,"spent_gald":observed("party_spent_gald_word")?,
            "visited":visited,"items":observed_inventory(source)?,"clock":observed("ui_clock")?
        });
        // Checkout leaves obsolete basket rows behind at Root. Equip uses the same buffer.
        let compare_list = mode != 0 && !equipment;
        if compare_list {
            let list = word(8)?;
            let count = list >> 16;
            ensure!(count <= 528, "invalid shop list length {count}");
            let rows = (0..count)
                .map(|index| {
                    let packed = observed(&format!("shop_basket_{}_word", index / 2))?;
                    let entry = ((packed >> (16 - index % 2 * 16)) & 65535) as u16;
                    let id = entry >> 6;
                    ensure!((1..528).contains(&id), "invalid shop basket item {id}");
                    Ok(json!({"id":id,"quantity":entry & 63}))
                })
                .collect::<Result<Vec<_>>>()?;
            let position = word(12)?;
            expected["rows"] = json!(rows);
            expected["row"] = json!(list & 65535);
            expected["first"] = json!(position >> 16);
            expected["scroll"] = json!(position as u16 as i16);
            if choice == "sell" {
                ensure!(party >> 24 < 7, "invalid shop category");
                expected["category"] = json!(party >> 24);
            }
            if matches!(mode, 3 | 4) {
                expected["character"] = json!((party >> 8) & 255);
            }
        }
        let compare_description = matches!(mode, 1..=4) && !equipment;
        if compare_description {
            let description = word(36)?;
            expected["description_previous"] = match (description >> 16) as i16 {
                0 => json!("None"),
                id @ 1..=527 => json!({"Item":id}),
                category @ -8..=-2 => json!({"Category":-category - 1}),
                other => anyhow::bail!("invalid shop description {other}"),
            };
            expected["description_fade"] = json!((description >> 8) & 255);
        }
        Ok(Self {
            expected,
            compare_list,
            compare_description,
        })
    }

    fn compare(self, native: &Value) -> Result<Value> {
        ensure!(native["shop"].is_object(), "missing native shop snapshot");
        let mut actual = native["shop"].clone();
        actual["equipment"] = json!(native["menu"]["page"] == "Equip");
        actual["clock"] = native["presentation_counter"].clone();
        if self.compare_list {
            let scroll = actual["scroll"].as_i64().context("missing Shop scroll")?;
            actual["scroll"] = json!((scroll + scroll.signum()) % 5);
        }
        if self.compare_description {
            let opacity = actual["description_opacity"]
                .as_u64()
                .context("missing Shop description opacity")?;
            ensure!(opacity <= 255, "invalid Shop description opacity");
            // The capture observes memory after drawing; the snapshot retains the drawn opacity.
            actual["description_fade"] = json!((255 - opacity).saturating_sub(16));
        }
        let passed = matching_fields(&actual, &self.expected);
        Ok(json!({"expected":self.expected,"actual":actual,"passed":passed}))
    }
}

fn shop_state(native: &Value, source: &Value) -> Result<Value> {
    ShopState::observe(source)?.compare(native)
}

fn equipment_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |offset: u8| {
        let key = format!("equipment_menu_{offset:02x}_word");
        observed_word(source, &key)
    };
    let owner = word(0)?;
    let selection = word(4)?;
    let mode = word(8)? >> 16;
    let position = word(12)?;
    let transition = word(16)?;
    let description = word(20)?;
    let focus = match mode {
        0 => json!("Character"),
        1 => json!({"Optimal":{"thrust":(owner >> 16) & 255 != 0}}),
        2 => json!("Slots"),
        3 => json!("List"),
        _ => anyhow::bail!("invalid Equip mode {mode}"),
    };
    let previous = description >> 16;
    let mut expected = json!({"focus":focus,"slot":selection & 65535,
        "by_parameter":owner >> 24 != 0,
        "page_fade":transition >> 24,"page_closing":(transition >> 16) & 255 == 4,
        "description_previous":(previous != 0).then_some(previous),
        "description_fade":(description >> 8) & 255});
    let menu = &native["menu"];
    let mut actual = menu["equipment"].clone();
    actual["page_closing"] = json!(actual["page_closing"] == true && actual["page_fade"] != 255);
    if matches!(mode, 2 | 3) {
        expected["first"] = json!(position >> 16);
        expected["scroll"] = json!(position as u16 as i16);
        let scroll = actual["scroll"].as_i64().context("missing Equip scroll")?;
        actual["scroll"] = json!((scroll + scroll.signum()) % 5);
    }
    if mode == 3 {
        expected["row"] = json!(selection >> 16);
    }
    let items = observed_inventory(source)?;
    let mut passed = menu["page"] == "Equip"
        && menu["character"] == (owner >> 8) & 255
        && menu["equipment_count"] == word(8)? & 65535
        && menu["party"]["items"] == items
        && native["presentation_counter"] == source["ui_clock"]
        && matching_fields(&actual, &expected);
    let mut equipment = Vec::new();
    for character in 0..9 {
        let expected = json!(observed_equipment(source, character)?);
        let actual = &menu["party"]["members"][character]["equipment"];
        passed &= *actual == expected;
        equipment.push(json!({"character":character,"expected":expected,"actual":actual}));
    }
    Ok(
        json!({"expected":expected,"actual":actual,"equipment":equipment,
        "inventory":{"expected":items,"actual":menu["party"]["items"]},
        "character":{"expected":(owner >> 8) & 255,"actual":menu["character"]},
        "count":{"expected":word(8)? & 65535,"actual":menu["equipment_count"]},"passed":passed}),
    )
}

fn tech_state(native: &Value, source: &Value, loadout: bool) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode_slot = word("tech_menu_00_word")?;
    let mode = mode_slot >> 16;
    let focus = match mode {
        0 => "Control",
        1 => "Character",
        2 | 5 => "Shortcuts",
        3 | 4 | 6 => "List",
        7 | 8 => "CannotForget",
        9 | 10 => "Forget",
        11 | 12 => "Target",
        13 => "AssistCharacter",
        14 | 15 => "AssistList",
        _ => anyhow::bail!("unsupported Tech observation mode {mode}"),
    };
    let row_target = word("tech_menu_04_word")?;
    let party = word("tech_menu_0c_word")?;
    let mut expected = json!({"focus":focus,"unison":matches!(mode,5|6),
        "row":row_target >> 16,"first":word("tech_menu_08_word")? >> 16,
        "target_ticks":party & 255});
    if matches!(mode, 9 | 10) {
        expected["focus"] = json!({"Forget":{"yes":word("tech_menu_10_word")? >> 24 == 0}});
    }
    if matches!(mode, 2 | 5 | 6) || mode >= 13 {
        expected["slot"] = json!(mode_slot & 65535);
    }
    if matches!(mode, 11 | 12) {
        expected["target"] = json!(row_target & 65535);
    }
    if mode >= 13 {
        expected["assist"] = json!(row_target & 65535);
    }
    let menu = &native["menu"];
    let mut actual = menu["tech"].clone();
    let transition = word("tech_menu_14_word")?;
    expected["page_fade"] = json!(transition >> 24);
    expected["page_closing"] = json!((transition >> 16) & 255 == 4);
    actual["page_closing"] = json!(actual["page_closing"] == true && actual["page_fade"] != 255);
    expected["scroll"] = json!(word("tech_menu_08_word")? as u16 as i16);
    let scroll = actual["scroll"].as_i64().context("missing Tech scroll")?;
    actual["scroll"] = json!((scroll + scroll.signum()) % 5);
    let description = word("tech_menu_1c_word")?;
    expected["description_previous"] = if description >> 16 == 0 {
        Value::Null
    } else {
        json!({"technique":description >> 16,"character":description & 65535})
    };
    expected["description_fade"] = json!(word("tech_menu_20_word")? >> 24);
    expected["cannot_forget_opacity"] = json!((word("tech_menu_18_word")? >> 8) & 255);
    expected["forget_opacity"] = json!(word("tech_menu_18_word")? & 255);
    if mode >= 13 || matches!(mode, 5 | 6) {
        expected["banner_opacity"] = json!(word("tech_menu_18_word")? >> 24);
    }
    let controls = word("party_control_types_word")?;
    let controls = [24, 16, 8, 0].map(|shift| (controls >> shift) & 255);
    let character = ((party >> 8) & 255) as usize;
    let list_owner = if mode >= 13 {
        (row_target & 65535) as usize
    } else {
        character
    };
    ensure!(list_owner < 8, "invalid Tech list owner {list_owner}");
    let counts = word(&format!("tech_counts_{}_word", list_owner / 2))?;
    let count = ((counts >> (16 - (list_owner % 2) * 16)) & 65535) as usize;
    ensure!(count <= 36, "invalid Tech list length {count}");
    let choices = (0..count)
        .map(|i| {
            word(&format!("tech_{list_owner}_choices_{}_word", i / 2))
                .map(|packed| (packed >> (16 - (i % 2) * 16)) & 65535)
        })
        .collect::<Result<Vec<_>>>()?;
    let unison_available = character < 4
        && controls[character] == 2
        && word(&format!("controller_{character}_status_word"))? & 0xff00 != 0;
    let navigation = menu["page"] == "Tech"
        && menu["character"] == (party >> 8) & 255
        && native["presentation_counter"] == source["ui_clock"]
        && matching_fields(&actual, &expected);
    if !loadout {
        return Ok(
            json!({"scope":"navigation", "mode":mode,"expected":expected,"actual":actual,
            "passed":navigation}),
        );
    }
    let at_save_point = (word("field_save_point_word")? >> 16) & 255 == 1;
    let mut passed = navigation
        && menu["at_save_point"] == at_save_point
        && menu["tech_choices"] == json!(choices)
        && menu["party"]["settings"]["battle_controls"] == json!(controls)
        && menu["tech_unison_available"] == unison_available
        && menu["party"]["formation"]
            .as_array()
            .is_some_and(|f| f.len() as u64 == party >> 24);
    let mut members = Vec::new();
    for character in 0..9 {
        let vitals = word(&format!("tech_character_{character}_vitals_word"))?;
        let skills = word(&format!("ex_character_{character}_skills_word"))?;
        let skills = [24, 16, 8, 0].map(|shift| (skills >> shift) & 255);
        let equipment = observed_equipment(source, character)?;
        let mut shortcuts = Vec::new();
        for index in 0..2 {
            let packed = word(&format!(
                "tech_character_{character}_shortcuts_{index}_word"
            ))?;
            shortcuts.extend([packed >> 16, packed & 65535]);
        }
        let assists = word(&format!("tech_character_{character}_assists_word"))?;
        let owners = word(&format!("tech_character_{character}_assist_owners_word"))?;
        let assists: Vec<_> = (0..2)
            .map(|i| {
                let owner = (owners >> (24 - i * 8)) & 255;
                let technique = (assists >> (16 - i * 16)) & 65535;
                if owner == 0 || technique == 0 {
                    Value::Null
                } else {
                    json!({"character":owner - 1,"technique":technique})
                }
            })
            .collect();
        let expected = json!({"hp":vitals >> 16,"tp":vitals & 65535,"ex_skills":skills,"equipment":equipment,
            "conditions":word(&format!("tech_character_{character}_conditions_word"))?,
            "shortcuts":shortcuts,"assists":assists});
        let member = &menu["party"]["members"][character];
        let actual = json!({"hp":member["hp"],"tp":member["tp"],"conditions":member["conditions"].as_u64().unwrap_or(0),
            "ex_skills":member.get("ex_skills").cloned().unwrap_or(json!([0,0,0,0])),"equipment":member["equipment"],
            "shortcuts":member["shortcuts"],"assists":(0..2).map(|i|member["assist_shortcuts"][i].clone()).collect::<Vec<_>>()});
        passed &= expected == actual;
        let bits = |name| -> Result<u64> {
            Ok(
                word(&format!("tech_character_{character}_{name}_hi_word"))? << 32
                    | word(&format!("tech_character_{character}_{name}_lo_word"))?,
            )
        };
        let known = bits("known")?;
        let flags = json!([known, bits("enabled")? & known]);
        passed &= flags == menu["tech_flags"][character];
        members.push(
            json!({"character":character,"expected":expected,"actual":actual,
            "expected_flags":flags,"actual_flags":menu["tech_flags"][character]}),
        );
    }
    Ok(
        json!({"mode":mode,"expected":expected,"actual":actual,"members":members,
        "at_save_point":{"expected":at_save_point,"actual":menu["at_save_point"]},
        "choices":{"expected":choices,"actual":menu["tech_choices"]},
        "controls":{"expected":controls,"actual":menu["party"]["settings"]["battle_controls"]},
        "unison_available":{"expected":unison_available,"actual":menu["tech_unison_available"]},"passed":passed}),
    )
}

fn unison_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode_slot = word("unison_mode_slot_word")?;
    let mode = mode_slot >> 16;
    ensure!(mode <= 1, "invalid Unison menu mode");
    let party = word("unison_scroll_party_word")?;
    let character = party & 255;
    let count = (party >> 8) & 255;
    ensure!(
        (1..=4).contains(&count) && character < count,
        "invalid Unison party selection"
    );
    let mut expected = json!({"focus":if mode==0 {"Slots"} else {"List"},
        "character":character,"slot":mode_slot & 65535});
    if mode == 1 {
        let selection = word("unison_row_first_word")?;
        expected["row"] = json!(selection >> 16);
        expected["first"] = json!(selection & 65535);
    }
    let menu = &native["menu"];
    let mut actual = menu["unison"].clone();
    let scroll = actual["scroll"].as_i64().context("missing Unison scroll")?;
    actual["scroll"] = json!((scroll + scroll.signum()) % 5);
    expected["scroll"] = json!((party >> 16) as u16 as i16);
    let mut passed = menu["page"] == "Unison"
        && native["presentation_counter"] == source["ui_clock"]
        && matching_fields(&actual, &expected);
    let mut members = Vec::new();
    for character in 0..9 {
        let mut shortcuts = Vec::new();
        for index in 0..2 {
            let packed = word(&format!(
                "tech_character_{character}_shortcuts_{index}_word"
            ))?;
            shortcuts.extend([packed >> 16, packed & 65535]);
        }
        let actual = &menu["party"]["members"][character]["shortcuts"];
        passed &= *actual == json!(shortcuts);
        members.push(json!({"character":character,"expected":shortcuts,"actual":actual}));
    }
    let counts = word(if character < 2 {
        "unison_counts_first_word"
    } else {
        "unison_counts_last_word"
    })?;
    let count = (counts >> if character % 2 == 0 { 16 } else { 0 }) & 65535;
    ensure!(count <= 36, "invalid Unison technique count");
    let choices = (0..count)
        .map(|i| {
            word(&format!("unison_{character}_choices_{}_word", i / 2))
                .map(|w| (w >> if i % 2 == 0 { 16 } else { 0 }) & 65535)
        })
        .collect::<Result<Vec<_>>>()?;
    passed &= menu["unison_choices"] == json!(choices);
    let description = word("unison_description_word")?;
    let selected = if description >> 16 == 0 {
        Value::Null
    } else {
        json!({"character":description & 65535,"technique":description >> 16})
    };
    passed &= menu["unison"]["description_previous"] == selected;
    Ok(
        json!({"expected":expected,"actual":actual,"members":members,
        "choices":{"expected":choices,"actual":menu["unison_choices"]},
        "description_previous":{"expected":selected,"actual":menu["unison"]["description_previous"]},"passed":passed}),
    )
}

fn ex_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| observed_word(source, key);
    let mode_slot = word("ex_mode_slot_word")?;
    let mode = mode_slot >> 16;
    let selection = word("ex_skill_gem_word")?;
    let scroll = word("ex_compound_scroll_word")?;
    let focus = match mode {
        0 => json!("Character"),
        1 => json!("Gems"),
        2 => json!("Skills"),
        3 => json!("GemList"),
        4 => json!("SkillList"),
        5 | 7 => json!({"Confirm": {
            "yes":word("ex_counts_confirm_word")? & 255 == 0,"replacing":mode == 5}}),
        6 => json!("Compounds"),
        _ => anyhow::bail!("unknown EX menu mode {mode}"),
    };
    let mut expected = json!({"focus":focus,"slot":mode_slot & 65535});
    match mode {
        3 | 5 | 7 => expected["gem"] = json!(selection & 65535),
        4 => expected["skill"] = json!(selection >> 16),
        6 => expected["compound"] = json!(scroll >> 16),
        _ => {}
    }
    if matches!(mode, 2..=5 | 7) {
        expected["first"] = json!(scroll & 65535);
    }
    let menu = &native["menu"];
    let actual = &menu["ex_skills"];
    let character = (word("ex_scroll_character_word")? >> 8) & 255;
    let mut passed = menu["page"] == "ExSkills"
        && menu["character"] == character
        && native["presentation_counter"] == source["ui_clock"]
        && matching_fields(actual, &expected);
    let unpack = |word: u64| [24, 16, 8, 0].map(|shift| (word >> shift) & 255);
    let counts = unpack(word("ex_gem_inventory_word")?);
    let mut inventory = Vec::new();
    for (id, count) in [40, 41, 42, 43, 496].into_iter().zip(
        counts
            .into_iter()
            .chain([word("ex_max_inventory_word")? >> 24]),
    ) {
        let actual = menu["party"]["items"][id.to_string()].as_u64().unwrap_or(0);
        passed &= actual == count;
        inventory.push(json!({"item":id,"expected":count,"actual":actual}));
    }
    let mut members = Vec::new();
    for character in 0..9 {
        let gems = unpack(word(&format!("ex_character_{character}_gems_word"))?);
        let skills = unpack(word(&format!("ex_character_{character}_skills_word"))?);
        let compounds = word(&format!("ex_character_{character}_compounds_word"))?;
        let recent = word(&format!("ex_character_{character}_recent_compounds_word"))?;
        let expected = json!({"ex_gems":gems,"ex_skills":skills,
            "compound_ex_skills":(0..24).filter(|i| compounds & (1 << i) != 0).collect::<Vec<_>>(),
            "recent_compound_ex_skills":(0..24).filter(|i| recent & (1 << i) != 0).collect::<Vec<_>>()});
        let member = &menu["party"]["members"][character];
        let actual = json!({"ex_gems":member["ex_gems"],"ex_skills":member["ex_skills"],
            "compound_ex_skills":member.get("compound_ex_skills").cloned().unwrap_or(json!([])),
            "recent_compound_ex_skills":member.get("recent_compound_ex_skills").cloned().unwrap_or(json!([]))});
        passed &= actual == expected;
        members.push(json!({"character":character,"expected":expected,"actual":actual}));
    }
    let count = word("ex_compound_count_word")? >> 16;
    ensure!(count <= 24, "invalid EX compound list size");
    let compounds = (0..count)
        .map(|i| {
            word(&format!("ex_compound_ids_{}_word", i / 2))
                .map(|word| (word >> if i % 2 == 0 { 16 } else { 0 }) & 65535)
        })
        .collect::<Result<Vec<_>>>()?;
    passed &= menu["ex_compounds"] == json!(compounds);
    let member = menu["party"]["formation"][character as usize]
        .as_u64()
        .context("missing selected EX character")?
        - 1;
    let stats_word = |name| word(&format!("ex_character_{member}_{name}_word"));
    let vitals = stats_word("vitals")?;
    let attack = stats_word("attack")?;
    let defense = stats_word("defense_luck")?;
    let accuracy = stats_word("accuracy_evasion")?;
    let stats = json!({"hp":vitals >> 16,"tp":vitals & 65535,
        "slash":attack >> 16,"thrust":attack & 65535,
        "defense":defense >> 16,"luck":defense & 65535,
        "accuracy":accuracy >> 16,"evasion":accuracy & 65535,
        "intelligence":stats_word("intelligence")? >> 16});
    passed &= matching_fields(&menu["ex_stats"], &stats);
    Ok(
        json!({"expected":expected,"actual":actual,"character":character,
        "inventory":inventory,"members":members,
        "compounds":{"expected":compounds,"actual":menu["ex_compounds"]},
        "stats":{"expected":stats,"actual":menu["ex_stats"]},"passed":passed}),
    )
}

fn observed_float(state: &Value, key: &str) -> Result<f64> {
    let bits = u32::try_from(
        observed_word(state, key).with_context(|| format!("missing observation {key}"))?,
    )?;
    let value = f32::from_bits(bits);
    ensure!(value.is_finite(), "nonfinite observation {key}");
    Ok(f64::from(value))
}

fn verify_action_prompt_origin(origin: &Value, source: &Value) -> Result<()> {
    let presentation = &source["field_presentation"];
    let scene = observed_word(presentation, "scene_flags")?;
    let control = observed_word(presentation, "control_flags")?;
    ensure!(
        scene >> 24 == 0 && scene & 0x7f == 7 && control >> 24 == 0,
        "action hint origin requires active source field control"
    );
    let id = observed_word(origin, "id")?;
    let opacity = observed_word(origin, "opacity")?;
    let remaining = observed_word(origin, "remaining")?;
    ensure!(
        (1..=u64::from(u8::MAX)).contains(&id)
            && (1..=255).contains(&opacity)
            && (1..30).contains(&remaining)
            && *origin == source["action_prompt"],
        "action hint origin differs from observed source state"
    );
    Ok(())
}

fn verify_camera_origin(origin: &Value, source: &Value, saved: &Value) -> Result<()> {
    let camera = &source["field_camera"];
    ensure!(
        camera["position_settled"] == true && camera["target_settled"] == true,
        "camera origin requires an observed settled view"
    );
    ensure!(
        origin.is_object()
            && camera["oracle_origin"].is_object()
            && same_values(origin, &camera["oracle_origin"])
            && saved.is_object()
            && same_values(&origin["settings"], saved),
        "camera origin differs from observed pose or saved desired settings"
    );
    Ok(())
}

/// Reconstruct visible birth parameters from particles observed after drawing.
fn verify_spark_origin(sparks: &[Value], source: &Value, effect_tick: Option<u64>) -> Result<()> {
    let points: Vec<_> = source["actors"]
        .as_array()
        .context("missing source actors")?
        .iter()
        .filter(|a| a["draw_callback"] == "8000e720")
        .collect();
    ensure!(
        points.len() == 1 || (points.is_empty() && sparks.is_empty()),
        "spark registration requires one observed emitter unless both are absent"
    );
    let particles: Vec<_> = source["particles"]
        .as_array()
        .context("missing source particles")?
        .iter()
        .filter(|p| p["recipe_address"] == "8020a4d8")
        .collect();
    ensure!(
        sparks.len() == particles.len(),
        "spark origin count differs from source"
    );
    for (spark, particle) in sparks.iter().zip(particles) {
        // MemoryWatcher samples after the presentation. Its observed timer is
        // one update newer than the image age reconstructed for the replay.
        let age = 59
            - particle["timer"]
                .as_i64()
                .context("missing particle timer")?;
        let speed = particle["velocity"][2]
            .as_f64()
            .context("missing spark speed")?;
        let size = particle["size"][0].as_f64().context("missing spark size")?;
        let center = &points[0]["position"];
        let position = &particle["position"];
        let x = position[0].as_f64().context("missing spark x")?
            - center[0].as_f64().context("missing emitter x")?;
        let y = position[1].as_f64().context("missing spark y")?
            - center[1].as_f64().context("missing emitter y")?;
        ensure!(
            (0..=60).contains(&age)
                && x.fract() == 0.
                && y.fract() == 0.
                && size.fract() == 0.
                && (speed * 8.).fract() == 0.
                && position[2].as_f64().context("missing spark z")?
                    == center[2].as_f64().context("missing emitter z")? + (age + 1) as f64 * speed,
            "source spark does not follow the rising-particle recipe"
        );
        ensure!(
            *spark
                == json!({"save_point":0,"age":age,"offset":[x as i32,y as i32],
            "size":size as u8,"speed_eighths":(speed * 8.) as u8}),
            "spark origin differs from independently observed birth parameters"
        );
        if let Some(tick) = effect_tick {
            let rotation = ((tick as u32).wrapping_sub(age as u32) & 127) + age as u32 + 1;
            ensure!(
                particle["rotation"][2].as_f64() == Some(f64::from(rotation)),
                "source spark rotation differs from the running effect clock"
            );
        }
    }
    Ok(())
}
fn slot_confirmation(native: &Value, source: &Value, animated: bool) -> Result<Value> {
    let word = |key: &str| -> Result<u32> {
        Ok(u32::try_from(
            observed_word(source, key).with_context(|| format!("missing observation {key}"))?,
        )?)
    };
    let selection = word("save_selection_word")?;
    let mode = word("save_mode_word")?;
    let screens = word("save_screen_word")?;
    let screen = screens >> 16;
    let opacity = word("save_popup_alpha_word")? >> 24;
    let page = match mode & 255 {
        0 => "Slots(Save)",
        1 => "Slots(Load)",
        _ => anyhow::bail!("unsupported source save menu mode"),
    };
    let confirming = matches!(screen, 6 | 13 | 14);
    ensure!(
        confirming || animated && screen == 5,
        "unsupported source popup screen"
    );
    let yes = (mode >> 8) & 255 == 0;
    let mut expected = json!({"page":page,"bank":selection >> 24,"slot":(selection >> 16) & 255,
        "confirmation":confirming.then_some(yes),"focus":"List","busy":false,"notice":null});
    let menu = &native["menu"];
    let mut actual = json!({"page":menu["page"],"bank":menu["bank"],"slot":menu["slot"],
        "confirmation":menu["confirmation"],"focus":menu["focus"],"busy":menu["busy"],"notice":menu["notice"]});
    if animated {
        expected["popup"] = if opacity == 0 {
            Value::Null
        } else {
            ensure!(
                !source["save_popup_content"].is_null(),
                "popup content was not observed before dismissal"
            );
            json!({"content":source["save_popup_content"],"opacity":opacity})
        };
        actual["popup"] = menu["popup"].clone();
    }
    let passed = expected == actual && (animated || opacity == 255 && confirming);
    Ok(
        json!({"expected":expected,"actual":actual,"source_screen":screen,"source_opacity":opacity,"passed":passed}),
    )
}
#[derive(Clone, Copy, PartialEq)]
enum Prompt {
    Save,
    Action,
    Skit,
}

fn field_prompt(kind: Prompt, native: &Value, source: &Value) -> Result<Value> {
    let name = match kind {
        Prompt::Save => "save",
        Prompt::Action => "action",
        Prompt::Skit => "skit",
    };
    let prefix = if kind != Prompt::Skit {
        "action_prompt"
    } else {
        "skit_prompt"
    };
    let alpha =
        observed_word(source, &format!("{prefix}_alpha")).context("missing prompt opacity")?;
    let remaining =
        observed_word(source, &format!("{prefix}_remaining")).context("missing prompt lifetime")?;
    ensure!(
        alpha <= 255
            && remaining < if kind == Prompt::Skit { 1800 } else { 30 }
            && (kind != Prompt::Save || source["action_prompt"] == 23),
        "source action is not a supported {name} prompt"
    );
    let suppressed = observed_word(source, "field_control_flags_word")
        .context("missing field control observation")?
        >> 24
        != 0;
    let mut expected = if alpha == 0 || suppressed {
        Value::Null
    } else {
        json!({"opacity":alpha,"text_opacity":if remaining < 19 { (remaining + 1) * 12 } else { 255 }})
    };
    if kind == Prompt::Skit && alpha != 0 && !suppressed {
        expected["id"] = json!(observed_word(source, "skit_id").context("missing skit ID")?);
    }
    if kind == Prompt::Action && alpha != 0 && !suppressed {
        expected["id"] =
            json!(observed_word(source, "action_prompt").context("missing action ID")?);
    }
    let mut actual = native.get(format!("{name}_prompt")).cloned();
    if let Some(Value::Object(value)) = &mut actual {
        value.remove("title"); // Text is checked by the image gate.
    }
    let native_phase = native
        .get("effect_counter")
        .and_then(Value::as_u64)
        .context("missing native prompt phase")?;
    let source_phase =
        observed_word(source, "presentation_counter").context("missing source prompt phase")?;
    let scene = observed_word(source, "scene_flags_word").context("missing source scene")?;
    let source_field = scene >> 24 == 0 && scene & 0x7f == 7;
    let native_field = ["menu", "shop", "skit"]
        .into_iter()
        .all(|key| native[key].is_null());
    let blink_matches = (source_field && native_field)
        .then_some(alpha == 0 || suppressed || native_phase & 32 == source_phase & 32);
    let not_checked_reason = blink_matches
        .is_none()
        .then_some("a nonfield scene retains the previous prompt presentation");
    Ok(
        json!({"expected":expected,"actual":actual,"blink_matches":blink_matches,
        "not_checked_reason":not_checked_reason,
        "field_presentation":{"native_active":native_field,"source_active":source_field},
        "passed":actual.as_ref().is_some_and(|p| *p == expected)
            && native_field == source_field && blink_matches.unwrap_or(true)}),
    )
}

fn vector_error(native: &Value, source: &Value, prefix: &str) -> Result<f64> {
    let values = native.as_array().context("missing native vector")?;
    ensure!(values.len() == 3, "invalid native vector");
    let mut error = 0f64;
    for (value, axis) in values.iter().zip(["x", "y", "z"]) {
        let native = value.as_f64().context("invalid native coordinate")?;
        error =
            error.max((native - observed_float(source, &format!("{prefix}_{axis}_bits"))?).abs());
    }
    Ok(error)
}
impl Fixture {
    fn verify(&self, base: &Path) -> Result<PathBuf> {
        let path = base.join(&self.path).canonicalize()?;
        check_hash(&path, &self.sha256)?;
        Ok(path)
    }
}
fn check_hash(path: &Path, expected: &str) -> Result<()> {
    ensure!(
        file_hash(path)? == expected,
        "fixture hash differs: {}",
        path.display()
    );
    Ok(())
}
pub(super) fn file_hash(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid case name"
    );
    Ok(())
}
fn verify_start(
    start: &Start,
    map: &Value,
    story: &Value,
    position: &Value,
    heading: &Value,
) -> Result<()> {
    ensure!(
        map.as_u64() == Some(u64::from(start.map_id))
            && story.as_i64() == Some(i64::from(start.story)),
        "paired field/story differs"
    );
    ensure!(
        position.as_array().is_some_and(|p| p.len() == 3
            && p.iter().zip(start.position).all(|(a, b)| a
                .as_f64()
                .is_some_and(|a| (a - f64::from(b)).abs() <= 0.001)))
            && heading
                .as_f64()
                .is_some_and(|a| (a - f64::from(start.heading)).abs() <= 0.001),
        "paired player pose differs"
    );
    Ok(())
}
fn run_logged(command: &mut Process, log: &Path) -> Result<()> {
    let file = fs::File::create(log)?;
    let status = command
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file))
        .status()?;
    ensure!(
        status.success(),
        "replay process failed ({status}); see {}",
        log.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooking_observations_do_not_require_a_restartable_checkpoint() {
        let mut source = json!({"cooking_settings_word":0,"cooking_known_word":0});
        for index in 0..132 {
            let key = match index {
                10 => "ex_gem_inventory_word".into(),
                124 => "ex_max_inventory_word".into(),
                _ => format!("inventory_{index}_word"),
            };
            source[key] = json!(0);
        }
        for character in 0..9 {
            source[format!("tech_character_{character}_vitals_word")] = json!((100 << 16) | 10);
            source[format!("tech_character_{character}_conditions_word")] = json!(0);
            for index in 0..6 {
                source[format!("cooking_character_{character}_training_{index}_word")] = json!(0);
            }
        }
        let party = json!({"cooking":{"known":0,"full":false,"recipe":0,"chef":0},
            "items":{},"members":vec![json!({"hp":100,"tp":10,"conditions":0,"cooking":vec![0;24]});9]});
        let mut native = json!({"persistent_party":party,"checkpoint":null,"menu":null});
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], true);
        native["persistent_party"]["members"][0]["hp"] = json!(99);
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], false);
        native["persistent_party"] = party.clone();
        native["menu"] = json!({"party":party});
        native["menu"]["party"]["items"] = json!({"1":1});
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], false);
        native["menu"] = Value::Null;
        native["checkpoint"] = json!({"progress":{"party":party}});
        native["persistent_party"] = Value::Null;
        assert!(cooking_state(&native, &source).is_err());
        native.as_object_mut().unwrap().remove("persistent_party");
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], true);
    }

    #[test]
    fn action_hint_origins_require_exact_observations_and_field_control() {
        let origin = json!({"id":2,"opacity":255,"remaining":29});
        let source = json!({"action_prompt":origin,
            "field_presentation":{"scene_flags":0x87,"control_flags":0}});
        assert!(verify_action_prompt_origin(&origin, &source).is_ok());
        for (key, value) in [("id", 1), ("opacity", 254), ("remaining", 28)] {
            let mut changed = origin.clone();
            changed[key] = json!(value);
            assert!(verify_action_prompt_origin(&changed, &source).is_err());
            let mut missing = source.clone();
            missing["action_prompt"]
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(verify_action_prompt_origin(&origin, &missing).is_err());
        }
        for (key, value) in [
            ("scene_flags", 0x01000087),
            ("scene_flags", 0x8b),
            ("control_flags", 0x01000000),
        ] {
            let mut inactive = source.clone();
            inactive["field_presentation"][key] = json!(value);
            assert!(verify_action_prompt_origin(&origin, &inactive).is_err());
        }
        assert!(verify_action_prompt_origin(&origin, &json!({"action_prompt":origin})).is_err());
    }

    #[test]
    fn camera_origins_require_settled_observations_and_unchanged_settings() {
        let settings = json!({"angles":[330,0,38],"distance":1669});
        let origin = json!({"settings":settings,"angles":[330.875,0,38.875],
            "distance":1669.00048828125,"position":[-1940,378,965],"target":[-2855,1513,153]});
        let mut source = json!({"field_camera":{"position_settled":true,
            "target_settled":true,"oracle_origin":origin}});
        assert!(verify_camera_origin(&origin, &source, &settings).is_ok());
        assert!(verify_camera_origin(&origin, &json!({}), &settings).is_err());
        source["field_camera"]["position_settled"] = json!(false);
        assert!(verify_camera_origin(&origin, &source, &settings).is_err());
        source["field_camera"]["position_settled"] = json!(true);
        let mut changed = origin.clone();
        changed["angles"][0] = json!(330);
        assert!(verify_camera_origin(&changed, &source, &settings).is_err());
        changed = settings.clone();
        changed["distance"] = json!(1670);
        assert!(verify_camera_origin(&origin, &source, &changed).is_err());
    }

    #[test]
    fn spark_origins_verify_empty_fields_and_preserve_particle_evidence() {
        let empty = json!({"actors":[],"particles":[]});
        assert!(verify_spark_origin(&[], &empty, Some(10)).is_ok());
        assert!(verify_spark_origin(&[], &json!({"actors":[]}), Some(10)).is_err());
        let spark = json!({"save_point":0,"age":1,"offset":[0,0],"size":20,"speed_eighths":8});
        assert!(verify_spark_origin(std::slice::from_ref(&spark), &empty, Some(10)).is_err());
        let mut source = json!({"actors":[],"particles":[{
            "recipe_address":"8020a4d8","timer":58,"velocity":[0,0,1],
            "size":[20,20],"position":[0,0,2],"rotation":[0,0,11]
        }]});
        assert!(verify_spark_origin(&[], &source, Some(10)).is_err());
        let emitter = json!({"draw_callback":"8000e720","position":[0,0,0]});
        source["actors"] = json!([emitter]);
        assert!(verify_spark_origin(std::slice::from_ref(&spark), &source, Some(10)).is_ok());
        source["particles"][0]["position"][2] = json!(3);
        assert!(verify_spark_origin(&[spark], &source, Some(10)).is_err());
        source["particles"] = json!([]);
        assert!(verify_spark_origin(&[], &source, Some(10)).is_ok());
        source["actors"] = json!([emitter, emitter]);
        assert!(verify_spark_origin(&[], &source, Some(10)).is_err());
    }

    #[test]
    fn legacy_save_point_alias_requires_the_same_raw_word() {
        let mut capture = json!({"memory_watch":{"locations":{
            "8035a73c":"field_control_flags_word"
        }}});
        assert!(save_point_alias(&capture));
        capture["memory_watch"]["locations"]["8035A73C"] = json!("field_save_point_word");
        assert!(save_point_alias(&capture));
        let reverse = source_observation(r#"{"field_save_point_word":65536}"#, true).unwrap();
        assert_eq!(reverse["field_control_flags_word"], 65536);
        let word = json!({"field_control_flags_word":0x1234_abcd_u32});
        let source = source_observation(&word.to_string(), true).unwrap();
        assert_eq!(
            observed_word(&source, "field_save_point_word").unwrap(),
            0x1234_abcd
        );
        assert_eq!(
            source_observation(&source.to_string(), true).unwrap(),
            source
        );
        for invalid in [
            json!({"field_control_flags_word":1,"field_save_point_word":2}),
            json!({"field_control_flags_word":0x1_0000_0000_u64}),
            json!({"field_control_flags_word":null}),
            json!({"field_save_point_word":0x1_0000_0000_u64}),
            json!({"field_save_point_word":null}),
        ] {
            assert!(source_observation(&invalid.to_string(), true).is_err());
        }
        for source in [
            source_observation("{}", true).unwrap(),
            source_observation(&word.to_string(), false).unwrap(),
        ] {
            assert!(observed_word(&source, "field_save_point_word").is_err());
        }
        capture["memory_watch"]["aliases"] = json!({"8035a740":["field_save_point_word"]});
        assert!(!save_point_alias(&capture));
        capture["memory_watch"]["aliases"] = json!({"8035a73c":["field_save_point_word"]});
        assert!(save_point_alias(&capture));
        capture["memory_watch"]["locations"]["8035a740"] = json!("field_save_point_word");
        assert!(!save_point_alias(&capture));
        capture["memory_watch"]["locations"] = json!({"8035a73c 0":"field_control_flags_word"});
        assert!(!save_point_alias(&capture));
    }

    #[test]
    fn shop_gate_checks_money_and_baskets_without_reading_inactive_shared_storage() {
        // Halo after selling one Apple Gel: 450 Gald, 100 spent, three gels remain.
        let mut source = json!({
            "presentation_counter":37351,"ui_clock":37784,"scene_flags_word":0x02000087,
            "shop_menu_04_word":0x00030000,"shop_menu_08_word":0x00030000,
            "shop_menu_0c_word":0,"shop_menu_10_word":2,"shop_menu_14_word":0x00010001,
            "shop_menu_18_word":0,"shop_menu_1c_word":0,"shop_menu_24_word":0x0000e000,
            "shop_basket_0_word":0x004000c0,"shop_basket_1_word":0x02c00000,
            "party_gald_word":450,"party_spent_gald_word":100,
            "visited_shops_first_word":2,"visited_shops_last_word":0
        });
        for index in 0..132 {
            let key = match index {
                10 => "ex_gem_inventory_word".into(),
                124 => "ex_max_inventory_word".into(),
                _ => format!("inventory_{index}_word"),
            };
            source[key] = json!(if index == 0 { 3 << 16 } else { 0 });
        }
        let frame: Frame = serde_json::from_value(json!({
            "name":"sale","native":"sale","dolphin_vi":1641,"shop_state":true
        }))
        .unwrap();
        verify_frame_observations(&frame, &source).unwrap();
        let native = json!({"presentation_counter":37784,"menu":null,"shop":{
            "id":1,"choice":"sell","focus":"items","row":0,"first":0,"category":0,
            "rows":[{"id":1,"quantity":0},{"id":3,"quantity":0},{"id":11,"quantity":0}],
            "total":0,"fade":0,"scroll":0,"description_previous":"None","description_opacity":15,
            "gald":450,"spent_gald":100,"visited":[1],"items":{"1":3}
        }});
        assert_eq!(shop_state(&native, &source).unwrap()["passed"], true);
        for (key, value) in [
            ("gald", json!(400)),
            ("spent_gald", json!(0)),
            ("items", json!({"1":4})),
        ] {
            let mut wrong = native.clone();
            wrong["shop"][key] = value;
            assert_eq!(shop_state(&wrong, &source).unwrap()["passed"], false);
        }
        let mut wrong = native.clone();
        wrong["shop"]["rows"][0]["quantity"] = json!(1);
        assert_eq!(shop_state(&wrong, &source).unwrap()["passed"], false);
        for key in source.as_object().unwrap().keys() {
            let mut missing = source.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                verify_frame_observations(&frame, &missing).is_err(),
                "accepted missing {key}"
            );
        }
        let mut overflow = source.clone();
        overflow["party_gald_word"] = json!(u64::from(u32::MAX) + 1);
        assert!(ShopState::observe(&overflow).is_err());
        source["shop_menu_10_word"] = json!(0);
        source["shop_menu_14_word"] = json!(0x00020001);
        source["shop_menu_04_word"] = json!(0x00030002);
        source["shop_menu_1c_word"] = json!(0xff000002_u32);
        source.as_object_mut().unwrap().remove("shop_basket_0_word");
        source.as_object_mut().unwrap().remove("shop_basket_1_word");
        let mut equipment = native;
        equipment["menu"] = json!({"page":"Equip"});
        equipment["shop"]["choice"] = json!("equip");
        equipment["shop"]["focus"] = json!("root");
        equipment["shop"]["fade"] = json!(255);
        assert_eq!(shop_state(&equipment, &source).unwrap()["passed"], true);
        equipment["menu"] = Value::Null;
        assert_eq!(shop_state(&equipment, &source).unwrap()["passed"], false);
    }

    #[test]
    fn prompt_blink_checks_the_absolute_rendered_effect_phase() {
        let source = json!({
            "presentation_counter":32,"field_control_flags_word":0,"scene_flags_word":0x87,
            "action_prompt":1,"action_prompt_alpha":255,"action_prompt_remaining":29,
        });
        let mut native = json!({
            "effect_counter":32,"presentation_counter":0,
            "action_prompt":{"id":1,"opacity":255,"text_opacity":255},
        });
        assert_eq!(
            field_prompt(Prompt::Action, &native, &source).unwrap()["passed"],
            true
        );
        native["effect_counter"] = json!(0);
        native["presentation_counter"] = json!(32);
        assert_eq!(
            field_prompt(Prompt::Action, &native, &source).unwrap()["blink_matches"],
            false
        );
        native.as_object_mut().unwrap().remove("effect_counter");
        assert!(field_prompt(Prompt::Action, &native, &source).is_err());
    }

    #[test]
    fn modal_prompt_checks_preserve_scene_and_content_gates() {
        for (scene, modal) in [(0x01000087, "menu"), (0x02000087, "shop"), (0x8b, "skit")] {
            let source = json!({
                "presentation_counter":32,"field_control_flags_word":0,"scene_flags_word":scene,
                "action_prompt":1,"action_prompt_alpha":255,"action_prompt_remaining":29,
            });
            let mut native = json!({
                "effect_counter":0,"presentation_counter":32,
                "action_prompt":{"id":1,"opacity":255,"text_opacity":255},
            });
            native[modal] = json!({});
            let result = field_prompt(Prompt::Action, &native, &source).unwrap();
            assert_eq!(result["passed"], true);
            assert!(result["blink_matches"].is_null());
            assert!(result["not_checked_reason"].is_string());
            native[modal] = Value::Null;
            assert_eq!(
                field_prompt(Prompt::Action, &native, &source).unwrap()["passed"],
                false
            );
            native[modal] = json!({});
            native["action_prompt"]["opacity"] = json!(254);
            assert_eq!(
                field_prompt(Prompt::Action, &native, &source).unwrap()["passed"],
                false
            );
        }
    }

    #[test]
    fn reused_observations_require_every_requested_transition_word() {
        let mut frame: Frame = serde_json::from_value(json!({
            "name":"opening", "native":"opening", "dolphin_vi":183,
            "effect_clock_state":true,
        }))
        .unwrap();
        let mut source = json!({"presentation_counter":37734});
        assert!(verify_frame_observations(&frame, &source).is_ok());
        source["ui_clock"] = json!(38267);
        frame.figurine_state = true;
        source["figurine_mode_row_word"] = json!(0);
        source["figurine_selection_scroll_word"] = json!(0);
        source["figurine_scroll_count_word"] = json!(19);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("figurine_fade_word")
        );
        frame.figurine_state = false;
        frame.skit_prompt = true;
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("field_control_flags_word")
        );
        frame.skit_prompt = false;
        frame.effect_state = true;
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("random_state")
        );
        source["random_state"] = json!(42);
        frame.tech_navigation_state = true;
        source["ui_clock"] = json!(38267);
        for offset in (0..=0x20).step_by(4) {
            source[format!("tech_menu_{offset:02x}_word")] = json!(0);
        }
        source["party_control_types_word"] = json!(0);
        source["tech_counts_0_word"] = json!(0);
        assert!(verify_frame_observations(&frame, &source).is_ok());
        source.as_object_mut().unwrap().remove("tech_menu_18_word");
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("tech_menu_18_word")
        );
        source["tech_menu_18_word"] = json!(0);
        source["tech_counts_0_word"] = json!(3 << 16);
        source["tech_0_choices_0_word"] = json!(0);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("tech_0_choices_1_word")
        );
        source["tech_0_choices_1_word"] = json!(0);
        source["party_control_types_word"] = json!(2 << 24);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("controller_0_status_word")
        );
        source["controller_0_status_word"] = json!(0);
        assert!(verify_frame_observations(&frame, &source).is_ok());
        // Assist lists belong to the target, independently of the selected character.
        source["tech_menu_00_word"] = json!(13 << 16);
        source["tech_menu_04_word"] = json!(3);
        source["tech_counts_1_word"] = json!(1);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("tech_3_choices_0_word")
        );
        source["tech_3_choices_0_word"] = json!(0);
        assert!(verify_frame_observations(&frame, &source).is_ok());
        frame.tech_navigation_state = false;
        frame.eye_actors.push(211);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("missing actor identity observation actor_211_id")
        );
        source["actor_211_id"] = json!(-1);
        source["actor_211_eye_mode_word"] = json!(0);
        source["actor_211_eye_mouth_mode_word"] = json!(0);
        // A recorded, reused slot is a separate comparison failure.
        assert!(verify_frame_observations(&frame, &source).is_ok());
        assert_eq!(observed_actor_id(&source, 211).unwrap(), -1);
        frame.eye_actors.clear();
        frame.inventory_state = true;
        for offset in [0x00, 0x10, 0x14, 0x18, 0x1c, 0x24, 0x28, 0x30] {
            source[format!("inventory_menu_{offset:02x}_word")] = json!(0);
        }
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("inventory_0_word")
        );
        source["inventory_menu_14_word"] = json!(6);
        assert!(
            verify_frame_observations(&frame, &source)
                .unwrap_err()
                .to_string()
                .contains("inventory_menu_0c_word")
        );
    }
}
