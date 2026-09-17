//! Physical resources convert independently of events, actors and runtime support.
use crate::scene::recovered::RecoveredModels;
#[path = "skin.rs"]
mod skin;
#[path = "transform.rs"]
mod transform;

use crate::{
    compression, compression::member as compressed, field::sections, media::library as audio,
    read::u32 as word, write_atomic,
};
use anyhow::{Context, Result, ensure};
#[cfg(test)]
use std::fs;
use std::{
    io::{Cursor, Read},
    path::Path,
};

const MAX_EXPANDED: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    File,
    /// Native directories can bound animation clips before their name tables.
    Member,
    /// A catalogue-declared field has no unique format magic.
    Field,
}

pub(crate) fn is_model(bytes: &[u8]) -> bool {
    matches!(word(bytes, 0x20).ok(), Some(0x005b_bc61 | 0x00b7_49e0))
}

#[derive(serde::Serialize)]
struct SkeletonNode {
    name: String,
    parent: Option<usize>,
    geometry: usize,
    node_id: u16,
    /// One inherits the parent transform; other kinds use model space.
    transform_kind: u8,
    draw_priority: u8,
    /// The constructor copies the high byte into the transform metadata.
    node_flags: u16,
    transform: Option<transform::Transform>,
}

#[derive(serde::Serialize)]
struct ModelMetadata<'a> {
    source_size: usize,
    name: Option<&'a str>,
    root_geometry: Option<u16>,
    name_table_metadata: u32,
    unknown_04: u16,
    unknown_08: u32,
    unknown_16: u16,
    /// Blob-relative storage outside the decoded structures; its meaning is unknown.
    unreferenced_storage: Vec<Storage<'a>>,
}

#[derive(serde::Serialize)]
struct ModelContainer<'a> {
    source_size: usize,
    /// Wrapper-relative extents; model.json offsets are relative to this skeleton.
    skeleton: Option<std::ops::Range<usize>>,
    skin: Option<std::ops::Range<usize>>,
    /// Nonzero gaps retain their complete bytes; other uncovered bytes are zero.
    unreferenced_storage: Vec<Storage<'a>>,
}

#[derive(serde::Serialize)]
struct Storage<'a> {
    offset: usize,
    bytes: &'a [u8],
}

fn storage<'a>(bytes: &'a [u8], ranges: &[std::ops::Range<usize>]) -> Result<Vec<Storage<'a>>> {
    ranges
        .iter()
        .map(|range| {
            Ok(Storage {
                offset: range.start,
                bytes: bytes
                    .get(range.clone())
                    .context("unreferenced model storage exceeds its source")?,
            })
        })
        .collect()
}

fn skeleton_nodes(model: &crate::model::Model) -> Result<Vec<SkeletonNode>> {
    crate::geometry::model_node_info(model)
        .into_iter()
        .zip(&model.nodes)
        .map(|(node, source)| {
            Ok(SkeletonNode {
                name: node.name,
                parent: source.parent,
                geometry: node.object_index,
                node_id: source.node_id,
                transform_kind: source.transform_kind,
                draw_priority: source.draw_priority,
                node_flags: source.flags,
                transform: transform::Transform::read(&source.data_words)?,
            })
        })
        .collect()
}

/// Model header selectors bind controller slots to TPL image indices. Slot3
/// belongs to the separate outline palette and has no primary-header byte.
fn texture_selectors(bytes: &[u8], image_count: u32) -> Result<[Option<u8>; 14]> {
    let header = bytes.get(0x0c..0x19).context("truncated model selectors")?;
    let mut slots = [None; 14];
    for (index, &value) in header.iter().enumerate() {
        slots[index + usize::from(index >= 3)] = value.checked_sub(1);
    }
    // Caller-bound palettes have no local images; retain their index bindings.
    ensure!(
        image_count == 0
            || slots
                .iter()
                .flatten()
                .all(|&index| u32::from(index) < image_count),
        "model selector exceeds TPL image count"
    );
    Ok(slots)
}

pub(crate) fn model_selectors(bytes: &[u8], image_count: u32) -> Result<[Option<u8>; 14]> {
    let end = if word(bytes, 4)? == 0 && word(bytes, 8)? == 0 {
        bytes.len()
    } else {
        crate::geometry::skeleton_range(bytes)?.end
    };
    if skin::model(bytes, end)?.is_some() {
        Ok([None; 14])
    } else {
        texture_selectors(bytes, image_count)
    }
}

/// Names are relative output namespaces. Every failed child is reported and siblings continue.
/// False means the root is not a recognized geometry/archive resource.
pub(crate) fn cook<F: FnMut(&str, Result<()>)>(
    bytes: &[u8],
    name: &str,
    output: &Path,
    audio: Option<&audio::Cooker>,
    input: Input,
    report: &mut F,
) -> bool {
    Walker {
        output,
        audio,
        report,
        recovered: None,
    }
    .visit(bytes, name, None, 0, input)
}

struct Walker<'a, 'r, F> {
    output: &'a Path,
    audio: Option<&'a audio::Cooker>,
    report: &'r mut F,
    recovered: Option<&'r mut RecoveredModels>,
}

pub(crate) fn cook_recovered(
    bytes: &[u8],
    name: &str,
    output: &Path,
    audio: Option<&audio::Cooker>,
    input: Input,
    recovered: &mut RecoveredModels,
    report: &mut impl FnMut(&str, Result<()>),
) -> bool {
    Walker {
        output,
        audio,
        report,
        recovered: Some(recovered),
    }
    .visit(bytes, name, None, 0, input)
}

impl<F: FnMut(&str, Result<()>)> Walker<'_, '_, F> {
    fn visit(
        &mut self,
        bytes: &[u8],
        name: &str,
        palette: Option<&[u8]>,
        depth: u8,
        input: Input,
    ) -> bool {
        if let Err(error) = resonance_content::validate_asset_path(name) {
            (self.report)(name, Err(error));
            return true;
        }
        if depth > 16 {
            (self.report)(
                name,
                Err(anyhow::anyhow!("archive nesting exceeds 16 levels")),
            );
            return true;
        }
        // Compression preserves the declared role. Once unwrapped, a field
        // must validate as a field even if its optional script is absent.
        if input == Input::Field && !bytes.starts_with(b"MSCF") {
            let result = if let Some(payload) = compressed(bytes) {
                compression::decode(payload)
                    .map(|expanded| self.member(&expanded, name, palette, depth + 1, input))
            } else {
                self.map(bytes, name, depth)
            };
            if result.is_err() {
                (self.report)(name, result);
            }
            return true;
        }
        if audio::is_stream(bytes) {
            let result = (|| {
                let paths = self
                    .audio
                    .context("embedded audio decoder is unavailable")?
                    .cook_embedded_stream(bytes, name)?;
                self.json(name, "voice", &paths)
            })();
            (self.report)(name, result);
            return true;
        }
        if bytes.is_empty() || bytes.iter().all(|&byte| byte == 0) {
            let result = self.json(
                name,
                "empty",
                &serde_json::json!({"zero_bytes": bytes.len()}),
            );
            (self.report)(name, result);
            return true;
        }
        if bytes.starts_with(b"CAMM") {
            let result =
                super::field::camera(bytes).and_then(|track| self.json(name, "camera", &track));
            (self.report)(name, result);
            return true;
        }
        if bytes.starts_with(b"MSCF") {
            let result = self.cabinet(bytes, name, depth, input);
            if result.is_err() {
                (self.report)(name, result);
            }
            return true;
        }
        if is_model(bytes) {
            let result = self.model(bytes, name, palette);
            (self.report)(name, result);
            return true;
        }
        match word(bytes, 0).ok() {
            Some(0x0020_af30) => {
                if let Err(error) = self.textures(bytes, name) {
                    (self.report)(name, Err(error));
                }
                return true;
            }
            Some(0x007b_7960) => {
                let result = if crate::animation::is_animation(bytes) {
                    // Native member tables can delimit a clip before its
                    // removed name table. A whole file must still be complete.
                    let decode = || match input {
                        Input::File => crate::animation::decode(bytes),
                        Input::Member => crate::animation::read_member(bytes),
                        Input::Field => unreachable!("field inputs were dispatched above"),
                    };
                    match &mut self.recovered {
                        Some(recovered) => recovered.decode_animation(bytes, decode),
                        None => decode().map(std::sync::Arc::new),
                    }
                    .and_then(|motion| self.json(name, "animation", motion.as_ref()))
                } else {
                    self.skeleton(bytes, name)
                };
                (self.report)(name, result);
                return true;
            }
            Some(0x005b_bc61 | 0x00b7_49e0) => {
                (self.report)(
                    name,
                    Err(anyhow::anyhow!(
                        "standalone geometry needs its texture resource"
                    )),
                );
                return true;
            }
            _ => {}
        }
        if let Some(payload) = compressed(bytes) {
            match compression::decode(payload) {
                Ok(expanded) => self.member(&expanded, name, palette, depth + 1, input),
                Err(error) => (self.report)(name, Err(error)),
            }
            return true;
        }
        if let Some(entries) = super::archive::entries(bytes) {
            let directory = super::archive::Directory::new(&entries);
            for (index, range) in entries.iter().enumerate() {
                let Some(range) = range else {
                    continue;
                };
                if directory.members[index] == Some(index) {
                    self.member(
                        &bytes[range.clone()],
                        &format!("{name}/{index}"),
                        None,
                        depth + 1,
                        Input::Member,
                    );
                }
            }
            let result = self
                .json(name, "archive", &directory)
                .and_then(|_| super::archive::padding(bytes, 4 + entries.len() * 8, &entries));
            (self.report)(name, result);
            return true;
        }
        match super::cook_script(bytes, name, self.output) {
            Ok(false) => {}
            result => {
                (self.report)(name, result.map(|_| ()));
                return true;
            }
        }
        if let Ok(ranges) = sections(bytes) {
            if super::field::is_map(bytes, &ranges) {
                let result = self.map(bytes, name, depth);
                if result.is_err() {
                    (self.report)(name, result);
                }
                return true;
            }
            let model_at = |index: usize| {
                ranges
                    .get(index)
                    .and_then(Option::as_ref)
                    .is_some_and(|range| is_model(&bytes[range.clone()]))
            };
            let actor = ranges.len() == 31 && model_at(0);
            let terrain =
                ranges.len() == 5 && model_at(0) && ranges[1].is_none() && ranges[3].is_none();
            let attachment = (5..=7).contains(&ranges.len())
                && ranges[0].as_ref().is_some_and(|r| r.len() == 64)
                && model_at(1);
            // A secondary outline may borrow its sole primary sibling's texture table.
            // Multiple candidates remain unresolved rather than selecting an arbitrary palette.
            let mut primaries = ranges.iter().flatten().filter_map(|range| {
                let part = &bytes[range.clone()];
                (word(part, 0).is_ok_and(|offset| offset > 0) && is_model(part)).then_some(part)
            });
            let first = primaries.next();
            let palette = if primaries.next().is_none() {
                first.or(palette)
            } else {
                palette
            };
            for (index, range) in ranges.iter().enumerate() {
                if let Some(range) = range {
                    if (actor && crate::character::collision_slot(index, &bytes[range.clone()]))
                        || (terrain && index == 4)
                    {
                        let child = format!("{name}/{index}");
                        let format = if actor {
                            crate::field::collision_data::Format::Short
                        } else {
                            crate::field::collision_data::Format::Detect
                        };
                        let result = self.collision(&bytes[range.clone()], &child, format);
                        (self.report)(&child, result);
                        continue;
                    }
                    if index == 0 && attachment {
                        let child = format!("{name}/{index}");
                        let result = super::attachment::decode(&bytes[range.clone()])
                            .and_then(|recipe| self.json(&child, "attachment", &recipe));
                        (self.report)(&child, result);
                        continue;
                    }
                    self.member(
                        &bytes[range.clone()],
                        &format!("{name}/{index}"),
                        palette,
                        depth + 1,
                        Input::Member,
                    );
                }
            }
            let result = self.directory(bytes, name, &ranges);
            (self.report)(name, result);
            return true;
        }
        false
    }

    fn member(
        &mut self,
        bytes: &[u8],
        name: &str,
        palette: Option<&[u8]>,
        depth: u8,
        input: Input,
    ) {
        if !self.visit(bytes, name, palette, depth, input) {
            (self.report)(
                name,
                Err(anyhow::anyhow!(
                    "unrecognized physical member ({} bytes)",
                    bytes.len()
                )),
            );
        }
    }

    fn cabinet(&mut self, bytes: &[u8], name: &str, depth: u8, input: Input) -> Result<()> {
        let mut cabinet = cab::Cabinet::new(Cursor::new(bytes))?;
        let names: Vec<_> = cabinet
            .folder_entries()
            .flat_map(|folder| folder.file_entries())
            .map(|entry| entry.name().to_owned())
            .collect();
        let members = names
            .iter()
            .map(|entry| entry.replace('\\', "/"))
            .collect::<Vec<_>>();
        for member in &members {
            resonance_content::validate_asset_path(member)?;
        }
        self.json(name, "cabinet", &members)?;
        for entry in names {
            let child = format!("{name}/{}", entry.replace('\\', "/"));
            let result = (|| -> Result<Vec<u8>> {
                resonance_content::validate_asset_path(&child)?;
                let mut expanded = Vec::new();
                cabinet
                    .read_file(&entry)?
                    .take(MAX_EXPANDED + 1)
                    .read_to_end(&mut expanded)?;
                ensure!(
                    expanded.len() as u64 <= MAX_EXPANDED,
                    "expanded member exceeds 64 MiB"
                );
                Ok(expanded)
            })();
            match result {
                Ok(expanded) => self.member(
                    &expanded,
                    &child,
                    None,
                    depth + 1,
                    if input == Input::Field {
                        Input::Field
                    } else {
                        Input::File
                    },
                ),
                Err(error) => (self.report)(&child, Err(error)),
            }
        }
        Ok(())
    }

    fn map(&mut self, bytes: &[u8], name: &str, depth: u8) -> Result<()> {
        let ranges = sections(bytes)?;
        ensure!(
            ranges.len() >= super::field::REQUIRED_SECTIONS,
            "field header omits required section slots"
        );
        for (index, range) in ranges.iter().enumerate() {
            let Some(range) = range else { continue };
            let part = &bytes[range.clone()];
            let child = format!("{name}/{index}");
            let result = match index {
                4 | 5 => self.collision(part, &child, crate::field::collision_data::Format::Detect),
                6 => super::cook_script(part, &child, self.output).and_then(|recognized| {
                    ensure!(recognized, "invalid field script header");
                    Ok(())
                }),
                7 => super::field::models(part).and_then(|models| {
                    self.json(&child, "models", &models)?;
                    let ids = (word(part, 4)? & !3) as usize;
                    let mut consumed: Vec<_> = models
                        .iter()
                        .map(|model| Some(model.range.clone()))
                        .collect();
                    if ids != 0 {
                        consumed.push(Some(ids..ids + models.len() * 2));
                    }
                    for model in &models {
                        self.member(
                            &part[model.range.clone()],
                            &format!("{child}/{}", model.archive_entry),
                            None,
                            depth + 1,
                            Input::Member,
                        );
                    }
                    super::archive::padding(part, 8 + models.len() * 4, &consumed)
                }),
                _ => {
                    self.member(part, &child, None, depth + 1, Input::Member);
                    continue;
                }
            };
            (self.report)(&child, result);
        }
        let result = self
            .json(name, "members", &super::FieldDirectory::new(bytes, &ranges))
            .and_then(|_| super::archive::padding(bytes, 4 + ranges.len() * 4, &ranges));
        (self.report)(name, result);
        Ok(())
    }

    fn collision(
        &self,
        bytes: &[u8],
        name: &str,
        format: crate::field::collision_data::Format,
    ) -> Result<()> {
        let mesh = crate::field::collision_data::Mesh::read(bytes, format)?;
        self.json(name, "collision", &mesh.groups)?;
        self.json(name, "collision-metadata", &mesh.metadata)
    }

    fn directory(
        &self,
        bytes: &[u8],
        name: &str,
        ranges: &[Option<std::ops::Range<usize>>],
    ) -> Result<()> {
        self.json(name, "members", &super::archive::Directory::new(ranges))?;
        super::archive::padding(bytes, 4 + ranges.len() * 4, ranges)
    }

    fn model(&mut self, bytes: &[u8], name: &str, palette: Option<&[u8]>) -> Result<()> {
        // Decode physical suffixes before palette normalization shifts offsets.
        let skeleton = if word(bytes, 4)? == 0 && word(bytes, 8)? == 0 {
            None
        } else {
            Some(crate::geometry::skeleton_range(bytes)?)
        };
        let end = skeleton.as_ref().map_or(bytes.len(), |range| range.end);
        if let Some(range) = skeleton.as_ref().filter(|range| !range.is_empty())
            && let Err(error) = self.skeleton(&bytes[range.clone()], name)
        {
            (self.report)(&format!("{name}/skeleton"), Err(error));
        }
        let skin = skin::model(bytes, end)?;
        self.model_container(
            bytes,
            name,
            skeleton,
            skin.as_ref().map(|(_, range)| range.clone()),
        )?;
        let normalized = crate::character::texture_palette(palette.unwrap_or(bytes), bytes)?;
        // Texture recovery must not depend on successful mesh/material conversion.
        let tpl = normalized
            .get(word(&normalized, 0)? as usize..word(&normalized, 4)? as usize)
            .context("model texture resource exceeds container")?;
        let selectors = skin
            .is_none()
            .then(|| texture_selectors(bytes, word(tpl, 4)?))
            .transpose()?;
        let mut recovered_textures = self
            .recovered
            .as_ref()
            .map(|recovered| recovered.decode_textures(tpl))
            .transpose()?;
        if let Some(decoded) = &mut recovered_textures {
            let name = format!("{name}/palettes");
            let catalogue = match std::sync::Arc::get_mut(decoded) {
                Some(decoded) => decoded.publish_as(self.output, &name, &mut self.report)?,
                None => decoded.publish_alias(self.output, &name, &mut self.report)?,
            };
            self.json(&name, "textures", &catalogue)?;
            (self.report)(&format!("{name}/textures"), Ok(()));
        } else {
            self.textures(tpl, &format!("{name}/palettes"))?;
        }
        let textures = if word(bytes, 0)? == 0 && word(tpl, 4)? == 0 {
            super::physical_scene::TextureSource::Caller
        } else {
            super::physical_scene::TextureSource::Local {
                catalogue: format!("{name}/palettes/textures.json"),
            }
        };
        // A malformed secondary mip/palette still has its own diagnostic; preserve
        // the physical reader's independent base-image geometry recovery.
        let alpha = recovered_textures
            .as_ref()
            .and_then(|textures| textures.base_alpha().ok());
        if let Some(model) = self
            .recovered
            .as_ref()
            .and_then(|recovered| recovered.get(&normalized))
        {
            let scene = super::physical_scene::Scene {
                textures,
                ..model.geometry.scene.clone()
            };
            if let Some(mesh) = &model.mesh {
                mesh.share(&self.output.join(&scene.mesh))?;
            }
            self.nodes(name, &scene.bone_names)?;
            self.json(name, "scene", &scene)?;
        } else {
            let (decoded, publication) =
                super::physical_scene::cook_decoded(&normalized, self.output, textures, alpha)?;
            self.nodes(name, &decoded.scene.bone_names)?;
            self.json(name, "scene", &decoded.scene)?;
            if let Some(recovered) = &mut self.recovered {
                recovered.remember(
                    &normalized,
                    decoded,
                    recovered_textures.unwrap(),
                    Some(publication),
                );
            }
        }
        if let Some((recipe, _)) = &skin {
            self.json(name, "skin", recipe)?;
        } else if let Some(selectors) = selectors.filter(|slots| slots.iter().any(Option::is_some))
        {
            self.json(
                name,
                "texture-selectors",
                &serde_json::json!({
                    "texture_indices_by_slot": selectors,
                }),
            )?;
        }
        Ok(())
    }

    fn model_container(
        &self,
        bytes: &[u8],
        name: &str,
        skeleton: Option<std::ops::Range<usize>>,
        skin: Option<std::ops::Range<usize>>,
    ) -> Result<()> {
        let mut covered = vec![0..skeleton.as_ref().map_or(bytes.len(), |range| range.end)];
        if let Some(range) = &skin {
            covered.push(range.clone());
        }
        let unused = crate::read::unreferenced_ranges(bytes, covered);
        self.json(
            name,
            "model-container",
            &ModelContainer {
                source_size: bytes.len(),
                skeleton,
                skin,
                unreferenced_storage: storage(bytes, &unused)?,
            },
        )
    }

    fn skeleton(&self, bytes: &[u8], name: &str) -> Result<()> {
        let model = crate::model::Model::parse(bytes)?;
        let nodes = skeleton_nodes(&model)?;
        self.json(name, "skeleton", &nodes)?;
        self.nodes(
            name,
            &nodes.into_iter().map(|node| node.name).collect::<Vec<_>>(),
        )?;
        self.json(
            name,
            "motion-bindings",
            &crate::animation::ModelBindings::from_model(&model),
        )?;
        self.json(
            name,
            "model",
            &ModelMetadata {
                source_size: bytes.len(),
                name: model.name.as_deref(),
                root_geometry: (model.root_geometry != u16::MAX).then_some(model.root_geometry),
                name_table_metadata: model.name_table_metadata,
                unknown_04: model.field4,
                unknown_08: model.field8,
                unknown_16: model.field16,
                unreferenced_storage: storage(bytes, &model.unreferenced_ranges)?,
            },
        )
    }

    fn nodes(&self, name: &str, bones: &[String]) -> Result<()> {
        let Some(path) = super::physical_scene::publish_nodes(self.output, bones)? else {
            return Ok(());
        };
        let module = path
            .trim_start_matches("scripts/")
            .trim_end_matches(".sym")
            .replace('/', "::");
        let root = "../".repeat(name.split('/').count());
        write_atomic(
            &self.output.join(name).join("nodes.md"),
            format!("Model `{name}` uses [{}]({root}{path}).\n\nImport with `use {module};`. The source lists each node's original name and ordinal.\n", module).as_bytes(),
        )
    }

    fn json(&self, name: &str, kind: &str, value: &impl serde::Serialize) -> Result<()> {
        write_atomic(
            &self.output.join(format!("{name}/{kind}.json")),
            &serde_json::to_vec(value)?,
        )
    }

    fn textures(&mut self, bytes: &[u8], name: &str) -> Result<()> {
        let catalogue = if let Some(recovered) = &mut self.recovered {
            let mut decoded = recovered.decode_textures(bytes)?;
            let catalogue = match std::sync::Arc::get_mut(&mut decoded) {
                Some(decoded) => decoded.publish_as(self.output, name, &mut self.report)?,
                None => decoded.publish_alias(self.output, name, &mut self.report)?,
            };
            recovered.remember_textures(decoded);
            catalogue
        } else {
            crate::texture::cook_catalogue(bytes, name, self.output, &mut self.report)?
        };
        self.json(name, "textures", &catalogue)?;
        (self.report)(&format!("{name}/textures"), Ok(()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabinet_members_dispatch_by_structure_and_preserve_declared_field_roles() -> Result<()> {
        use std::io::Write;
        let cabinet = |members: &[(&str, &[u8])]| -> Result<Vec<u8>> {
            let mut builder = cab::CabinetBuilder::new();
            let folder = builder.add_folder(cab::CompressionType::None);
            for (name, _) in members {
                folder.add_file(*name);
            }
            let mut writer = builder.build(Cursor::new(Vec::new()))?;
            let mut index = 0;
            while let Some(mut file) = writer.next_file()? {
                file.write_all(members[index].1)?;
                index += 1;
            }
            Ok(writer.finish()?.into_inner())
        };
        let field = |script: bool| {
            let mut bytes = vec![0; 68];
            bytes[..4].copy_from_slice(&16_u32.to_be_bytes());
            if script {
                bytes[28..32].copy_from_slice(&68_u32.to_be_bytes());
                bytes.extend([0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff, 0, 0]);
            }
            let bank = bytes.len() as u32;
            bytes[32..36].copy_from_slice(&bank.to_be_bytes());
            bytes.extend([1_u32, 0].map(u32::to_be_bytes).concat());
            bytes
        };
        let output = crate::temporary_path(&std::env::temp_dir().join("generic-cabinet"));
        let run = |bytes: &[u8], name: &str, input| {
            let mut errors = Vec::new();
            assert!(cook(bytes, name, &output, None, input, &mut |_, result| {
                if let Err(error) = result {
                    errors.push(error);
                }
            }));
            errors
        };
        let result = (|| -> Result<()> {
            let mut compact = vec![0; 32];
            compact[8..12].fill(255);
            for at in (12..28).step_by(4) {
                compact[at..at + 4].copy_from_slice(&28u32.to_be_bytes());
            }
            let bytes = cabinet(&[("retained.record", &compact)])?;
            let errors = run(&bytes, "actions", Input::File);
            assert_eq!(errors.len(), 1);
            assert!(!output.join("actions/retained.record/actions.json").exists());

            let source = field(true);
            let bytes = cabinet(&[("renamed.data", &source), ("other.payload", &[0; 8])])?;
            let errors = run(&bytes, "arbitrary", Input::File);
            assert!(errors.is_empty(), "{errors:?}");
            assert!(output.join("arbitrary/renamed.data/6/script.ssb").is_file());
            assert!(
                output
                    .join("arbitrary/renamed.data/7/models.json")
                    .is_file()
            );
            assert!(output.join("arbitrary/other.payload/empty.json").is_file());

            let bytes = cabinet(&[("scriptless.data", &field(false))])?;
            let mut packed = vec![0];
            packed.extend([bytes.len() as u32; 2].map(u32::to_le_bytes).concat());
            packed.extend(bytes);
            let errors = run(&packed, "declared", Input::Field);
            assert!(errors.is_empty(), "{errors:?}");
            assert!(
                output
                    .join("declared/scriptless.data/7/models.json")
                    .is_file()
            );

            let mut broken = source;
            broken[76..78].fill(255);
            let bytes = cabinet(&[("broken.data", &broken)])?;
            let errors = run(&bytes, "damaged", Input::File);
            assert_eq!(
                errors.len(),
                1,
                "damaged script must fail without blocking its siblings"
            );
            assert!(!output.join("damaged/broken.data/6/script.ssb").exists());
            assert!(output.join("damaged/broken.data/7/models.json").is_file());

            let errors = run(&[0; 8], "invalid", Input::Field);
            assert_eq!(
                errors.len(),
                1,
                "declared fields cannot fall back to empty data"
            );
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }

    #[test]
    fn standalone_skeleton_keeps_ids_inheritance_priority_and_flags() -> Result<()> {
        let mut bytes = vec![0; 140];
        for (at, value) in [
            (0, 0x007b7960_u32),
            (12, 32),
            (32, 88),
            (48, 60),
            (60, 88),
            (88, 0x08000000),
            (120, 2_f32.to_bits()),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        bytes[6..8].copy_from_slice(&2_u16.to_be_bytes());
        for (index, id, kind, priority, flags) in
            [(0, 7_u16, 1, 9, 0x4105_u16), (1, 13, 2, 2, 0x0801)]
        {
            let at = 32 + index * 28;
            bytes[at + 20..at + 22].copy_from_slice(&u16::MAX.to_be_bytes());
            bytes[at + 22..at + 24].copy_from_slice(&id.to_be_bytes());
            bytes[at + 24] = kind;
            bytes[at + 25] = priority;
            bytes[at + 26..at + 28].copy_from_slice(&flags.to_be_bytes());
        }
        let nodes = skeleton_nodes(&crate::model::Model::parse(&bytes)?)?;
        assert_eq!(
            nodes.iter().map(|node| node.node_id).collect::<Vec<_>>(),
            [7, 13]
        );
        assert_eq!(nodes[1].parent, Some(0));
        assert_eq!((nodes[0].transform_kind, nodes[1].transform_kind), (1, 2));
        assert_eq!((nodes[0].draw_priority, nodes[1].draw_priority), (9, 2));
        assert_eq!((nodes[0].node_flags, nodes[1].node_flags), (0x4105, 0x0801));
        let transform = serde_json::to_value(&nodes[0].transform)?;
        assert_eq!(transform["flags"], 8);
        assert_eq!(
            transform["translation"]["value"],
            serde_json::json!([2., 0., 0.])
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; no mesh or texture conversion"]
    fn original_standalone_skeleton_export_preserves_native_node_metadata() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("standalone-skeleton"));
        for disc in ["disc1", "disc2"] {
            let package = fs::read(root.join(disc).join("files/lloyd000.bin"))?;
            let primary = sections(&package)?[0]
                .clone()
                .context("missing primary model")?;
            let model = &package[primary];
            let bytes = &model[crate::geometry::skeleton_range(model)?];
            let authored = crate::model::Model::parse(bytes)?;
            assert_eq!(authored.nodes.len(), 83);
            let mut reports = 0;
            assert!(cook(
                bytes,
                disc,
                &output,
                None,
                Input::File,
                &mut |_, result| {
                    result.unwrap();
                    reports += 1;
                }
            ));
            assert_eq!(reports, 1);
            let header: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(disc).join("model.json"))?)?;
            assert_eq!(header["source_size"], bytes.len());
            for (field, at) in [("unknown_04", 4), ("unknown_16", 22)] {
                assert_eq!(header[field], crate::read::u16(bytes, at)?);
            }
            for (field, at) in [("unknown_08", 8), ("name_table_metadata", 24)] {
                assert_eq!(header[field], word(bytes, at)?);
            }
            assert_eq!(header["name"], "llo000.gpl");
            assert_eq!(header["unreferenced_storage"], serde_json::json!([]));
            assert_eq!(header["root_geometry"], crate::read::u16(bytes, 20)?);
            let nodes: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(disc).join("skeleton.json"))?)?;
            let nodes = nodes.as_array().context("missing cooked skeleton nodes")?;
            assert_eq!(nodes.len(), authored.nodes.len());
            for (index, (node, source)) in nodes.iter().zip(&authored.nodes).enumerate() {
                assert_eq!(node["node_id"], source.node_id);
                assert_eq!(node["transform_kind"], source.transform_kind);
                assert_eq!(node["draw_priority"], source.draw_priority);
                assert_eq!(node["node_flags"], source.flags);
                let data = source.data_offset as usize;
                if data == 0 {
                    assert!(node["transform"].is_null());
                } else {
                    let transform = &node["transform"];
                    let flags = bytes[data];
                    assert_eq!(transform["flags"], flags);
                    assert_eq!(
                        transform["metadata"],
                        serde_json::json!(&bytes[data + 1..data + 4])
                    );
                    assert_eq!(transform["kind"], "components");
                    for (mask, channel, start) in [(1, "scale", 1), (8, "translation", 8)] {
                        let words = &source.data_words[start..start + 3];
                        if flags & mask != 0 {
                            let values: [f32; 3] =
                                serde_json::from_value(transform[channel]["value"].clone())?;
                            assert_eq!(values.map(f32::to_bits).as_slice(), words);
                        } else {
                            assert_eq!(transform[channel]["value"], serde_json::json!(words));
                        }
                    }
                    if flags & 4 != 0 {
                        assert_eq!(transform["rotation"]["kind"], "quaternion");
                        let values: [f32; 4] =
                            serde_json::from_value(transform["rotation"]["xyzw"].clone())?;
                        assert_eq!(
                            values.map(f32::to_bits).as_slice(),
                            &source.data_words[4..8]
                        );
                    }
                    assert_eq!(
                        transform["unused_matrix_tail"],
                        serde_json::json!(&source.data_words[11..13])
                    );
                }
                assert_eq!(source.node_id, index as u16);
                assert_eq!(source.transform_kind, 1);
            }
            // The native readers stop at the referenced structures, regardless of tail values.
            for tail in [b"unexplained\0".as_slice(), &[0; 16]] {
                let mut extended = bytes.to_vec();
                extended.extend_from_slice(tail);
                let mut failures = Vec::new();
                assert!(cook(
                    &extended,
                    disc,
                    &output,
                    None,
                    Input::File,
                    &mut |_, result| {
                        if let Err(error) = result {
                            failures.push(error.to_string());
                        }
                    },
                ));
                assert!(failures.is_empty(), "{failures:?}");
                let header: serde_json::Value =
                    serde_json::from_slice(&fs::read(output.join(disc).join("model.json"))?)?;
                assert_eq!(header["source_size"], extended.len());
                let ranges = crate::model::Model::parse(&extended)?.unreferenced_ranges;
                assert_eq!(
                    ranges.len(),
                    usize::from(tail.iter().any(|&byte| byte != 0))
                );
                let saved = header["unreferenced_storage"]
                    .as_array()
                    .context("missing model storage")?;
                assert_eq!(saved.len(), ranges.len());
                for (saved, range) in saved.iter().zip(&ranges) {
                    assert_eq!(saved["offset"], range.start);
                    assert_eq!(saved["bytes"], serde_json::json!(&extended[range.clone()]));
                }
                assert!(output.join(disc).join("skeleton.json").is_file());
                assert!(output.join(disc).join("motion-bindings.json").is_file());
            }
        }
        fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; reads one bounded enemy package per disc"]
    fn original_model_trailer_preserves_every_source_byte() -> Result<()> {
        use std::io::{Seek, SeekFrom};

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("model-container"));
        let writer = Walker {
            output: &output,
            audio: None,
            recovered: None,
            report: &mut |_: &str, _: Result<()>| {},
        };
        for disc in ["disc1", "disc2"] {
            let extracted = root.join(disc);
            let sources = crate::source_assets::Sources::read(&extracted)?;
            let files = extracted.join("files");
            let usual = fs::read(files.join(sources.usual))?;
            let directory = &usual[sections(&usual)?[10].clone().context("enemy directory")?];
            let start = word(directory, 218 * 4)?;
            let end = word(directory, 219 * 4)?;
            let mut source = fs::File::open(files.join(sources.enemy))?;
            source.seek(SeekFrom::Start(u64::from(start)))?;
            let mut packed = vec![0; (end - start) as usize];
            source.read_exact(&mut packed)?;
            let package = compression::decode(&packed)?;
            let model = package
                .get(word(&package, 0x18)? as usize..word(&package, 0x1c)? as usize)
                .context("primary enemy model")?;
            let skeleton = crate::geometry::skeleton_range(model)?;
            let end = skeleton.end;
            assert!(skin::model(model, end)?.is_none());
            let ranges = crate::read::unreferenced_ranges(model, vec![0..end]);
            assert_eq!(ranges, [24704..24736]);
            assert_eq!(&model[end + 3..end + 10], b"bh03.h\0");
            assert!(storage(model, &[end..model.len() + 1]).is_err());
            let mut extended = model.to_vec();
            extended.extend([0; 16]);
            let mut zero_tail = model.to_vec();
            zero_tail[end..].fill(0);
            for bytes in [model, extended.as_slice(), zero_tail.as_slice()] {
                writer.model_container(bytes, disc, Some(skeleton.clone()), None)?;
                let receipt: serde_json::Value = serde_json::from_slice(&fs::read(
                    output.join(disc).join("model-container.json"),
                )?)?;
                assert_eq!(receipt["source_size"], bytes.len());
                assert_eq!(receipt["skeleton"], serde_json::json!(skeleton));
                assert!(receipt["skin"].is_null());
                let storage: Vec<crate::read::Storage> =
                    serde_json::from_value(receipt["unreferenced_storage"].clone())?;
                let mut restored = vec![0; receipt["source_size"].as_u64().unwrap() as usize];
                for span in &storage {
                    assert!(span.offset >= end);
                    restored[span.offset..span.offset + span.bytes.len()]
                        .copy_from_slice(&span.bytes);
                }
                assert_eq!(restored[end..], bytes[end..]);
                assert_eq!(
                    storage.is_empty(),
                    bytes[end..].iter().all(|&byte| byte == 0)
                );
            }
        }
        fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    fn broken_actor_directory_is_not_reinterpreted_as_a_single_resource() -> Result<()> {
        let mut bytes = [0; 160];
        for (index, value) in [31_u32, 128, 256].into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        let output = crate::temporary_path(&std::env::temp_dir().join("broken-actor-directory"));
        let mut reports = 0;
        let recognized = cook(&bytes, "actor", &output, None, Input::File, &mut |_, _| {
            reports += 1;
        });
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        assert!(!recognized);
        assert_eq!(reports, 0);
        Ok(())
    }

    #[test]
    fn indexed_animation_members_allow_omitted_names_but_validate_tracks() -> Result<()> {
        let mut clip = vec![0; 67];
        for (at, value) in [
            (0, 0x007b_7960_u32),
            (4, 24),
            (16, 12),
            (20, 67),
            (28, 36),
            (36, 10_f32.to_bits()),
            (40, 52),
            (56, 64),
        ] {
            clip[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for at in [10, 12, 14, 32, 44] {
            clip[at..at + 2].copy_from_slice(&1_u16.to_be_bytes());
        }
        clip[49] = 1;
        clip[64..].copy_from_slice(&[1, 2, 3]);
        let expected = crate::animation::unbound_indexed(&clip)?;
        let output = crate::temporary_path(&std::env::temp_dir().join("indexed-animation"));
        let mut errors = Vec::new();
        assert!(cook(
            &clip,
            "whole",
            &output,
            None,
            Input::File,
            &mut |_, result| {
                if let Err(error) = result {
                    errors.push(error);
                }
            }
        ));
        assert_eq!(errors.len(), 1, "a whole file cannot omit declared names");
        assert!(!output.join("whole/animation.json").exists());

        let mut bank = vec![0; 32];
        for (index, value) in [3_u32, 32, 67, 0, 0, 32, 67].into_iter().enumerate() {
            bank[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
        }
        bank.extend(clip);
        let mut reports = 0;
        assert!(cook(
            &bank,
            "bank",
            &output,
            None,
            Input::File,
            &mut |_, result| {
                result.unwrap();
                reports += 1;
            }
        ));
        assert_eq!(reports, 2, "aliased slots share one cooked member");
        let actual: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("bank/0/animation.json"))?)?;
        assert_eq!(actual, expected);
        let directory: super::super::archive::Directory =
            serde_json::from_slice(&fs::read(output.join("bank/archive.json"))?)?;
        assert_eq!(directory.members, [Some(0), None, Some(0)]);

        bank[32 + 56..32 + 60].copy_from_slice(&67_u32.to_be_bytes());
        errors.clear();
        assert!(cook(
            &bank,
            "broken",
            &output,
            None,
            Input::File,
            &mut |_, result| {
                if let Err(error) = result {
                    errors.push(error);
                }
            }
        ));
        assert_eq!(
            errors.len(),
            1,
            "indexed members still require bounded channel data"
        );
        assert!(!output.join("broken/0/animation.json").exists());
        fs::remove_dir_all(output)?;
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; JSON animation conversion only"]
    fn original_motion_banks_use_generic_member_cooking() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let output = crate::temporary_path(&std::env::temp_dir().join("generic-motion-bank"));
        let result = (|| -> Result<()> {
            for disc in ["disc1", "disc2"] {
                let bytes = fs::read(root.join(disc).join("files/d.d"))?;
                let ranges =
                    super::super::archive::entries(&bytes).context("motion bank directory")?;
                let mut members = 0;
                let mut others = 0;
                for (index, range) in ranges.iter().enumerate() {
                    let Some(range) = range else { continue };
                    let source = &bytes[range.clone()];
                    if !crate::animation::is_animation(source) {
                        others += 1;
                        continue;
                    }
                    // The bank also contains models; this check isolates animation
                    // dispatch from texture conversion and uses no filename hint.
                    let name = format!("{disc}/renamed-resource/{index}");
                    let mut result = Ok(());
                    Walker {
                        output: &output,
                        audio: None,
                        recovered: None,
                        report: &mut |_: &str, outcome| result = outcome,
                    }
                    .member(source, &name, None, 1, Input::Member);
                    result.with_context(|| name.clone())?;
                    let cooked: serde_json::Value = serde_json::from_slice(&fs::read(
                        output.join(&name).join("animation.json"),
                    )?)?;
                    ensure!(
                        cooked == crate::animation::unbound_indexed(source)?,
                        "{name}: cooked animation differs from source"
                    );
                    members += 1;
                }
                ensure!(members > 0, "motion bank has no animations");
                eprintln!(
                    "{disc}: {members} animation members match generic cooking; {others} other members skipped"
                );
                fs::remove_dir_all(output.join(disc))?;
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }

    #[test]
    #[ignore = "requires original NPC archives; no asset conversion"]
    fn original_npc_packages_are_complete_section_directories() -> Result<()> {
        for disc in ["disc1", "disc2"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/extracted")
                .join(disc)
                .join("files/npc_all.bin");
            let bytes = fs::read(path)?;
            let entries =
                super::super::archive::entries(&bytes).context("NPC archive directory")?;
            assert_eq!(entries.len(), 374);
            for range in entries.into_iter().flatten() {
                let bytes = &bytes[range];
                let parts = sections(bytes)?;
                assert_eq!(parts.len(), 31);
                super::super::archive::padding(bytes, 128, &parts)?;
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_model_extents_cover_character_and_field_leaves() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
        let check = |bytes: &[u8]| -> Result<()> {
            let range = crate::geometry::skeleton_range(bytes)?;
            ensure!(!range.is_empty(), "expected an authored skeleton");
            crate::model::Model::parse(&bytes[range.clone()])?;
            super::super::archive::padding(bytes, range.end, &[])
        };
        for file in ["lloyd000.bin", "genius000.bin", "shihna003.bin"] {
            let bytes = fs::read(root.join(file))?;
            let primary = sections(&bytes)?[0]
                .clone()
                .context("missing character model")?;
            check(&bytes[primary]).with_context(|| file.to_owned())?;
        }
        for (file, index) in [("chu_i05_00.bin", 1), ("faa_d02.bin", 4)] {
            let map = crate::field::MapArchive::decode(&fs::read(root.join("MAP").join(file))?)?;
            let bank = map.section(7)?;
            let binding = crate::character::field_model_entries(bank)?
                .into_iter()
                .nth(index - 1)
                .context("missing field actor")?;
            let actor = &bank[binding.1];
            let range = sections(actor)?[0].clone().context("missing actor model")?;
            check(&actor[range]).with_context(|| file.to_owned())?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original extracted disc"]
    fn original_caller_bound_field_materials_preserve_bindings_and_geometry() -> Result<()> {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files/MAP");
        let output =
            crate::temporary_path(&std::env::temp_dir().join("resonance-caller-materials"));
        let mut count = 0;
        for entry in fs::read_dir(source)? {
            let path = entry?.path();
            let name = path.file_stem().unwrap().to_str().unwrap();
            let table = if name == "dar_d00" {
                "takara.tpl"
            } else if name.starts_with("vmh_d0") {
                "VMH_sphere.tpl"
            } else if name.starts_with("woa_d0") {
                "cone.tpl"
            } else {
                continue;
            };
            let map = crate::field::MapArchive::decode(&fs::read(&path)?)?;
            let mut bytes = map.section(16)?;
            if word(bytes, 0)? == 31 {
                bytes = &bytes[sections(bytes)?[0]
                    .clone()
                    .context("missing primary model")?];
            }
            ensure!(
                word(bytes, 0)? == 0,
                "{name} unexpectedly has a local texture table"
            );
            let mut report = |_: &str, result: Result<()>| result.unwrap();
            Walker {
                output: &output,
                audio: None,
                recovered: None,
                report: &mut report,
            }
            .model(bytes, name, None)?;
            let recipe: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(name).join("scene.json"))?)?;
            assert_eq!(recipe["textures"]["kind"], "caller");
            assert!(!output.join("intermediate").join(name).exists());
            for material in recipe["draws"].as_array().unwrap() {
                assert!(material.get("combination").is_none());
                assert!(material["recipe"]["source_mode"].is_u64());
                assert!(material["recipe"]["source_texture_count"].is_u64());
                assert!(material["recipe"]["operations"].is_array());
                let texture = &material["textures"][0];
                assert_eq!(texture["stage"], 0);
                assert_eq!(texture["image"], 0);
                assert_eq!(texture["table"], table);
                assert_eq!(texture["wrap"], serde_json::json!(["repeat", "repeat"]));
                assert_eq!(texture["min_filter"], "linear");
                assert_eq!(texture["mag_filter"], "linear");
            }
            let glb = fs::read(output.join(recipe["mesh"].as_str().unwrap()))?;
            let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
            let mesh: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len])?;
            assert!(mesh.get("materials").is_none());
            assert!(mesh.get("images").is_none());
            assert_eq!(
                mesh["meshes"].as_array().unwrap().len(),
                recipe["draws"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|draw| !draw["mesh"].is_null())
                    .count()
            );
            assert!(!mesh["scenes"][0]["nodes"].as_array().unwrap().is_empty());
            count += 1;
        }
        assert_eq!(count, 17);
        fs::remove_dir_all(output)?;
        Ok(())
    }
}
