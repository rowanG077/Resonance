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
type DolPublisher = fn(&Path, &[u8], &Path) -> Result<Vec<String>>;
type RelPublisher = fn(&Path, &Path) -> Result<Option<Vec<String>>>;

/// Table ordinals follow declaration order; keep the enum and its reader in sync.
macro_rules! ordered {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        $vis enum $name { $($variant),* }
        impl $name {
            const ALL: [Self; <[&str]>::len(&[$(stringify!($variant)),*])] = [$(Self::$variant),*];
        }
    };
}
pub(crate) use ordered;

/// Share identical disc resources within this cook; preserve changed variants.
#[derive(Default)]
pub(crate) struct Publications {
    completed: BTreeMap<String, Vec<(String, Sources)>>,
    pub hits: usize,
}

impl Publications {
    fn cook(
        &mut self,
        name: &str,
        key: String,
        output: &Path,
        cook: impl FnOnce(&Path) -> Result<Sources>,
    ) -> Result<Sources> {
        let variants = self.completed.entry(name.to_owned()).or_default();
        if let Some((_, sources)) = variants.iter().find(|(hash, _)| *hash == key) {
            self.hits += 1;
            return Ok(sources.clone());
        }
        let prefix = if variants.is_empty() {
            String::new()
        } else {
            format!("variants/{key}/")
        };
        let mut sources = cook(&output.join(&prefix))?;
        for path in sources.values_mut().flatten() {
            path.insert_str(0, &prefix);
        }
        variants.push((key, sources.clone()));
        Ok(sources)
    }
}

fn fingerprint(parts: &[String]) -> Result<String> {
    Ok(crate::digest(&serde_json::to_vec(parts)?))
}

#[test]
fn shared_cook_reuses_sources_and_keeps_changed_dependencies_separate() -> Result<()> {
    let output = crate::temporary_path(&std::env::temp_dir().join("shared-embedded"));
    let mut cache = Publications::default();
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

pub(crate) struct Catalogues {
    pub menu: crate::menu::Inputs,
    pub resources: crate::resource::Catalogue,
    pub figurines: figurine_catalogue::Catalogue,
    pub monsters: monster_catalogue::Catalogue,
    menu_source: crate::menu::Source,
}

impl Catalogues {
    pub(crate) fn read(executable: &[u8]) -> Result<Self> {
        Ok(Self {
            menu: crate::menu::Inputs::read(executable)?,
            resources: crate::resource::read(executable)?,
            figurines: figurine_catalogue::read(executable)?,
            monsters: monster_catalogue::read(executable)?,
            menu_source: crate::menu::Source::read(executable)?,
        })
    }

    pub(crate) fn field_phases(&self) -> &crate::field_catalogue::Phases {
        &self.menu_source.phases
    }

    pub(crate) fn menu(&self) -> Result<crate::menu::Tables> {
        crate::menu::assemble(&self.menu_source, &self.menu)
    }
}

pub(crate) fn cook_tables(
    dol: &Path,
    executable: &[u8],
    catalogues: &Catalogues,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Vec<String>> {
    let source_hash = crate::digest(executable);
    let module = dol
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid executable filename")?;
    let mut paths = Vec::new();
    let mut publish = |family: &str, result: Result<Vec<String>>| {
        report(
            &format!("sys/main.dol/{family}"),
            result.map(|outputs| paths.extend(outputs)),
        );
    };
    macro_rules! tables {
        ($($family:literal => $table:expr),+ $(,)?) => {
            $(publish($family, crate::embedded::write_source(
                module, &source_hash, output, $family, $table,
            ));)+
        };
    }
    let menu = &catalogues.menu;
    tables! {
        "arte-catalogue" => &menu.arte,
        "item-catalogue" => &menu.items,
        "character-catalogue" => &menu.characters,
        "inventory-ui" => &menu.inventory,
        "technique-ui" => &menu.technique,
        "status-ui" => &menu.status,
        "strategy-ui" => &menu.strategy,
        "cooking-ui" => &menu.cooking,
        "options-ui" => &menu.options,
        "ex-skills" => &menu.ex_skills,
        "rename-ui" => &menu.rename,
        "synopsis-manual" => &menu.synopsis,
        "world-map" => &menu.world,
        "title-catalogue" => &menu.titles,
        "save-menu" => &menu.save,
        "shop-ui" => &menu.shop,
        "ui-style" => &menu.style,
        "resource-catalogue" => &catalogues.resources,
        "figurine-catalogue" => &catalogues.figurines,
        "monster-catalogue" => &catalogues.monsters,
        "field-catalogue" => catalogues.field_phases(),
    }
    let independent: &[(&str, DolPublisher)] = &[
        ("sound-test", sound_test::cook),
        ("grade-shop", grade_shop::cook),
        ("crafting", crafting::cook),
        ("record-screen", record_screen::cook),
        ("credits-resources", super::credits::cook_resources),
    ];
    for &(name, cook) in independent {
        publish(name, cook(dol, executable, output));
    }
    paths.sort();
    Ok(paths)
}

#[test]
#[ignore = "requires an extracted executable; no media conversion"]
fn failed_table_publication_does_not_block_other_catalogues() -> Result<()> {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol");
    let executable = fs::read(&file)?;
    let temporary = tempfile::tempdir()?;
    let output = temporary.path();
    fs::create_dir(output.join("embedded"))?;
    // Force just the item publisher to fail, without corrupting other inputs.
    fs::write(output.join("embedded/item-catalogue"), b"not a directory")?;
    let mut failures = BTreeMap::new();
    let catalogues = Catalogues::read(&executable)?;
    let paths = cook_tables(
        &file,
        &executable,
        &catalogues,
        output,
        &mut |label, result| {
            if let Err(error) = result {
                failures.insert(label.to_owned(), format!("{error:#}"));
            }
        },
    )?;
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(failures.contains_key("sys/main.dol/item-catalogue"));
    catalogues.menu()?;
    assert!(
        paths
            .iter()
            .any(|path| path == "embedded/resource-catalogue/main.dol.json")
    );
    assert!(paths.iter().all(|path| output.join(path).is_file()));
    Ok(())
}

fn cook_dol(
    extracted: &Path,
    executable: &[u8],
    catalogues: &Catalogues,
    title: &crate::scene::title::Recipe,
    output: &Path,
    report: &mut impl FnMut(&str, Result<()>),
) -> Result<Sources> {
    let mut sources = Sources::new();
    let mut dol_outputs = cook_tables(
        &extracted.join("sys/main.dol"),
        executable,
        catalogues,
        output,
        report,
    )?;
    for (family, result) in [
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
        ("defeat-ui", defeat_ui::cook(extracted, executable, output)),
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
    catalogues: &Catalogues,
    executable: &[u8],
    publications: &mut Publications,
    report: &mut impl FnMut(&str, Result<()>),
    source_hashes: &BTreeMap<String, String>,
) -> Result<Sources> {
    let file_root = extracted.join("files");
    let directory = crate::font_directory::Directory::read(executable)?;
    let startup = crate::field_resources::resolve_path(&file_root, &directory.startup)?;
    let executable_hash = crate::digest(executable);
    let title = crate::scene::title::Recipe::read(extracted, executable)?;
    let dol_key = fingerprint(&[
        executable_hash.clone(),
        crate::media::hash_file(&file_root.join(startup))?,
        serde_json::to_string(&title)?,
    ])?;
    let mut sources = publications.cook("dol", dol_key, output, |output| {
        cook_dol(extracted, executable, catalogues, &title, output, report)
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
                let mut cooked = publications.cook(&group, key.clone(), output, |output| {
                    let paths = font_atlas(&bytes, &key, executable, output, report)?;
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
            let tiles = match super::overworld_encounters::tiles::read(&file, source_hashes) {
                Ok(tiles) => tiles,
                Err(error) => {
                    report(&format!("{source}/overworld-tiles"), Err(error));
                    continue;
                }
            };
            // Native tables depend on the executable; tile bindings also depend
            // on the selected physical packages, which may differ across discs.
            let key = fingerprint(&[
                crate::digest(&bytes),
                executable_hash.clone(),
                crate::digest(&serde_json::to_vec(&tiles)?),
            ]);
            let key = match key {
                Ok(key) => key,
                Err(error) => {
                    report(&source, Err(error));
                    continue;
                }
            };
            let result = publications.cook(&source, key, output, |destination| {
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
                let publishers: &[(&str, RelPublisher)] = &[
                    ("overworld-encounters", super::overworld_encounters::cook),
                    ("overworld-collision", super::overworld_collision::cook),
                ];
                for (family, result) in [
                    (
                        "long-range-unlocks",
                        super::field_unlocks::cook(&file, executable, output),
                    ),
                    (
                        "overworld-tiles",
                        tiles
                            .as_ref()
                            .map(|tiles| {
                                crate::embedded::write(&file, output, "overworld-tiles", tiles)
                            })
                            .transpose(),
                    ),
                ]
                .into_iter()
                .chain(
                    publishers
                        .iter()
                        .map(|&(family, cook)| (family, cook(&file, output))),
                ) {
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
            &mut crate::scene::decoded::Package::default(),
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
    let pictures = crate::item::Pictures::read(executable)?;
    let mut decoded = BTreeMap::<u32, Option<String>>::new();
    let mut aliases = BTreeMap::new();
    for (id, offset) in pictures.offsets().enumerate() {
        let entry = decoded.entry(offset).or_insert_with(|| {
            let name = format!("embedded/item-pictures/{offset:x}");
            let result = (|| {
                let bytes = pictures.decode(offset)?;
                tpl::parse_tpl(&bytes)?;
                let mut produced = false;
                geometry::cook(
                    &bytes,
                    &name,
                    output,
                    None,
                    geometry::Input::File,
                    &mut crate::scene::decoded::Package::default(),
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
    crate::texture::cook(width, height, rgba, &output.join(&texture))?;
    Ok(texture)
}
