use super::{Oracle, Trace, compare};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::Command,
};

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
    let all = oracle.cases()?;
    let catalogue: serde_json::Value = serde_json::from_slice(&fs::read(
        repo.join("tools/oracle/cases/battle-artes.json"),
    )?)?;
    let mut inactive = BTreeMap::new();
    for slot in catalogue["inactive_authored_slots"]
        .as_array()
        .context("missing inactive slot inventory")?
    {
        let id = format!("party:{}:{}", slot["character"], slot["arte"]);
        let reason = slot["reason"]
            .as_str()
            .context("inactive slot has no reason")?;
        ensure!(
            all.iter().any(|case| case.id == id),
            "unknown inactive slot {id}"
        );
        ensure!(
            !reason.is_empty() && inactive.insert(id, reason).is_none(),
            "invalid inactive slot classification"
        );
    }
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

    let reference = if cases.iter().all(|case| inactive.contains_key(&case.id)) {
        output.join("dolphin")
    } else if let Some(path) = std::env::var_os("RESONANCE_ARTE_REFERENCE") {
        absolute(PathBuf::from(path))
    } else {
        let reference = output.join("dolphin");
        let disc = std::env::var_os("RESONANCE_DOLPHIN_DISC")
            .context("set RESONANCE_DOLPHIN_DISC or RESONANCE_ARTE_REFERENCE")?;
        let generated = output.join("cases.json");
        fs::write(
            &generated,
            serde_json::to_vec_pretty(
                &cases
                    .iter()
                    .filter(|case| !inactive.contains_key(&case.id))
                    .collect::<Vec<_>>(),
            )?,
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
        if let Some(reason) = inactive.get(&case.id) {
            report.push(json!({"case":case.id,"status":"inactive_authored_slot","reason":reason,"equivalent":null,"error":null}));
            continue;
        }
        let directory = output.join(case.id.replace(':', "-"));
        fs::create_dir(&directory)?;
        let result = (|| -> Result<()> {
            let path = reference.join(case.id.replace(':', "-")).join("trace.json");
            let mut source: Trace = serde_json::from_slice(
                &fs::read(&path)
                    .with_context(|| format!("missing source trace {}", path.display()))?,
            )?;
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
        report.push(json!({"case": case.id, "equivalent": error.is_none(), "error": error}));
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
