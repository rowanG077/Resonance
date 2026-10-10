//! One invocation verifies fixtures, replays both engines, and runs named gates.
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
    /// Explain the matching game events selected for comparison.
    registration: String,
    disc_sha256: String,
    native_save: Fixture,
    native_replay: Fixture,
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
    /// Register the verified video recording's first frame to a VI.
    video_first_vi: i64,
    /// Select a recording explicitly when Dolphin creates multiple files.
    #[serde(default)]
    video_segment: Option<usize>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    map_id: u32,
    story: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    name: String,
    native: String,
    dolphin_vi: u32,
    /// Compare saved formation and field leader.
    #[serde(default)]
    party_state: bool,
    /// Compare the visible Items category, list selection, and inventory counts.
    #[serde(default)]
    inventory_state: bool,
    /// Compare saved cooking choices, training, inventory, and party vitals.
    #[serde(default)]
    cooking_state: bool,
    /// Explicit acceptance regions; the full image remains diagnostic when present.
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
        pair.version == 2
            && pair.revision == 0
            && pair.replay.game_id == "GQSEAF"
            && !pair.description.is_empty()
            && !pair.registration.is_empty()
            && !pair.frames.is_empty(),
        "invalid paired case"
    );
    ensure!(!output.exists(), "paired output already exists");
    let base = case.parent().unwrap_or(Path::new("."));
    let save = pair.native_save.verify(base)?;
    let replay = pair.native_replay.verify(base)?;
    let state = pair.dolphin.state.verify(base)?;
    let prefix = PathBuf::from(format!("{}.dtm", state.display()));
    check_hash(&prefix, &pair.dolphin.prefix_sha256)?;
    check_hash(disc, &pair.disc_sha256)?;
    let native_spec: Value = serde_json::from_slice(&fs::read(&replay)?)?;
    ensure!(
        native_spec["version"] == 2,
        "paired native replay requires version 2 event steps"
    );
    let native_steps = native_spec["steps"]
        .as_array()
        .context("missing native replay steps")?;
    let native_captures: BTreeSet<_> = native_steps
        .iter()
        .filter(|step| step["do"] == "capture")
        .map(|step| step["name"].as_str().context("missing native capture name"))
        .collect::<Result<_>>()?;
    let source_script = append_dtm(&pair.replay, &fs::read(&prefix)?, pair.dolphin.start_poll)?;
    let input_hash = format!("{:x}", Sha256::digest(&source_script));
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
        verify_observations(&pair, &path.join("memory.jsonl"))?;
    }
    let mut names = BTreeSet::new();
    for frame in &pair.frames {
        validate_name(&frame.name)?;
        validate_name(&frame.native)?;
        ensure!(names.insert(&frame.name), "duplicate paired frame name");
        ensure!(
            frame.regions.iter().all(|&[x, y, w, h]| w > 0
                && h > 0
                && u64::from(x) + u64::from(w) <= 640
                && u64::from(y) + u64::from(h) <= 480),
            "invalid paired image region"
        );
        ensure!(
            i64::from(frame.dolphin_vi) >= pair.dolphin.video_first_vi
                && frame.dolphin_vi < pair.replay.polls
                && native_captures.contains(frame.native.as_str()),
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
        verify_native_reference(&report, path)?
    } else {
        file_hash(native)?
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
            .arg("--field-origin")
            .arg("--output")
            .arg(output.join("source-state.json")),
        &output.join("state.log"),
    )?;
    let observed: Value = serde_json::from_slice(&fs::read(output.join("source-state.json"))?)?;
    ensure!(
        observed["movie"]["input_count"].as_u64() == Some(u64::from(pair.dolphin.start_poll)),
        "Dolphin input origin differs from the manifest"
    );
    verify_start(
        &pair.start,
        &observed["field"]["map_id"],
        &observed["progress"]["story"],
    )?;
    let saved: Value = serde_json::from_slice(&fs::read(&save)?)?;
    let native_start = pair.start;
    verify_start(
        &native_start,
        &saved["state"]["map_id"],
        &saved["state"]["progress"]["script_globals"][16],
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
        native_record["complete"] == true
            && native_record["valid"] == true
            && native_record["audio_device"] == false
            && native_record["keyboard_input"] == true
            && native_record["unprepared_reads"] == 0
            && native_record["width"] == 640
            && native_record["height"] == 480,
        "native replay did not meet recording invariants"
    );
    let recorded_captures =
        verify_native_captures(&native_record, native_steps.len(), &native_captures)?;
    let initialized = &native_record["initial"];
    verify_start(
        &native_start,
        &initialized["map_id"],
        &initialized["progress"]["script_globals"][16],
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
            .args(["--xvfb", "--field-origin"]);
        let last_vi = i64::from(pair.frames.iter().map(|f| f.dolphin_vi).max().unwrap());
        let observations = last_vi
            .checked_sub(pair.dolphin.video_first_vi.min(0))
            .and_then(|vi| vi.checked_add(1))
            .and_then(|count| u32::try_from(count).ok())
            .context("video observation count exceeds the capture limit")?;
        command
            .arg("--video")
            .arg("--watch-vis")
            .arg(observations.to_string())
            .arg("--timeout")
            .arg((observations / 15 + 60).max(240).to_string());
        run_logged(&mut command, &output.join("capture.log"))?;
    }
    let capture: Value = serde_json::from_slice(&fs::read(dolphin_output.join("capture.json"))?)?;
    verify_capture(&pair, &capture, &input_hash)?;
    {
        let first_vi = pair.dolphin.video_first_vi;
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
    let memory: BTreeMap<u32, Value> = fs::read_to_string(dolphin_output.join("memory.jsonl"))?
        .lines()
        .map(|line| -> Result<_> {
            let value: Value = serde_json::from_str(line)?;
            let vi = u32::try_from(value["vi_sample"].as_u64().context("missing VI index")?)?;
            Ok((vi, value))
        })
        .collect::<Result<_>>()?;
    let mut states = Vec::new();
    let mut passed = true;
    for frame in &pair.frames {
        let source_state = memory
            .get(&frame.dolphin_vi)
            .context("missing registered VI")?;
        let native_state = recorded_captures[frame.native.as_str()];
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
        let presentation = fs::read(output.join(format!("native/{}.json", frame.native)))
            .context("missing presentation diagnostics")
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(Into::into));
        let diagnostics = pose_diagnostics(native_state, presentation, source_state);
        let mut gates = json!({});
        type CompareState = fn(&Value, &Value) -> Result<Value>;
        for (name, enabled, compare) in [
            ("party", frame.party_state, party_state as CompareState),
            ("inventory", frame.inventory_state, inventory_state),
            ("cooking", frame.cooking_state, cooking_state),
        ] {
            if enabled {
                gates[name] = compare(native_state, source_state)?;
            }
        }
        let good = gates
            .as_object()
            .unwrap()
            .values()
            .all(|gate| gate.is_null() || gate["passed"] == true)
            && location["passed"] == true;
        passed &= good;
        let state = json!({"name":frame.name,"dolphin_vi":frame.dolphin_vi,
            "location":location,
            "diagnostics":diagnostics,
            "gates":gates,
            "passed":good});
        states.push(state);
        let source = output.join(format!("video-frames/vi-{:06}.png", frame.dolphin_vi));
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
        let (images_passed, frame_results) =
            compare_frame_images(frame, &source, &actual, &output.join("images"))?;
        passed &= images_passed;
        results.extend(frame_results);
    }
    let mut audio_results = Vec::new();
    if !pair.audio.is_empty() {
        let dsp = verified_dsp_recording(&capture, &dolphin_output)?;
        for window in &pair.audio {
            let report =
                audio::compare(&dsp, &output.join("native/audio.wav"), None, &window.window)?;
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
            "native_artifacts":native_artifacts(&output.join("native"))?,
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

fn compare_frame_images(
    frame: &Frame,
    source: &Path,
    actual: &Path,
    output: &Path,
) -> Result<(bool, Vec<Value>)> {
    let mut passed = true;
    let mut results = Vec::new();
    let mut regions = vec![None];
    regions.extend(frame.regions.iter().copied().map(Some));
    for (i, region) in regions.into_iter().enumerate() {
        let name = format!("{}-{i}", frame.name);
        let full = compare(source, actual, &output.join(&name), 8, 0.01, region)?;
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
                source,
                actual,
                &output.join(&name).join("without-disc-loading"),
                8,
                0.01,
                region,
                exclusions,
            )?
        };
        let acceptance = region.is_some() || frame.regions.is_empty();
        passed &= !acceptance || good;
        results.push(json!({"name":name,"acceptance":acceptance,"passed":good,"full_image_passed":full,
            "excluded_regions":exclusions,
            "exclusion_reason":if exclusions.is_empty() { None } else {
                Some("Original DVD transfer telemetry; native shows loading text only during actual asset preparation.")
            }}));
    }
    Ok((passed, results))
}
// Validate requested observations before launching native rendering.
fn verify_observations(pair: &Pair, path: &Path) -> Result<()> {
    let memory: BTreeMap<u64, Value> = fs::read_to_string(path)?
        .lines()
        .map(|line| -> Result<_> {
            let value: Value = serde_json::from_str(line)?;
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
        if frame.disc_loading_overlay {
            observed_word(source, "figurine_model_load_word")?;
        }
        // These comparisons validate all needed source fields before inspecting native values.
        if frame.party_state {
            party_state(&Value::Null, source)?;
        }
        if frame.inventory_state {
            inventory_state(&Value::Null, source)?;
        }
        if frame.cooking_state {
            observed_word(source, "cooking_settings_word")?;
            observed_word(source, "cooking_known_word")?;
            observed_inventory(source)?;
            for character in 0..9 {
                observed_word(source, &format!("tech_character_{character}_vitals_word"))?;
                observed_word(
                    source,
                    &format!("tech_character_{character}_conditions_word"),
                )?;
                for index in 0..6 {
                    observed_word(
                        source,
                        &format!("cooking_character_{character}_training_{index}_word"),
                    )?;
                }
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
    let party = &native["persistent_party"];
    let actual = json!({"formation":party["formation"], "field_leader":party["field_leader"],
        "leader_locked":party["leader_locked"]});
    Ok(json!({"expected":expected,"actual":actual,"passed":actual == expected}))
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

fn native_conditions(conditions: u32, hp: u32) -> Result<(Value, BTreeSet<String>)> {
    ensure!(
        conditions & !(0x8000_03e0 | 0x001f_f000) == 0,
        "observation contains unsupported source conditions {conditions:#x}"
    );
    ensure!(
        (hp == 0) == (conditions & 0x8000_0000 != 0),
        "inconsistent knockout condition"
    );
    let ailments = json!({
        "poison": match conditions & 0x60 {
            0x20 => "mild", 0x40 => "severe", 0x60 => "both", _ => "none",
        },
        "paralysis": conditions & 0x80 != 0,
        "petrified": conditions & 0x100 != 0,
        "curse": conditions & 0x200 != 0,
    });
    let buffs = [
        (0x1000, "attack_up"),
        (0x4000, "defense_up"),
        (0x40000, "magic_attack_up"),
        (0x100000, "magic_defense_up"),
        (0x10000, "accuracy_up"),
        (0x2000, "attack_down"),
        (0x8000, "defense_down"),
        (0x20000, "accuracy_down"),
        (0x80000, "magic_attack_down"),
    ]
    .into_iter()
    .filter(|&(flag, _)| conditions & flag != 0)
    .map(|(_, buff)| buff.to_owned())
    .collect();
    Ok((ailments, buffs))
}

fn cooking_state(native: &Value, source: &Value) -> Result<Value> {
    let word = |key: &str| -> Result<u32> { Ok(observed_word(source, key)?.try_into()?) };
    let [full, recipe, chef, _] = word("cooking_settings_word")?.to_be_bytes();
    let party = &native["persistent_party"];
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
        let (ailments, buffs) = native_conditions(
            word(&format!("tech_character_{character}_conditions_word"))?,
            vitals >> 16,
        )?;
        let expected = json!({"hp":vitals >> 16,"tp":vitals & 65535,
            "ailments":ailments,"queued_buffs":buffs,"training":training});
        let member = &party["members"][character];
        let buffs: std::collections::BTreeSet<String> =
            serde_json::from_value(member["queued_buffs"].clone())?;
        let actual = json!({"hp":member["hp"],"tp":member["tp"],
            "ailments":member["ailments"],"queued_buffs":buffs,"training":member["cooking"]});
        passed &= expected == actual;
        members.push(json!({"character":character,"expected":expected,"actual":actual}));
    }
    Ok(json!({"expected":expected,"actual":party["cooking"],
        "items":{"expected":items,"actual":party["items"]},"members":members,"passed":passed}))
}

fn inventory_state(native: &Value, source: &Value) -> Result<Value> {
    let selection = observed_word(source, "inventory_menu_14_word")?;
    let mode = selection & 65535;
    let focus = match mode {
        0 => "Categories".to_owned(),
        1 | 5 | 10 => "List".to_owned(),
        2..=4 => "Target".to_owned(),
        6..=8 => "Transform(22)".to_owned(),
        9 => format!(
            "Discard({})",
            observed_word(source, "inventory_menu_18_word")? >> 16 == 0
        ),
        _ => anyhow::bail!("unsupported Items observation mode {mode}"),
    };
    let expected = json!({"category":observed_word(source, "inventory_menu_00_word")? >> 24,
        "row":selection >> 16,"first":observed_word(source, "inventory_menu_10_word")? >> 16,"focus":focus});
    let actual = &native["menu"]["inventory"];
    let items = observed_inventory(source)?;
    let actual_items = &native["persistent_party"]["items"];
    let passed = native["menu"]["page"] == "Items" && *actual == expected && *actual_items == items;
    Ok(
        json!({"expected":expected,"actual":actual,"items":{"expected":items,"actual":actual_items},"passed":passed}),
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
fn verify_native_captures<'a>(
    recording: &'a Value,
    steps: usize,
    expected: &BTreeSet<&str>,
) -> Result<BTreeMap<&'a str, &'a Value>> {
    ensure!(
        recording["completed_steps"].as_u64() == Some(steps as u64),
        "native recording did not complete every event step"
    );
    let mut captures = BTreeMap::new();
    for capture in recording["captures"]
        .as_array()
        .context("missing native captures")?
    {
        let name = capture["name"]
            .as_str()
            .context("missing native capture name")?;
        ensure!(
            captures.insert(name, capture).is_none(),
            "duplicate native capture {name}"
        );
    }
    ensure!(
        captures.keys().copied().eq(expected.iter().copied()),
        "native recording did not complete exactly the named captures"
    );
    Ok(captures)
}

fn verified_dsp_recording(capture: &Value, directory: &Path) -> Result<PathBuf> {
    let recordings = capture["audio"]["recordings"]
        .as_array()
        .context("missing Dolphin audio evidence")?;
    let dsp: Vec<_> = recordings
        .iter()
        .filter(|recording| {
            recording["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("_dspdump.wav"))
        })
        .collect();
    ensure!(
        dsp.len() == 1 && dsp[0]["finalized"] == true,
        "expected one finalized DSP recording"
    );
    let path = directory.join(dsp[0]["path"].as_str().unwrap());
    check_hash(
        &path,
        dsp[0]["sha256"]
            .as_str()
            .context("DSP recording has no content hash")?,
    )?;
    Ok(path)
}
fn native_artifacts(directory: &Path) -> Result<BTreeMap<String, String>> {
    fs::read_dir(directory)?
        .map(|entry| {
            let entry = entry?;
            ensure!(
                entry.file_type()?.is_file(),
                "unexpected native recording entry"
            );
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid native artifact name"))?;
            Ok((name, file_hash(&entry.path())?))
        })
        .collect()
}

fn verify_native_reference(report: &Value, directory: &Path) -> Result<String> {
    ensure!(
        report["complete"] == true,
        "native reference report is incomplete"
    );
    let renderer = report["native_binary_sha256"]
        .as_str()
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("native reference has no valid renderer identity")?;
    check_hash(
        &directory.join("case.json"),
        report["case_sha256"]
            .as_str()
            .context("native reference has no case identity")?,
    )?;
    let artifacts = native_artifacts(&directory.join("native"))?;
    ensure!(
        artifacts.contains_key("recording.json")
            && serde_json::to_value(artifacts)? == report["native_artifacts"],
        "native reference artifacts differ from their report"
    );
    Ok(renderer.to_owned())
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
fn verify_start(start: &Start, map: &Value, story: &Value) -> Result<()> {
    ensure!(
        map.as_u64() == Some(u64::from(start.map_id))
            && story.as_i64() == Some(i64::from(start.story)),
        "paired field/story differs"
    );
    Ok(())
}

/// Optional telemetry explains a mismatch; missing diagnostics never alter a gate.
fn pose_diagnostics(native: &Value, presentation: Result<Value>, source: &Value) -> Value {
    let mut diagnostics = json!({});
    let presentation = match presentation {
        Ok(value) => value,
        Err(error) => {
            diagnostics["presentation_error"] = json!(format!("{error:#}"));
            Value::Null
        }
    };
    let heading = || -> Result<f64> {
        let heading = native["heading"]
            .as_f64()
            .context("missing native heading")?;
        Ok(
            ((heading - observed_float(source, "controlled_heading_bits")? + 180.)
                .rem_euclid(360.)
                - 180.)
                .abs(),
        )
    };
    for (name, result) in [
        (
            "position_error",
            vector_error(&native["position"], source, "controlled"),
        ),
        (
            "camera_position_error",
            vector_error(&presentation["camera"]["position"], source, "camera"),
        ),
        (
            "camera_target_error",
            vector_error(&presentation["camera"]["target"], source, "camera_target"),
        ),
        ("heading_error", heading()),
    ] {
        diagnostics[name] = match result {
            Ok(value) => json!({"value":value}),
            Err(error) => json!({"unavailable":format!("{error:#}")}),
        };
    }
    diagnostics
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
    fn explicit_image_regions_accept_matching_content_and_report_background_differences()
    -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "resonance-image-regions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&directory)?;
        let source = directory.join("source.png");
        let actual = directory.join("actual.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([255; 3])).save(&source)?;
        let mut image = image::RgbImage::from_fn(16, 16, |x, y| {
            image::Rgb(if x < 4 && y < 4 { [255; 3] } else { [0; 3] })
        });
        image.save(&actual)?;
        let mut frame: Frame = serde_json::from_value(json!({
            "name":"dialogue", "native":"dialogue", "dolphin_vi":1,
            "regions":[[0,0,4,4]]
        }))?;
        let (passed, reports) =
            compare_frame_images(&frame, &source, &actual, &directory.join("explicit"))?;
        assert!(passed);
        assert_eq!(reports[0]["acceptance"], false);
        assert_eq!(reports[0]["passed"], false);
        assert_eq!(reports[1]["acceptance"], true);
        assert_eq!(reports[1]["passed"], true);
        assert!(
            directory
                .join("explicit/dialogue-0/difference.png")
                .is_file()
        );

        frame.regions.clear();
        assert!(
            !compare_frame_images(&frame, &source, &actual, &directory.join("whole"))?.0,
            "without explicit regions the whole image remains the acceptance gate"
        );
        frame.regions.push([0, 0, 4, 4]);
        image.put_pixel(0, 0, image::Rgb([0; 3]));
        image.save(&actual)?;
        assert!(
            !compare_frame_images(&frame, &source, &actual, &directory.join("broken"))?.0,
            "a mismatch inside an explicit region must fail"
        );
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn optional_pose_telemetry_reports_missing_values_without_rejecting_the_origin() -> Result<()> {
        let start: Start = serde_json::from_value(json!({"map_id":332,"story":2500}))?;
        verify_start(&start, &json!(332), &json!(2500))?;
        assert!(verify_start(&start, &json!(333), &json!(2500)).is_err());
        let native = json!({"position":[0,0,0],"heading":360});
        let mut source = json!({"controlled_heading_bits":0});
        for axis in ["x", "y", "z"] {
            source[format!("controlled_{axis}_bits")] = json!(0);
        }
        let report = pose_diagnostics(&native, Ok(Value::Null), &source);
        assert_eq!(report["position_error"]["value"], 0.);
        assert_eq!(report["heading_error"]["value"], 0.);
        assert!(report["camera_position_error"]["unavailable"].is_string());
        source["controlled_x_bits"] = json!(f32::NAN.to_bits());
        let report = pose_diagnostics(&native, Err(anyhow::anyhow!("missing sidecar")), &source);
        assert!(report["position_error"]["unavailable"].is_string());
        assert_eq!(report["presentation_error"], "missing sidecar");
        verify_start(&start, &json!(332), &json!(2500))?;
        Ok(())
    }

    #[test]
    fn paired_images_require_registered_video_instead_of_unhashed_frame_dumps() {
        assert!(
            serde_json::from_value::<Frame>(json!({
                "name":"A","native":"A","dolphin_vi":1,"dolphin":1
            }))
            .is_err()
        );
        let mut dolphin = json!({
            "state":{"path":"state","sha256":"hash"},"prefix_sha256":"prefix",
            "start_poll":1,"version":"2606","binary_sha256":"binary",
            "configs":{},"game_settings":{}
        });
        assert!(serde_json::from_value::<Dolphin>(dolphin.clone()).is_err());
        dolphin["video_first_vi"] = json!(0);
        assert!(serde_json::from_value::<Dolphin>(dolphin).is_ok());
    }

    #[test]
    fn native_recording_requires_each_named_capture_exactly_once() -> Result<()> {
        let expected = BTreeSet::from(["A", "B", "C"]);
        let recording = json!({"completed_steps":5,"captures":[
            {"name":"C","map_id":340},{"name":"A"},{"name":"B"}
        ]});
        assert_eq!(
            verify_native_captures(&recording, 5, &expected)?["C"]["map_id"],
            340
        );
        for names in [
            vec!["A", "A", "C"],
            vec!["A", "C"],
            vec!["A", "B", "D"],
            vec!["A", "B", "C", "C"],
        ] {
            let mut invalid = recording.clone();
            invalid["captures"] = json!(
                names
                    .into_iter()
                    .map(|name| json!({"name":name}))
                    .collect::<Vec<_>>()
            );
            assert!(verify_native_captures(&invalid, 5, &expected).is_err());
        }
        assert!(verify_native_captures(&recording, 6, &expected).is_err());
        let mut invalid = recording;
        invalid["captures"][0] = json!({});
        assert!(verify_native_captures(&invalid, 5, &expected).is_err());
        Ok(())
    }

    #[test]
    fn reused_dsp_recording_requires_its_recorded_content_hash() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "resonance-audio-reference-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&directory)?;
        let path = directory.join("capture_dspdump.wav");
        fs::write(&path, b"recorded PCM")?;
        let capture = json!({"audio":{"recordings":[{
            "path":"capture_dspdump.wav","finalized":true,"sha256":file_hash(&path)?
        }]}});
        assert_eq!(verified_dsp_recording(&capture, &directory)?, path);
        fs::write(&path, b"different PCM")?;
        assert!(verified_dsp_recording(&capture, &directory).is_err());
        fs::write(&path, b"recorded PCM")?;
        for key in ["sha256", "finalized"] {
            let mut invalid = capture.clone();
            invalid["audio"]["recordings"][0]
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(verified_dsp_recording(&invalid, &directory).is_err());
        }
        fs::remove_file(&path)?;
        assert!(verified_dsp_recording(&capture, &directory).is_err());
        fs::remove_dir(directory)?;
        Ok(())
    }

    #[test]
    fn native_reference_requires_complete_identified_unchanged_artifacts() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "resonance-native-reference-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(directory.join("native"))?;
        fs::write(directory.join("case.json"), b"case")?;
        fs::write(directory.join("native/recording.json"), b"recording")?;
        fs::write(directory.join("native/frame.png"), b"image")?;
        let report = json!({"complete":true,"native_binary_sha256":"a".repeat(64),
            "case_sha256":file_hash(&directory.join("case.json"))?,
            "native_artifacts":native_artifacts(&directory.join("native"))?});
        assert_eq!(
            verify_native_reference(&report, &directory)?,
            "a".repeat(64)
        );
        for missing in [
            "complete",
            "native_binary_sha256",
            "case_sha256",
            "native_artifacts",
        ] {
            let mut incomplete = report.clone();
            incomplete.as_object_mut().unwrap().remove(missing);
            assert!(verify_native_reference(&incomplete, &directory).is_err());
        }
        fs::write(directory.join("native/frame.png"), b"different image")?;
        assert!(verify_native_reference(&report, &directory).is_err());
        fs::write(directory.join("native/frame.png"), b"image")?;
        fs::write(directory.join("case.json"), b"different case")?;
        assert!(verify_native_reference(&report, &directory).is_err());
        fs::remove_dir_all(directory)?;
        Ok(())
    }

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
            "items":{},"members":vec![json!({"hp":100,"tp":10,
                "ailments":{"poison":"none","paralysis":false,"petrified":false,"curse":false},
                "queued_buffs":[],"cooking":vec![0;24]});9]});
        let mut native = json!({"persistent_party":party,"checkpoint":null,"menu":null});
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], true);
        native["persistent_party"]["members"][0]["hp"] = json!(99);
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], false);
        native["persistent_party"] = party.clone();
        native["menu"] = json!({"page":"Cooking"});
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], true);
        native["persistent_party"]["items"] = json!({"1":1});
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], false);
        native["persistent_party"] = Value::Null;
        assert!(cooking_state(&native, &source).is_err());
        native["persistent_party"] = party;
        native["persistent_party"]["members"][0]["ailments"]["poison"] = json!("both");
        native["persistent_party"]["members"][0]["queued_buffs"] =
            json!(["attack_up", "accuracy_up"]);
        source["tech_character_0_conditions_word"] = json!(0x11060);
        assert_eq!(cooking_state(&native, &source).unwrap()["passed"], true);
        for invalid in [1u32, 4, 0x400, 0x800, 0x200000, 0x80000000] {
            source["tech_character_0_conditions_word"] = json!(invalid);
            assert!(cooking_state(&native, &source).is_err());
        }
        source["tech_character_0_conditions_word"] = json!(0);
        source["tech_character_0_vitals_word"] = json!(10);
        assert!(cooking_state(&native, &source).is_err());
    }
}
