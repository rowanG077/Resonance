//! Stage battle catalogues against an existing library; retain each domain's outcome.
use anyhow::{Context, Result, ensure};
use resonance_import::{battle_audio, battle_event, battle_model, battle_scene};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};

fn outcome(name: &str, result: Result<Value>) -> Value {
    match result {
        Ok(value) => {
            println!(
                "{name}: {} closure paths",
                value["paths"].as_array().map_or(0, Vec::len)
            );
            value
        }
        Err(error) => {
            eprintln!("{name}: {error:#}");
            json!({"paths": [], "error": format!("{error:#}")})
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(
        args.len() == 4,
        "usage: cook_battle_catalogues EXTRACTED1 EXTRACTED2 LIBRARY OUTPUT"
    );
    let [first, second, library, output] = args.as_slice() else {
        unreachable!()
    };
    fs::create_dir_all(output)?;
    ensure!(
        library.canonicalize()? != output.canonicalize()?,
        "stage into a separate output directory"
    );

    let scenes = outcome(
        "scenes",
        battle_scene::publish_all(first, output).map(|publication| {
            json!({
                "source": first,
                "paths": publication.paths,
                "failures": publication.failures,
                "converted": publication.converted,
                "reused": publication.reused,
            })
        }),
    );
    let audio = outcome(
        "audio",
        battle_audio::publish_all(&[first.clone(), second.clone()], library, output)
            .map(|paths| json!({"paths": paths})),
    );
    let enemies = outcome(
        "enemies",
        (|| -> Result<Value> {
            let menu: resonance_content::menu_data::MenuData =
                serde_json::from_slice(&fs::read(library.join("game/menu-data.json"))?)?;
            let monsters = &menu.monsters()?.records;
            let mut paths: BTreeSet<_> =
                battle_model::publish_enemies(first, monsters, library, output)?
                    .into_iter()
                    .collect();
            for monster in monsters {
                let path = resonance_content::battle_model::enemy_path(monster.id);
                let enemy: resonance_content::battle_model::Enemy =
                    serde_json::from_slice(&fs::read(output.join(path))?)?;
                paths.extend(enemy.files.into_keys());
            }
            Ok(json!({"source": first, "paths": paths}))
        })(),
    );
    let events = outcome(
        "events",
        (|| -> Result<Value> {
            let publication = battle_event::publish(first, library, output)?;
            let mut paths = publication.paths.clone();
            for path in &publication.paths {
                let scene: resonance_content::battle_event::Scene =
                    serde_json::from_slice(&fs::read(output.join(path))?)?;
                paths.extend(scene.files.into_keys());
            }
            Ok(json!({"source": first, "paths": paths, "inputs": publication.inputs}))
        })(),
    );
    let report = json!({
        "sources": [first, second], "library": library, "output": output,
        "scenes": scenes, "audio": audio, "enemies": enemies, "events": events,
    });
    let report_path = output.join("battle-catalogues-report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("write {}", report_path.display()))?;
    let failed = ["scenes", "audio", "enemies", "events"]
        .iter()
        .any(|domain| report[*domain].get("error").is_some())
        || report["scenes"]["failures"]
            .as_object()
            .is_some_and(|failures| !failures.is_empty());
    println!("Report: {}", report_path.display());
    ensure!(
        !failed,
        "some battle publications failed; inspect the complete report"
    );
    Ok(())
}
