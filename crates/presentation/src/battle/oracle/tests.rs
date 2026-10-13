use super::{Oracle, Trace, compare};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    process::Command,
};

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
            let source: Trace = serde_json::from_slice(
                &fs::read(&path)
                    .with_context(|| format!("missing source trace {}", path.display()))?,
            )?;
            // Bound a single action to one minute of simulation, including cleanup.
            let native = oracle.run(case, &source, MAX_UPDATES)?;
            fs::write(directory.join("native.json"), serde_json::to_vec(&native)?)?;
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
