//! Decode embedded artwork and data without publishing native executable bytes.
mod cabinets;
pub(crate) mod cooking_ui;
mod crafting;
pub(crate) mod defeat_ui;
pub(crate) mod ex_skills;
pub(crate) mod figurine_catalogue;
mod grade_shop;
pub(crate) mod inventory_ui;
pub(crate) mod monster_catalogue;
pub(crate) mod options_ui;
mod record_screen;
pub(crate) mod rename_ui;
pub(crate) mod save_menu;
pub(crate) mod shop_ui;
mod sound_test;
pub(crate) mod status_ui;
pub(crate) mod strategy_ui;
pub(crate) mod synopsis;
pub(crate) mod technique_ui;
pub(crate) mod text;
pub(crate) mod title_catalogue;
pub(crate) mod ui_style;
pub(crate) mod world_map;

use super::geometry;
use crate::{dol, read::u32 as word, tpl, write_atomic};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};

type Sources = BTreeMap<String, Vec<String>>;

/// Shared for every extracted source in one cook. Changed inputs retain their
/// own namespace instead of replacing an earlier source's named publications.
#[derive(Default)]
pub(crate) struct Cache {
    first: BTreeMap<String, String>,
    completed: BTreeMap<(String, String), Sources>,
    pub hits: usize,
}

impl Cache {
    fn cook(
        &mut self,
        name: &str,
        key: String,
        output: &Path,
        cook: impl FnOnce(&Path) -> Result<Sources>,
    ) -> Result<Sources> {
        let identity = (name.to_owned(), key.clone());
        if let Some(sources) = self.completed.get(&identity) {
            self.hits += 1;
            return Ok(sources.clone());
        }
        let first = self
            .first
            .entry(name.to_owned())
            .or_insert_with(|| key.clone());
        let prefix = if *first == key {
            String::new()
        } else {
            format!("variants/{key}/")
        };
        let mut sources = cook(&output.join(&prefix))?;
        for path in sources.values_mut().flatten() {
            path.insert_str(0, &prefix);
        }
        self.completed.insert(identity, sources.clone());
        Ok(sources)
    }
}

fn fingerprint(parts: &[String]) -> Result<String> {
    Ok(crate::digest(&serde_json::to_vec(parts)?))
}

#[test]
fn shared_cook_reuses_sources_and_keeps_changed_dependencies_separate() -> Result<()> {
    let output = crate::temporary_path(&std::env::temp_dir().join("shared-embedded"));
    let mut cache = Cache::default();
    let mut calls = 0;
    let mut results = Vec::new();
    for dependency in ["first", "first", "changed", "changed"] {
        let key = fingerprint(&["same source".into(), dependency.into()])?;
        results.push(cache.cook("files/source.rel", key, &output, |output| {
            calls += 1;
            write_atomic(&output.join("tables/data.json"), dependency.as_bytes())?;
            Ok(Sources::from([(
                "files/source.rel".into(),
                vec!["tables/data.json".into()],
            )]))
        })?);
    }
    assert_eq!((calls, cache.hits), (2, 2));
    assert_eq!(results[0], results[1]);
    assert_eq!(results[2], results[3]);
    let first = &results[0]["files/source.rel"][0];
    let variant = &results[2]["files/source.rel"][0];
    assert_eq!(first, "tables/data.json");
    assert!(variant.starts_with("variants/"));
    assert_eq!(fs::read(output.join(first))?, b"first");
    assert_eq!(fs::read(output.join(variant))?, b"changed");
    fs::remove_dir_all(output)?;
    Ok(())
}

/// Paths relative to the extracted disc's files directory.
pub(crate) fn owns(path: &str) -> bool {
    !path.contains('/') && font_file(path)
}

fn font_file(name: &str) -> bool {
    name.ends_with("fontb0.dat") || name.ends_with("fontb1.dat")
}

/// Source keys are relative to the extracted disc; outputs are relative to output.
fn cook_dol(
    extracted: &Path,
    executable: &[u8],
    title: &crate::scene::title::Recipe,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Sources> {
    let mut sources = Sources::new();
    let mut dol_outputs = Vec::new();
    let dol = extracted.join("sys/main.dol");
    for (family, result) in [
        (
            "arte-catalogue",
            crate::arte::cook(&dol, executable, output),
        ),
        (
            "item-catalogue",
            crate::item::cook(&dol, executable, output),
        ),
        (
            "character-catalogue",
            crate::character_data::cook(&dol, executable, output),
        ),
        (
            "resource-catalogue",
            crate::resource::cook(&dol, executable, output),
        ),
        (
            "movie-directory",
            crate::media::cook_movie_directory(executable, output),
        ),
        (
            "field-effects",
            crate::field_effects::cook_recipe(executable, output),
        ),
        (
            "title-texture-animations",
            crate::texture_animation::cook_title(executable, output),
        ),
        ("title-resources", title.cook(output)),
        ("sound-test", sound_test::cook(&dol, executable, output)),
        ("grade-shop", grade_shop::cook(&dol, executable, output)),
        ("inventory-ui", inventory_ui::cook(&dol, executable, output)),
        ("technique-ui", technique_ui::cook(&dol, executable, output)),
        ("status-ui", status_ui::cook(&dol, executable, output)),
        ("strategy-ui", strategy_ui::cook(&dol, executable, output)),
        ("cooking-ui", cooking_ui::cook(&dol, executable, output)),
        ("options-ui", options_ui::cook(&dol, executable, output)),
        ("ex-skills", ex_skills::cook(&dol, executable, output)),
        ("crafting", crafting::cook(&dol, executable, output)),
        (
            "figurine-catalogue",
            figurine_catalogue::cook(&dol, executable, output),
        ),
        (
            "monster-catalogue",
            monster_catalogue::cook(&dol, executable, output),
        ),
        ("rename-ui", rename_ui::cook(&dol, executable, output)),
        ("synopsis-manual", synopsis::cook(&dol, executable, output)),
        ("world-map", world_map::cook(&dol, executable, output)),
        (
            "title-catalogue",
            title_catalogue::cook(&dol, executable, output),
        ),
        ("save-menu", save_menu::cook(&dol, executable, output)),
        ("defeat-ui", defeat_ui::cook(extracted, executable, output)),
        ("shop-ui", shop_ui::cook(&dol, executable, output)),
        ("ui-style", ui_style::cook(&dol, executable, output)),
        (
            "record-screen",
            record_screen::cook(&dol, executable, output),
        ),
        (
            "credits-resources",
            super::credits::cook_resources(&dol, executable, output),
        ),
    ] {
        match result {
            Ok(paths) => {
                dol_outputs.extend(paths);
                report(&format!("sys/main.dol/{family}"), Ok(()));
            }
            Err(error) => report(&format!("sys/main.dol/{family}"), Err(error)),
        }
    }
    let mut cabinets = cabinets::Scanner::new(output);
    // Seven text sections precede the eleven initialized data sections.
    for section in 7..18 {
        let result = (|| {
            let offset = word(executable, section * 4)? as usize;
            let length = word(executable, 0x90 + section * 4)? as usize;
            if offset == 0 || length == 0 {
                return Ok(());
            }
            let bytes = executable
                .get(offset..offset.checked_add(length).context("DOL range overflow")?)
                .context("DOL data section exceeds file")?;
            scan_tpls(
                bytes,
                &format!("embedded/dol/section-{section}"),
                output,
                &mut dol_outputs,
                report,
            );
            cabinets.scan(
                bytes,
                &format!("embedded/dol/section-{section}"),
                &mut dol_outputs,
                report,
            );
            Ok(())
        })();
        report(&format!("sys/main.dol/data-{section}"), result);
    }
    let result = item_pictures(executable, output, report);
    match result {
        Ok(paths) => dol_outputs.extend(paths),
        Err(error) => report("sys/main.dol/item-pictures", Err(error)),
    }
    match save_artwork(executable, output, report) {
        Ok(paths) => dol_outputs.extend(paths),
        Err(error) => report("sys/main.dol/save-artwork", Err(error)),
    }
    let result = crate::menu::cook_embedded(executable, output);
    if result.is_ok() {
        dol_outputs.push("embedded/menu/tables.json".into());
    }
    report("sys/main.dol/menu", result);
    let result = crate::font::cook_embedded(extracted, output);
    if result.is_ok() {
        dol_outputs.extend([
            "fonts".into(),
            "ui/story-subtitles.json".into(),
            "embedded/dialogue.json".into(),
        ]);
    }
    report("sys/main.dol/dialogue-font-subtitles-and-layout", result);
    sources.insert("sys/main.dol".into(), dol_outputs);
    Ok(sources)
}

/// Source keys are relative to the extraction; returned outputs share one data
/// root, with content-addressed variants only when the dependencies differ.
pub(crate) fn cook(
    extracted: &Path,
    output: &Path,
    cache: &mut Cache,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Sources> {
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let file_root = extracted.join("files");
    let directory = crate::font_directory::Directory::read(&executable)?;
    let startup = crate::field_resources::resolve_path(&file_root, &directory.startup)?;
    let executable_hash = crate::digest(&executable);
    let title = crate::scene::title::Recipe::read(extracted, &executable)?;
    let dol_key = fingerprint(&[
        executable_hash.clone(),
        crate::media::hash_file(&file_root.join(startup))?,
        serde_json::to_string(&title)?,
    ])?;
    let mut sources = cache.cook("dol", dol_key, output, |output| {
        cook_dol(extracted, &executable, &title, output, report)
    })?;
    let palette = crate::digest(
        &directory
            .palette
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    let mut cabinets = cabinets::Scanner::new(output);
    let fonts = directory.paths(&file_root).unwrap_or_else(|error| {
        report("sys/main.dol/font-directory-sources", Err(error));
        Default::default()
    });
    let entries = match fs::read_dir(&file_root) {
        Ok(entries) => entries,
        Err(error) => {
            report("files", Err(error.into()));
            return Ok(sources);
        }
    };
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => files.push(entry.path()),
            Err(error) => report("files/directory-entry", Err(error.into())),
        }
    }
    files.extend(fonts.iter().map(|path| file_root.join(path)));
    files.sort();
    files.dedup();
    for file in files {
        let Some(name) = file.strip_prefix(&file_root).ok().and_then(|n| n.to_str()) else {
            continue;
        };
        let name = name.replace('\\', "/");
        let source = format!("files/{name}");
        if fonts.contains(&name) || owns(&name) {
            let result = (|| {
                let bytes = fs::read(&file)?;
                let key = fingerprint(&[crate::digest(&bytes), palette.clone()])?;
                let group = format!("font/{key}");
                let mut cooked = cache.cook(&group, key.clone(), output, |output| {
                    let paths = font_atlas(&bytes, &key, &executable, output, report)?;
                    Ok(Sources::from([("font".into(), paths)]))
                })?;
                Ok(Sources::from([(
                    source.clone(),
                    cooked.remove("font").unwrap(),
                )]))
            })();
            match result {
                Ok(cooked) => {
                    sources.extend(cooked);
                    report(&source, Ok(()));
                }
                Err(error) => report(&source, Err(error)),
            }
        } else if file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("rel"))
        {
            let bytes = match fs::read(&file) {
                Ok(bytes) => bytes,
                Err(error) => {
                    report(&source, Err(error.into()));
                    continue;
                }
            };
            let key = (|| -> Result<String> {
                // Field unlocks use the executable; archive catalogues export
                // the filenames and sizes declared by this particular module.
                let mut dependencies = vec![crate::digest(&bytes), executable_hash.clone()];
                if let Some(sources) = crate::battle::all::Sources::for_module(&file)? {
                    dependencies.push(serde_json::to_string(&sources)?);
                    for archive in crate::battle::all::Archive::ALL {
                        let source = sources.archive(archive);
                        dependencies.push(serde_json::to_string(&(
                            source,
                            file_root.join(source).metadata()?.len(),
                        ))?);
                    }
                }
                fingerprint(&dependencies)
            })();
            let key = match key {
                Ok(key) => key,
                Err(error) => {
                    report(&source, Err(error));
                    continue;
                }
            };
            let result = cache.cook(&source, key, output, |destination| {
                let mut variant_cabinets = cabinets::Scanner::new(destination);
                let cabinets = if destination == output {
                    &mut cabinets
                } else {
                    &mut variant_cabinets
                };
                let output = destination;
                let mut paths = Vec::new();
                let result = (|| {
                    let count = word(&bytes, 0xc)? as usize;
                    let table = word(&bytes, 0x10)? as usize;
                    ensure!(
                        count <= 256
                            && table
                                .checked_add(count * 8)
                                .is_some_and(|end| end <= bytes.len()),
                        "invalid REL section table"
                    );
                    for section in 0..count {
                        let offset = word(&bytes, table + section * 8)?;
                        let length = word(&bytes, table + section * 8 + 4)? as usize;
                        // Low bit denotes executable text; zero denotes BSS/null.
                        if offset == 0 || offset & 1 != 0 || length == 0 {
                            continue;
                        }
                        let start = (offset & !3) as usize;
                        let result = (|| {
                            let data = bytes
                                .get(
                                    start
                                        ..start
                                            .checked_add(length)
                                            .context("REL range overflow")?,
                                )
                                .context("REL data section exceeds file")?;
                            scan_tpls(
                                data,
                                &format!("embedded/rel/{name}/section-{section}"),
                                output,
                                &mut paths,
                                report,
                            );
                            cabinets.scan(
                                data,
                                &format!("embedded/rel/{name}/section-{section}"),
                                &mut paths,
                                report,
                            );
                            Ok(())
                        })();
                        if result.is_err() {
                            report(&format!("{source}/data-{section}"), result);
                        }
                    }
                    Ok(())
                })();
                if result.is_err() {
                    report(&source, result);
                }
                for (family, result) in [
                    (
                        "sources",
                        crate::battle::all::Sources::publish(&file, output),
                    ),
                    ("entrance", crate::battle::cook_entrance(&file, output)),
                    (
                        "contact-effects",
                        crate::battle::cook_contact_effects(&file, output),
                    ),
                    (
                        "contact-sounds",
                        crate::battle::cook_contact_sounds(&file, output),
                    ),
                    (
                        "actor-tables",
                        crate::battle::cook_actor_tables(&file, output),
                    ),
                    ("placement", crate::battle::cook_placement(&file, output)),
                    ("motion", crate::battle::cook_motion(&file, output)),
                    ("ui-tables", crate::battle::cook_ui_tables(&file, output)),
                    (
                        "messages",
                        crate::battle::cook_message_tables(&file, output),
                    ),
                    (
                        "unison-tables",
                        crate::battle::cook_unison_tables(&file, output),
                    ),
                    (
                        "unison-opener",
                        crate::battle::cook_unison_opener(&file, output),
                    ),
                    (
                        "unison-parameters",
                        crate::battle::cook_unison_parameters(&file, output),
                    ),
                    (
                        "ordinary-spell-parameters",
                        crate::battle::cook_ordinary_spell_parameters(&file, output),
                    ),
                    (
                        "stored-spell-parameters",
                        crate::battle::cook_stored_spell_parameters(&file, output),
                    ),
                    (
                        "elemental-spell-parameters",
                        crate::battle::cook_elemental_spell_parameters(&file, output),
                    ),
                    (
                        "recovery-parameters",
                        crate::battle::cook_recovery_parameters(&file, output),
                    ),
                    (
                        "summon-parameters",
                        crate::battle::cook_summon_parameters(&file, output),
                    ),
                    (
                        "martial-parameters",
                        crate::battle::cook_martial_parameters(&file, output),
                    ),
                    (
                        "enemy-parameters",
                        crate::battle::cook_enemy_parameters(&file, output),
                    ),
                    (
                        "archive-directories",
                        crate::battle::cook_archive_directories(&file, output),
                    ),
                    (
                        "normal-actions",
                        crate::battle::cook_normal_actions(&file, output),
                    ),
                    (
                        "casting-voices",
                        crate::battle::cook_casting_voices(&file, output),
                    ),
                    (
                        "casting-programs",
                        crate::battle::cook_casting_programs(&file, output),
                    ),
                    (
                        "party-settings",
                        crate::battle::all::cook_party_settings(&file, output, report),
                    ),
                    (
                        "victory-groups",
                        crate::battle::cook_victory_groups(&file, output),
                    ),
                    (
                        "long-range-unlocks",
                        super::field_unlocks::cook(&file, &executable, output),
                    ),
                    (
                        "overworld-encounters",
                        super::overworld_encounters::cook(&file, output),
                    ),
                    (
                        "overworld-collision",
                        super::overworld_collision::cook(&file, output),
                    ),
                ] {
                    match result {
                        Ok(Some(outputs)) => {
                            paths.extend(outputs);
                            report(&format!("{source}/{family}"), Ok(()));
                        }
                        Ok(None) => {}
                        Err(error) => report(&format!("{source}/{family}"), Err(error)),
                    }
                }
                Ok(Sources::from([(source.clone(), paths)]))
            });
            match result {
                Ok(cooked) => sources.extend(cooked),
                Err(error) => report(&source, Err(error)),
            }
        }
    }
    Ok(sources)
}

#[test]
#[ignore = "requires original battle modules; no media conversion"]
fn original_normal_action_outputs_deduplicate_module_variants() -> Result<()> {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let output = crate::temporary_path(&std::env::temp_dir().join("normal-action-tables"));
    let mut paths = std::collections::BTreeSet::new();
    for name in [
        "US_r_Top2Btl.rel",
        "r_Top2Btl.rel",
        "Top2BtlD.rel",
        "US_Top2Btl.rel",
        "US_m_Top2Btl.rel",
        "Top2Btl.rel",
        "m_Top2Btl.rel",
    ] {
        let outputs = crate::battle::cook_normal_actions(&files.join(name), &output)?
            .context("missing normal action output")?;
        let path = outputs[0].clone();
        let groups: serde_json::Value = serde_json::from_slice(&fs::read(output.join(&path))?)?;
        assert_eq!(
            groups.as_array().unwrap().len(),
            if name.contains("r_") || name == "Top2BtlD.rel" {
                9
            } else {
                11
            }
        );
        paths.insert(path);
    }
    assert_eq!(paths.len(), 2);
    fs::remove_dir_all(output)?;
    Ok(())
}

fn scan_tpls(
    bytes: &[u8],
    name: &str,
    output: &Path,
    paths: &mut Vec<String>,
    report: &mut impl FnMut(&str, Result<()>),
) {
    for (at, header) in bytes.windows(12).enumerate() {
        if header[..4] != [0, 0x20, 0xaf, 0x30] {
            continue;
        }
        let payload = &bytes[at..];
        // Require real, bounded descriptors before treating a data-section word
        // as an embedded asset. The shared decoder validates every image/page.
        let Ok(textures) = tpl::parse_tpl(payload) else {
            continue;
        };
        if textures.is_empty() || textures.len() > 4096 {
            continue;
        }
        let path = format!("{name}/tpl-{at:x}");
        let mut produced = false;
        geometry::cook(
            payload,
            &path,
            output,
            None,
            geometry::Input::File,
            &mut |child, result| {
                produced |= result.is_ok();
                report(child, result);
            },
        );
        if produced {
            paths.push(path);
        }
    }
}

fn item_pictures(
    executable: &[u8],
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Vec<String>> {
    // fn_800DDAD0 indexes this complete 0x884-byte table; the last image (544)
    // is also an authored menu overlay. Do not stop at the 528 equipment rows.
    let bank = dol::slice(executable, 0x8026bdfc, 0x353dc)?;
    let table = dol::slice(executable, 0x802a11d8, 0x884)?;
    let mut decoded = BTreeMap::<u32, Option<String>>::new();
    let mut aliases = BTreeMap::new();
    for (id, row) in table.chunks_exact(4).enumerate() {
        let offset = word(row, 0)?;
        let entry = decoded.entry(offset).or_insert_with(|| {
            let name = format!("embedded/item-pictures/{offset:x}");
            let result = (|| {
                let header = bank
                    .get(offset as usize..offset as usize + 9)
                    .context("item picture header exceeds bank")?;
                let size = u32::from_le_bytes(header[1..5].try_into()?) as usize + 9;
                let source = bank
                    .get(
                        offset as usize
                            ..(offset as usize)
                                .checked_add(size)
                                .context("item picture overflow")?,
                    )
                    .context("item picture exceeds bank")?;
                let bytes = crate::compression::decode(source)?;
                tpl::parse_tpl(&bytes)?;
                let mut produced = false;
                geometry::cook(
                    &bytes,
                    &name,
                    output,
                    None,
                    geometry::Input::File,
                    &mut |path, result| {
                        produced |= result.is_ok();
                        report(path, result);
                    },
                );
                Ok(produced)
            })();
            match result {
                Ok(true) => Some(name),
                Ok(false) => None,
                Err(error) => {
                    report(&name, Err(error));
                    None
                }
            }
        });
        if let Some(path) = entry {
            aliases.insert(id, path.clone());
        }
    }
    let path = "embedded/item-pictures.json";
    write_atomic(&output.join(path), &serde_json::to_vec(&aliases)?)?;
    let mut paths = decoded.into_values().flatten().collect::<Vec<_>>();
    paths.push(path.into());
    Ok(paths)
}

fn save_artwork_layout(executable: &[u8]) -> Result<Vec<(u32, tpl::TplTexture)>> {
    // The save writer copies a 96x32 banner and one leader-selected 32x32 icon.
    // CARDStat format 2 stores RGB5A3 directly, without a palette.
    let immediate = |address, opcode| -> Result<u32> {
        let instruction = word(dol::slice(executable, address, 4)?, 0)?;
        ensure!(instruction >> 16 == opcode, "unexpected save artwork copy");
        Ok(instruction & 0xffff)
    };
    let state = 0x80221cf8;
    let banner = state + immediate(0x800b3e90, 0x389f)?;
    let banner_size = immediate(0x800b3e94, 0x38a0)?;
    let icons = state + immediate(0x800b3ea0, 0x389f)?;
    let icon_size = immediate(0x800b3ea8, 0x38a0)?;
    ensure!(
        icon_size == 32 * 32 * 2 && banner_size == 96 * 32 * 2,
        "invalid save artwork dimensions"
    );
    let icon_bytes = banner.checked_sub(icons).context("save artwork order")?;
    ensure!(icon_bytes % icon_size == 0, "partial save icon");
    let icon_count = icon_bytes / icon_size;
    ensure!((1..=32).contains(&icon_count), "invalid save icon count");
    Ok((0..=icon_count)
        .map(|index| {
            let is_banner = index == icon_count;
            (
                icons + index * icon_size,
                tpl::TplTexture {
                    width: if is_banner { 96 } else { 32 },
                    height: 32,
                    format: 5,
                    data_offset: 0,
                    palette_offset: None,
                    palette_entries: 0,
                    palette_format: 0,
                    wrap: [0; 2],
                    filter: [1; 2],
                    lod: Default::default(),
                },
            )
        })
        .collect())
}

fn save_artwork(
    executable: &[u8],
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut images = Vec::new();
    let layout = save_artwork_layout(executable)?;
    for (index, (address, descriptor)) in layout.iter().enumerate() {
        let name = if index + 1 == layout.len() {
            "embedded/save-artwork/banner".into()
        } else {
            format!("embedded/save-artwork/icons/{index}")
        };
        let width = u32::from(descriptor.width);
        let height = u32::from(descriptor.height);
        let size = width * height * 2;
        let result = (|| -> Result<_> {
            let pixels =
                tpl::decode_texture(dol::slice(executable, *address, size as usize)?, descriptor)?;
            save_image(&name, width, height, &pixels, output)
        })();
        let path = match result {
            Ok(path) => {
                paths.push(path.clone());
                report(&name, Ok(()));
                Some(path)
            }
            Err(error) => {
                report(&name, Err(error));
                None
            }
        };
        images.push(serde_json::json!({"path":path,"dimensions":[width,height],"source_address":address,"source_size":size}));
    }
    let banner = images.pop().context("missing save banner")?;
    let metadata = "embedded/save-artwork/artwork.json";
    write_atomic(
        &output.join(metadata),
        &serde_json::to_vec(
            &serde_json::json!({"format":"rgb5a3","icons":images,"banner":banner}),
        )?,
    )?;
    paths.push(metadata.into());
    Ok(paths)
}

#[test]
#[ignore = "requires both extracted discs; no media conversion or playback"]
fn original_save_artwork_preserves_icons_and_complete_banner_layout() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut first = None;
    for disc in [1, 2] {
        let executable = fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
        let images = save_artwork_layout(&executable)?;
        assert_eq!(images.len(), 10);
        let mut source_bytes = Vec::new();
        for (index, (address, descriptor)) in images.iter().enumerate() {
            let width = if index == 9 { 96 } else { 32 };
            assert_eq!(
                (usize::from(descriptor.width), descriptor.height),
                (width, 32)
            );
            assert_eq!(*address, 0x80221dbc + index as u32 * 0x800);
            assert!(descriptor.palette_offset.is_none());
            let source = dol::slice(&executable, *address, width * 32 * 2)?;
            source_bytes.extend_from_slice(source);
            let rgba = tpl::decode_texture(source, descriptor)?;
            for y in 0..32 {
                for x in 0..width {
                    // Native RGB5A3 rows are 4x4 blocks across the whole image.
                    // For the banner that is 24 blocks per row, never three 8-block rows.
                    let at = ((y / 4 * (width / 4) + x / 4) * 16 + (y % 4) * 4 + x % 4) * 2;
                    let expected =
                        crate::font::rgb5a3(u16::from_be_bytes([source[at], source[at + 1]]));
                    assert_eq!(&rgba[(y * width + x) * 4..][..4], expected);
                }
            }
        }
        assert_eq!(source_bytes, dol::slice(&executable, 0x80221dbc, 0x6000)?);
        if let Some(expected) = &first {
            assert_eq!(&source_bytes, expected);
        } else {
            first = Some(source_bytes);
        }
    }
    Ok(())
}

fn font_atlas(
    bytes: &[u8],
    name: &str,
    executable: &[u8],
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Vec<String>> {
    // Six packed bytes represent a 24-pixel row. Preserve every physical cell,
    // including the extended banks unused by western dialogue.
    use crate::font_directory::{GLYPH_HEIGHT, ROW_BYTES};
    const PAGE_BYTES: usize = ROW_BYTES * GLYPH_HEIGHT * 36;
    crate::font_directory::validate_size(bytes.len() as u64)?;
    let palette = crate::font_directory::palette(executable)?.map(crate::font::rgb5a3);
    let prefix = format!("embedded/fonts/{name}");
    let mut pages = Vec::new();
    let mut paths = Vec::new();
    for (page, bytes) in bytes.chunks(PAGE_BYTES).enumerate() {
        let height = bytes.len() / ROW_BYTES;
        let mut rgba = Vec::with_capacity(bytes.len() * 16);
        for byte in bytes {
            for shift in [6, 4, 2, 0] {
                rgba.extend_from_slice(&palette[usize::from((byte >> shift) & 3)]);
            }
        }
        let page_name = format!("{prefix}/{page}");
        let result = save_image(&page_name, 384, height as u32, &rgba, output);
        match result {
            Ok(path) => {
                pages.push(serde_json::json!({"texture":path,"width":384,"height":height,"first_glyph":page*576,"glyph_count":height/24*16}));
                paths.push(path);
                report(&page_name, Ok(()));
            }
            Err(error) => report(&page_name, Err(error)),
        }
    }
    let metadata = format!("{prefix}/font.json");
    write_atomic(
        &output.join(&metadata),
        &serde_json::to_vec(
            &serde_json::json!({"glyph_size":[24,24],"columns":16,"glyph_count":bytes.len()/144,"pages":pages}),
        )?,
    )?;
    paths.push(metadata);
    Ok(paths)
}

pub(crate) fn cook_banner(
    bytes: &[u8],
    japanese: bool,
    name: &str,
    output: &Path,
) -> Result<Vec<String>> {
    let languages = match bytes.get(..4) {
        Some(b"BNR1") => 1,
        Some(b"BNR2") => 6,
        _ => anyhow::bail!("invalid GameCube banner header"),
    };
    ensure!(
        bytes.len() >= 0x1820 + languages * 320,
        "truncated banner metadata"
    );
    let image = tpl::TplTexture {
        width: 96,
        height: 32,
        format: 5,
        data_offset: 0x20,
        palette_offset: None,
        palette_entries: 0,
        palette_format: 0,
        wrap: [0; 2],
        filter: [1; 2],
        lod: Default::default(),
    };
    let pixels = tpl::decode_texture(bytes, &image)?;
    let texture = save_image(&format!("{name}/icon"), 96, 32, &pixels, output)?;
    let encoding = if japanese {
        encoding_rs::SHIFT_JIS
    } else {
        encoding_rs::WINDOWS_1252
    };
    let mut comments = Vec::new();
    for language in 0..languages {
        let mut at = 0x1820 + language * 320;
        let mut fields = BTreeMap::new();
        for (key, length) in [
            ("short_title", 32),
            ("short_maker", 32),
            ("long_title", 64),
            ("long_maker", 64),
            ("description", 128),
        ] {
            let field = &bytes[at..at + length];
            let end = field.iter().position(|&b| b == 0).unwrap_or(length);
            let (text, _, invalid) = encoding.decode(&field[..end]);
            ensure!(!invalid, "invalid banner text encoding");
            fields.insert(key, text.into_owned());
            at += length;
        }
        comments.push(fields);
    }
    let metadata = format!("{name}/banner.json");
    write_atomic(
        &output.join(&metadata),
        &serde_json::to_vec(
            &serde_json::json!({"texture":texture,"width":96,"height":32,"comments":comments}),
        )?,
    )?;
    Ok(vec![texture, metadata])
}

fn save_image(name: &str, width: u32, height: u32, rgba: &[u8], output: &Path) -> Result<String> {
    let texture = format!("{name}.ktx2");
    let png = crate::temporary_path(&output.join("intermediate").join(name).with_extension("png"));
    fs::create_dir_all(png.parent().unwrap())?;
    fs::create_dir_all(output.join(&texture).parent().unwrap())?;
    let result = (|| {
        image::save_buffer(&png, rgba, width, height, image::ColorType::Rgba8)?;
        crate::texture::cook_png(&png, &output.join(&texture))
    })();
    let _ = fs::remove_file(png);
    result?;
    Ok(texture)
}
