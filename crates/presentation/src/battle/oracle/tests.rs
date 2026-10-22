use super::{Oracle, Trace, compare};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{collections::BTreeSet, fs, path::PathBuf, process::Command};

pub(super) fn condition_branch(trace: &Trace, attempts: &[serde_json::Value]) -> Vec<bool> {
    attempts
        .iter()
        .map(|attempt| {
            trace.events.iter().any(|event| {
                event["kind"] == "condition_changed"
                    && ["owner", "target", "clock", "presentation_clock"]
                        .iter()
                        .all(|&key| event[key] == attempt[key])
                    && event["after"]
                        .as_array()
                        .is_some_and(|conditions| conditions.contains(&attempt["condition"]))
                    && event["before"]
                        .as_array()
                        .is_some_and(|conditions| !conditions.contains(&attempt["condition"]))
            })
        })
        .collect()
}

fn require_source_admission(source: &Trace) -> Result<()> {
    ensure!(
        !source.frames.is_empty(),
        "source trace has no execution window"
    );
    ensure!(
        source.setup["input"]["bound_admission_after_updates"].is_u64(),
        "source capture has no witnessed requested action admission; gaps: {:?}; other selection: {}",
        source.incomplete_observations,
        source.assessment["other_selection"],
    );
    Ok(())
}

#[test]
fn unadmitted_source_prelude_is_a_capture_failure() {
    let mut source = Trace {
        case: "enemy:219:2".into(),
        setup: json!({"input":{"button":"neutral"}}),
        frames: vec![json!({"combat_tick":3})],
        events: vec![],
        assessment: json!({"other_selection":{"selected":1,"expected":2}}),
        incomplete_observations: vec!["requested action admission was not observed".into()],
    };
    let error = require_source_admission(&source).unwrap_err().to_string();
    assert!(error.contains("source capture has no witnessed requested action admission"));
    assert!(error.contains("requested action admission was not observed"));
    source.setup["input"]["bound_admission_after_updates"] = json!(3);
    assert!(require_source_admission(&source).is_ok());
    source.frames.clear();
    assert!(require_source_admission(&source).is_err());
}

#[test]
fn initial_navigation_preserves_uninitialized_entry_targets_only() {
    let roster = [json!({"address":100}), json!({"address":200})];
    let mut observed = json!({"target":0,"action":[13]});
    assert!(
        super::initial_navigation_target(&observed, &roster, true)
            .unwrap()
            .is_none()
    );
    assert!(super::initial_navigation_target(&observed, &roster, false).is_err());
    observed["action"][0] = json!(3);
    assert!(super::initial_navigation_target(&observed, &roster, true).is_err());
    observed["target"] = json!(300);
    assert!(super::initial_navigation_target(&observed, &roster, true).is_err());
    observed["target"] = json!(200);
    assert_eq!(
        super::initial_navigation_target(&observed, &roster, true)
            .unwrap()
            .unwrap()
            .index(),
        1
    );
}

/// Explicitly run the catalogue, or a deterministic shard of it, against Dolphin.
/// A case stays in the report when setup, execution or observation fails.
#[test]
#[ignore = "requires cooked assets, a Dolphin disc/checkpoint and an explicit output directory"]
fn every_arte_matches_dolphin() -> Result<()> {
    const MAX_UPDATES: u32 = 60 * 60;
    const OFFENSIVE_STAT: u16 = 400;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let absolute = |path: PathBuf| {
        if path.is_absolute() {
            path
        } else {
            repo.join(path)
        }
    };
    let assets = absolute(
        std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join("local/all-assets")),
    );
    let output = absolute(PathBuf::from(
        std::env::var_os("RESONANCE_ARTE_OUTPUT").context("set RESONANCE_ARTE_OUTPUT")?,
    ));
    ensure!(
        !output.exists(),
        "preserve existing output: {}",
        output.display()
    );
    fs::create_dir_all(&output)?;
    let mut oracle = Oracle::load(&assets)?;
    // Optional branch discovery is bounded and uses the same CPU-only oracle/cache.
    // The inclusive range matches independently witnessed chance branches.
    let native_seeds = std::env::var("RESONANCE_ARTE_NATIVE_SEEDS")
        .ok()
        .map(|range| -> Result<_> {
            let (first, last) = range
                .split_once("..")
                .context("native seeds must be FIRST..LAST (inclusive)")?;
            let first: u64 = first.parse()?;
            let last: u64 = last.parse()?;
            ensure!(
                first <= last && last - first < 64,
                "native seed selection requires 1–64 candidates"
            );
            Ok(first..=last)
        })
        .transpose()?;
    let mut all = oracle.cases()?;
    let catalogue: serde_json::Value = serde_json::from_slice(&fs::read(
        repo.join("tools/oracle/cases/battle-artes.json"),
    )?)?;
    let excluded: BTreeSet<String> =
        serde_json::from_value(catalogue["excluded_comparisons"].clone())?;
    all.retain(|case| !excluded.contains(&case.id));
    let requested: BTreeSet<String> = std::env::var("RESONANCE_ARTE_CASES")
        .unwrap_or_default()
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    for id in &requested {
        ensure!(all.iter().any(|case| case.id == *id), "unknown arte {id}");
    }
    let shard = std::env::var("RESONANCE_ARTE_SHARD").unwrap_or_else(|_| "0/1".into());
    let (index, count) = shard.split_once('/').context("shard must be INDEX/COUNT")?;
    let (index, count): (usize, usize) = (index.parse()?, count.parse()?);
    ensure!(count > 0 && index < count, "invalid shard {shard}");
    let cases: Vec<_> = all
        .iter()
        .filter(|case| requested.is_empty() || requested.contains(&case.id))
        .enumerate()
        .filter(|(ordinal, _)| ordinal % count == index)
        .map(|(_, case)| case)
        .collect();
    ensure!(!cases.is_empty(), "empty catalogue selection");
    fs::write(
        output.join("inventory.json"),
        serde_json::to_vec_pretty(&json!({
            "catalogue_count": all.len(), "shard": shard,
            "selected": cases.iter().map(|case| &case.id).collect::<Vec<_>>()
        }))?,
    )?;

    let reference = if let Some(path) = std::env::var_os("RESONANCE_ARTE_REFERENCE") {
        absolute(PathBuf::from(path))
    } else {
        let reference = output.join("dolphin");
        let disc = std::env::var_os("RESONANCE_DOLPHIN_DISC")
            .context("set RESONANCE_DOLPHIN_DISC or RESONANCE_ARTE_REFERENCE")?;
        let generated = output.join("cases.json");
        fs::write(
            &generated,
            serde_json::to_vec_pretty(&cases.iter().collect::<Vec<_>>())?,
        )?;
        let mut bridge = Command::new("python3");
        bridge
            .current_dir(&repo)
            .arg("tools/oracle/arte_equivalence.py")
            .arg("--assets")
            .arg(&assets)
            .arg("--disc")
            .arg(disc)
            .arg("--output")
            .arg(&reference)
            .arg("--cases-file")
            .arg(generated)
            .arg("--max-vis")
            .arg(MAX_UPDATES.to_string())
            .arg("--offense")
            .arg(OFFENSIVE_STAT.to_string());
        // Retain individual failures from a partial batch and still run native cases.
        let status = bridge.status().context("start Dolphin arte bridge")?;
        fs::write(output.join("dolphin-exit.txt"), status.to_string())?;
        reference
    };

    let mut report = Vec::new();
    for case in cases {
        let directory = output.join(case.id.replace(':', "-"));
        fs::create_dir(&directory)?;
        let mut stage = "capture";
        let mut source_observations_complete = None;
        let mut native_observations_complete = None;
        let result = (|| -> Result<()> {
            let path = reference.join(case.id.replace(':', "-")).join("trace.json");
            let mut source: Trace = serde_json::from_slice(
                &fs::read(&path)
                    .with_context(|| format!("missing source trace {}", path.display()))?,
            )?;
            source_observations_complete = Some(source.incomplete_observations.is_empty());
            require_source_admission(&source)?;
            stage = "replay";
            // Bound a single action to one minute of simulation, including cleanup.
            let native = if let Some(seeds) = &native_seeds {
                ensure!(
                    source.setup.get("random_inputs").is_none(),
                    "observed random inputs cannot be combined with seed selection"
                );
                let stun = source
                    .setup
                    .get("contact_stun")
                    .is_some_and(|value| value.as_array().is_some_and(|values| !values.is_empty()));
                let effects = source
                    .setup
                    .get("chance_effects")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let conditions = source
                    .setup
                    .get("chance_conditions")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                ensure!(
                    stun || !effects.is_empty() || !conditions.is_empty(),
                    "seed selection requires independently witnessed chance branches"
                );
                ensure!(
                    effects.iter().all(|binding| {
                        binding["owner"].is_u64()
                            && binding["bank"].is_string()
                            && binding["program"].is_u64()
                            && binding["attempts"]
                                .as_array()
                                .is_some_and(|attempts| !attempts.is_empty())
                    }),
                    "conditional effect lacks its witnessed callback attempts"
                );
                ensure!(
                    conditions.iter().all(|attempt| {
                        attempt["condition"].is_string()
                            && attempt["applied"].is_boolean()
                            && attempt["chance_percent"]
                                .as_u64()
                                .is_some_and(|chance| (1..100).contains(&chance))
                            && ["owner", "target", "clock", "presentation_clock"]
                                .iter()
                                .all(|&key| attempt[key].is_u64())
                            && source.events.iter().any(|event| {
                                event["kind"] == "hit"
                                    && event["hp_change"].as_i64().is_some_and(|loss| loss < 0)
                                    && ["owner", "target", "clock", "presentation_clock"]
                                        .iter()
                                        .all(|&key| event[key] == attempt[key])
                            })
                    }) && condition_branch(&source, &conditions)
                        == conditions
                            .iter()
                            .filter_map(|attempt| attempt["applied"].as_bool())
                            .collect::<Vec<_>>(),
                    "conditional ailment lacks its witnessed contact outcome"
                );
                let branch = |trace: &Trace| {
                    json!({
                        "contact_stun": if stun { trace.setup["contact_stun"].clone() } else { serde_json::Value::Null },
                        "effects": trace.events.iter().filter(|event| event["kind"] == "program_started"
                            && effects.iter().any(|binding| ["owner", "bank", "program"].iter()
                                .all(|&key| event[key] == binding[key]))).collect::<Vec<_>>(),
                        "conditions": condition_branch(trace, &conditions),
                    })
                };
                let expected = branch(&source);
                let mut attempts = Vec::new();
                let mut selected = None;
                for seed in seeds.clone() {
                    source.setup["native_seed"] = json!(seed);
                    let candidate = oracle.run(case, &source, MAX_UPDATES);
                    let observed = candidate.as_ref().ok().map(&branch);
                    let matched = observed.as_ref() == Some(&expected);
                    attempts.push(json!({
                        "native_seed":seed, "branch":observed, "matched":matched,
                        "error":candidate.as_ref().err().map(|error| format!("{error:#}")),
                    }));
                    fs::write(
                        directory.join("seed-selection.json"),
                        serde_json::to_vec_pretty(&json!({
                            "source_seed":source.setup["seed"], "expected":expected,
                            "attempts":attempts, "selected_native_seed":matched.then_some(seed),
                        }))?,
                    )?;
                    let candidate = candidate?;
                    if matched {
                        selected = Some(candidate);
                        break;
                    }
                }
                let selected =
                    selected.context("no candidate matched the witnessed chance branch")?;
                fs::write(
                    directory.join("replay-source.json"),
                    serde_json::to_vec(&source)?,
                )?;
                selected
            } else {
                oracle.run(case, &source, MAX_UPDATES)?
            };
            fs::write(directory.join("native.json"), serde_json::to_vec(&native)?)?;
            stage = "observation";
            let damage = Command::new("python3")
                .current_dir(&repo)
                .arg("tools/oracle/damage_envelope.py")
                .arg("--normalize-native")
                .arg(directory.join("native.json"))
                .output()
                .context("start independent native damage envelope")?;
            ensure!(
                damage.status.success(),
                "native damage observation: {}",
                String::from_utf8_lossy(&damage.stderr)
            );
            let native: Trace = serde_json::from_slice(&fs::read(directory.join("native.json"))?)?;
            native_observations_complete = Some(native.incomplete_observations.is_empty());
            stage = if source_observations_complete == Some(false)
                || native_observations_complete == Some(false)
            {
                "observation"
            } else {
                "comparison"
            };
            if let Some(expected) = source.setup.get("contact_stun") {
                ensure!(
                    native.setup.get("contact_stun") == Some(expected),
                    "contact stun outcomes differ"
                );
            }
            compare::equivalent(&source, &native)?;
            Ok(())
        })();
        let error = result.err().map(|error| format!("{error:#}"));
        eprintln!("{}: {}", case.id, error.as_deref().unwrap_or("equivalent"));
        report.push(json!({
            "case": case.id,
            "equivalent": error.is_none(),
            "stage": if error.is_none() { "complete" } else { stage },
            "source_observations_complete": source_observations_complete,
            "native_observations_complete": native_observations_complete,
            "error": error,
        }));
        fs::write(
            output.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let failed = report
        .iter()
        .filter(|row| row["error"] != serde_json::Value::Null)
        .count();
    ensure!(
        failed == 0,
        "{failed}/{} arte comparisons failed; {}",
        report.len(),
        output.display()
    );
    Ok(())
}
