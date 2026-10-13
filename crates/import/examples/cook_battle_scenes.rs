//! Cook battle scenes, optionally refreshing actor and resident effect metadata.
use anyhow::{Context, Result, ensure};
use std::{fs, path::PathBuf};

fn unsupported_nodes(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(fields) => {
            usize::from(fields.get("kind").and_then(|v| v.as_str()) == Some("unsupported"))
                + usize::from(
                    fields
                        .get("unsupported_reason")
                        .and_then(|v| v.as_str())
                        .is_some_and(|reason| !reason.is_empty()),
                )
                + fields.values().map(unsupported_nodes).sum::<usize>()
        }
        serde_json::Value::Array(values) => values.iter().map(unsupported_nodes).sum(),
        _ => 0,
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let extracted = PathBuf::from(
        args.next()
            .context("usage: cook_battle_scenes EXTRACTED OUTPUT [--party] [--library LIBRARY] [--party-art IDS] [--ui] [--ui-only]")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: cook_battle_scenes EXTRACTED OUTPUT [--party] [--library LIBRARY] [--party-art IDS] [--ui] [--ui-only]")?,
    );
    let mut party = false;
    let mut ui = false;
    let mut ui_only = false;
    let mut library = None;
    let mut party_art = Vec::new();
    while let Some(flag) = args.next() {
        if flag == "--party" {
            party = true;
        } else if flag == "--ui-only" {
            ui = true;
            ui_only = true;
        } else if flag == "--ui" {
            ui = true;
        } else if flag == "--library" {
            library = Some(PathBuf::from(
                args.next().context("missing --library path")?,
            ));
        } else if flag == "--party-art" {
            party_art = args
                .next()
                .context("missing --party-art IDs")?
                .to_str()
                .context("party IDs are not UTF-8")?
                .split(',')
                .map(str::parse::<u8>)
                .collect::<Result<Vec<_>, _>>()?;
        } else {
            anyhow::bail!("expected --party, --library, --party-art, --ui or --ui-only");
        }
    }
    ensure!(
        !ui_only || !party && library.is_none() && party_art.is_empty(),
        "--ui-only cannot refresh actor content"
    );
    ensure!(library.is_none() || party, "--library requires --party");
    ensure!(
        party_art.is_empty() || party && library.is_some(),
        "--party-art requires --party and --library"
    );
    if ui {
        let paths = resonance_import::battle_ui::publish(&extracted, &output)?;
        fs::write(
            output.join(".ui-publication.json"),
            serde_json::to_vec(&paths)?,
        )?;
        println!("Battle UI: {} files", paths.len());
        if ui_only {
            return Ok(());
        }
    }
    if !party_art.is_empty() {
        let library = library.as_ref().unwrap();
        for character in 1..=9 {
            let path = resonance_content::battle_model::party_path(character);
            let target = output.join(&path);
            if !target.exists() {
                fs::create_dir_all(target.parent().unwrap())?;
                fs::copy(library.join(path), target)?;
            }
        }
        let paths = resonance_import::battle_model::party::publish_selected(
            &extracted, &output, &party_art,
        )?;
        // This local publication receipt updates the source ledger; it is not a runtime asset.
        fs::write(
            output.join(".party-art-publication.json"),
            serde_json::to_vec(&paths)?,
        )?;
        println!("Party artwork: {} selected bodies", paths.len());
    }

    if let Some(library) = &library {
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
        let paths = resonance_import::battle_effect::publish_cached(&usual, library, &output)?;
        println!(
            "Resident effects: {} metadata banks, artwork retained",
            paths.len()
        );
    }
    let publication = resonance_import::battle_scene::publish_all(&extracted, &output)?;
    // These counts describe retained metadata, not native action coverage.
    let mut unsupported = 0;
    let (mut prepared_actions, mut unsupported_actions, mut action_errors, mut hit_errors) =
        (0, 0, 0, 0);
    for path in publication.paths.iter().filter(|path| {
        path.starts_with("battle/scenes/")
            && path.ends_with(".json")
            && !path.ends_with("/catalogue.json")
    }) {
        let value: serde_json::Value = serde_json::from_slice(&fs::read(output.join(path))?)?;
        unsupported += unsupported_nodes(&value);
        if let Some(actions) = value.get("actions").and_then(|v| v.as_object()) {
            for action in actions.values() {
                match action.get("status").and_then(|v| v.as_str()) {
                    Some("prepared") => prepared_actions += 1,
                    Some("unsupported") => unsupported_actions += 1,
                    _ => {}
                }
            }
        }
        action_errors += usize::from(value.get("actions_error").is_some_and(|v| !v.is_null()));
        if let Some(projectiles) = value.get("projectiles").and_then(|v| v.as_array()) {
            hit_errors += projectiles
                .iter()
                .filter_map(|row| row.get("hit_errors").and_then(|v| v.as_object()))
                .map(serde_json::Map::len)
                .sum::<usize>();
        }
    }
    println!(
        "Scenes: {} converted, {} reused, {} failed; {} files; {} retained unsupported nodes.",
        publication.converted,
        publication.reused,
        publication.failures.len(),
        publication.paths.len(),
        unsupported,
    );
    println!(
        "Participant tracks: {prepared_actions} prepared, {unsupported_actions} unsupported, \
         {action_errors} malformed scene layouts; {hit_errors} unsupported callback rules."
    );
    if !publication.failures.is_empty() {
        eprintln!("{}", serde_json::to_string_pretty(&publication.failures)?);
    }
    ensure!(
        publication.failures.is_empty(),
        "scene conversion failed; full results: {}",
        output.join("battle/scenes/catalogue.json").display()
    );
    if party {
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
        let module = extracted.join("files/US_r_Top2Btl.rel");
        let path =
            resonance_import::battle_projectile::publish(&usual, &module, &output, "battle")?;
        println!("Resident projectile metadata: {path}");
        let path = resonance_import::battle_effect::publish_tints(&module, &output, "battle")?;
        println!("Battle tint metadata: {path}");
        let path = resonance_import::battle_profile::publish_party(
            &extracted.join("files/US_r_Top2Btl.rel"),
            &usual,
            &output,
            "battle",
        )?;
        println!("Party action metadata: {path}");
        let path = resonance_import::battle_unison::publish(&extracted, &output)?;
        println!("Unison catalogue: {path}");
        let paths = if library.is_some() {
            resonance_import::battle_enemy::publish_cached(&extracted, &output)?
        } else {
            resonance_import::battle_enemy::publish_all(&extracted, &output)?
        };
        let definitions = paths
            .iter()
            .filter(|path| path.ends_with("/definition.json"))
            .count();
        let effects = paths
            .iter()
            .filter(|path| path.ends_with("/effects.json"))
            .count();
        println!(
            "Enemy metadata: {definitions} definitions, {effects} effect banks, {} additional files",
            paths.len() - definitions - effects
        );
        if let Some(library) = library {
            let retained = if party_art.is_empty() {
                &library
            } else {
                &output
            };
            let paths = resonance_import::battle_model::party::publish_cached(
                &extracted, retained, &output,
            )?;
            println!(
                "Party motion metadata: {} bodies, artwork retained",
                paths.len()
            );
        }
    }
    Ok(())
}
