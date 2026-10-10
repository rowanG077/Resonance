//! Refresh current battle content in a disposable library copy.
use anyhow::{Context, Result, ensure};
use resonance_content::field_preload::{File, Manifest, Role, Shared};
use resonance_import::{
    battle_effect, battle_formation, battle_profile, battle_projectile, battle_recoil,
    battle_victory,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn paths(root: &Path, directory: &Path) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            result.extend(paths(root, &path)?);
        } else {
            result.push(
                path.strip_prefix(root)?
                    .to_str()
                    .context("non-UTF8 path")?
                    .to_owned(),
            );
        }
    }
    Ok(result)
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap())?;
    fs::write(target, bytes)?;
    Ok(())
}
fn main() -> Result<()> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("pass an isolated existing library")?,
    )
    .canonicalize()?;
    ensure!(
        output != repo.join("local/all-assets").canonicalize()?,
        "refresh an isolated copy"
    );
    let lock = output.join(".cook-media.lock");
    let mut owner = fs::File::create_new(&lock).context("another cook owns this library")?;
    let _lock = Lock(lock);
    writeln!(owner, "{}", std::process::id())?;
    let stage = tempfile::Builder::new()
        .prefix(".attack-refresh-")
        .tempdir_in(&output)?;
    let stage = stage.path();
    let extracted = repo.join("local/extracted/disc1");
    let rel = extracted.join("files/US_r_Top2Btl.rel");
    let common_path = extracted.join("files/BTL/BTLusual.dat");
    let common = fs::read(&common_path)?;
    let mut publications = BTreeSet::new();
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let (menu, metadata) = resonance_import::menu::publish_metadata(&extracted, &output, stage)?;
    publications.extend(metadata);
    publications.extend(resonance_import::battle_model::publish_enemies(
        &extracted,
        &menu.monsters()?.records,
        &output,
        stage,
    )?);
    publications.extend(
        resonance_import::battle_model::party::publish_all(&extracted, stage)?
            .into_iter()
            .map(|(path, _)| path),
    );
    publications.insert(resonance_import::battle_model::weapon::publish(
        &extracted, stage,
    )?);
    publications.insert(battle_recoil::publish(&rel, stage, "battle")?);
    publications.insert(battle_profile::publish_party(
        &rel, &common, stage, "battle",
    )?);
    publications.insert(battle_projectile::publish(&common, stage, "battle")?);
    battle_formation::publish(&common, &stage.join("battle/formations.json"))?;
    publications.insert("battle/formations.json".into());
    publications.extend(battle_effect::publish(&common, stage, "battle")?);
    publications.insert(battle_effect::publish_tints(&rel, stage, "battle")?);
    publications.insert(resonance_import::battle_scene::publish(
        &extracted, 237, stage,
    )?);
    publications.insert(battle_victory::publish(&extracted, stage)?);
    publications.extend(resonance_import::battle_ui::publish(&extracted, stage)?);
    let mut scripts = BTreeSet::new();
    for &(path, _) in resonance_script_content::FILES {
        if path.starts_with("battle/") && path.ends_with(".sym") {
            let path = format!("scripts/{path}");
            write(stage, &path, &fs::read(repo.join(&path))?)?;
            scripts.insert(path.clone());
            publications.insert(path);
        }
    }
    let mut shared: Shared =
        serde_json::from_slice(&fs::read(output.join("shared.preload.json"))?)?;
    shared.validate()?;
    let mut retired: BTreeSet<_> = shared
        .files
        .keys()
        .filter(|path| path.starts_with("scripts/battle/") && !scripts.contains(*path))
        .cloned()
        .collect();
    retired.extend(
        [
            "battle/voices.json",
            "battle/normal-actions.json",
            "battle/martial-actions.json",
            "battle/spell-actions.json",
        ]
        .map(str::to_owned),
    );
    retired.extend(
        paths(&output, &output.join("battle/enemies"))?
            .into_iter()
            .filter(|path| path.ends_with("/projectiles.json")),
    );
    let field_ids = fs::read_dir(output.join("fields"))?
        .map(|entry| -> Result<_> {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            Ok(name
                .strip_prefix("map-")
                .and_then(|name| name.strip_suffix(".preload.json"))
                .map(str::parse::<u32>)
                .transpose()?)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let shared_paths = shared
        .files
        .keys()
        .filter(|path| !retired.contains(*path))
        .chain(publications.iter())
        .cloned()
        .collect();
    let identity = resonance_import::save_identity::prepare(field_ids, &shared_paths, |path| {
        let staged = stage.join(path);
        Ok(fs::read(if staged.is_file() {
            staged
        } else {
            output.join(path)
        })?)
    })?;
    let identity_path = resonance_content::save_identity::PATH;
    write(stage, identity_path, &serde_json::to_vec(&identity)?)?;
    publications.insert(identity_path.into());
    let mut identities = BTreeMap::new();
    for path in paths(stage, stage)? {
        let bytes = fs::read(stage.join(&path))?;
        let roles = shared
            .files
            .get(&path)
            .map(|file| file.roles.clone())
            .unwrap_or_else(|| {
                [if path.ends_with(".sym") {
                    Role::Script
                } else if path.ends_with(".ktx2") {
                    Role::Texture
                } else {
                    Role::Data
                }]
                .into()
            });
        identities.insert(
            path,
            File {
                sha256: hash(&bytes),
                bytes: bytes.len() as u64,
                roles,
            },
        );
    }
    for path in &retired {
        shared.files.remove(path);
    }
    for (path, identity) in &identities {
        if publications.contains(path) || shared.files.contains_key(path) {
            shared.files.insert(path.clone(), identity.clone());
        }
    }
    shared.validate()?;
    write(
        stage,
        "shared.preload.json",
        &serde_json::to_vec_pretty(&shared)?,
    )?;
    let mut refreshed = vec!["shared.preload.json".to_owned()];
    for entry in fs::read_dir(output.join("fields"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("map-") || !name.ends_with(".preload.json") {
            continue;
        }
        let mut manifest: Manifest = serde_json::from_slice(&fs::read(entry.path())?)?;
        let mut changed = false;
        for path in &retired {
            changed |= manifest.files.remove(path).is_some();
        }
        for (path, identity) in &identities {
            if let Some(file) = manifest.files.get_mut(path)
                && (file.sha256 != identity.sha256 || file.bytes != identity.bytes)
            {
                file.sha256.clone_from(&identity.sha256);
                file.bytes = identity.bytes;
                changed = true;
            }
        }
        if changed {
            manifest.validate()?;
            let path = format!("fields/{name}");
            write(stage, &path, &serde_json::to_vec_pretty(&manifest)?)?;
            refreshed.push(path);
        }
    }
    let report = serde_json::json!({
        "scope": "primary-battle-development-refresh",
        "source_files": { "disc1/sys/main.dol": hash(&executable), "disc1/US_r_Top2Btl.rel": hash(&fs::read(rel)?), "disc1/BTL/BTLusual.dat": hash(&common) },
        "victory_inputs": battle_victory::inputs(&extracted)?,
        "publications": identities, "retired_files": retired, "refreshed_inventories": refreshed,
        "full_library_verified": false,
    });
    write(
        stage,
        "attack-refresh.json",
        &serde_json::to_vec_pretty(&report)?,
    )?;
    // Replacing directory entries preserves every hard-linked source file.
    // Conversion and inventory validation finish before installation begins.
    for path in paths(stage, stage)? {
        let destination = output.join(&path);
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::rename(stage.join(&path), destination)?;
    }
    for path in &retired {
        if output.join(path).exists() {
            fs::remove_file(output.join(path))?;
        }
    }
    println!(
        "Refreshed {} battle publications and {} inventories; retired {} files. Receipt: {}/attack-refresh.json",
        publications.len(),
        refreshed.len(),
        retired.len(),
        output.display()
    );
    Ok(())
}
