//! Assemble source-backed layers and publish their final shared assets.
use super::glb::Glb;
#[cfg(test)]
use crate::all_assets::physical_scene::{Scene, TextureSource};
use crate::{
    all_assets::{FieldDirectory, MemberKind},
    animation::{AuthoredAnimation, ModelBindings},
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
use resonance_content::{SceneClip, ScenePart};
#[cfg(test)]
use serde::de::DeserializeOwned;
#[cfg(test)]
use std::fs;
use std::{path::Path, sync::Arc};

// Background, secondary background, foreground and translucent scenery, in draw order.
const FIELD_LAYERS: [usize; 4] = [0, 10, 12, 2];

pub(crate) struct Map<'a> {
    root: &'a Path,
    directory: String,
    sections: FieldDirectory,
    archive: Arc<crate::field::MapArchive>,
    recovered: Option<&'a super::recovered::RecoveredModels>,
    pub(crate) source_sha256: String,
}

#[cfg(test)]
pub(crate) fn read<T: DeserializeOwned>(root: &Path, path: &str) -> Result<T> {
    resonance_content::validate_asset_path(path)?;
    serde_json::from_slice(&fs::read(root.join(path)).with_context(|| {
        format!(
            "missing cooked {path}; run cook-all --output {} first",
            root.display()
        )
    })?)
    .with_context(|| format!("invalid cooked {path}"))
}

/// Project supported materials without changing the source-owned publication.
#[cfg(test)]
pub(crate) fn model(root: &Path, directory: &str) -> Result<(ScenePart, Glb)> {
    let scene: Scene = read(root, &format!("{directory}/scene.json"))?;
    let owned = |path: &str| -> Result<()> {
        resonance_content::validate_asset_path(path)?;
        ensure!(
            path.starts_with(&format!("{directory}/")) && root.join(path).is_file(),
            "missing or incorrectly owned cooked model resource {path}"
        );
        Ok(())
    };
    resonance_content::validate_asset_path(&scene.mesh)?;
    ensure!(
        scene.mesh.starts_with("meshes/") && root.join(&scene.mesh).is_file(),
        "missing canonical mesh {}",
        scene.mesh
    );
    let textures = match &scene.textures {
        TextureSource::Local { catalogue } => {
            owned(catalogue)?;
            let textures = crate::texture::read(&root.join(catalogue))?;
            if textures.textures.is_empty() {
                Vec::new()
            } else {
                let directory = catalogue
                    .strip_suffix("/textures.json")
                    .context("texture catalogue path")?;
                crate::texture::bind(root, directory)?
                    .into_iter()
                    .map(|texture| texture.image(0).map(|image| image.path))
                    .collect::<Result<_>>()?
            }
        }
        TextureSource::Caller => {
            ensure!(
                scene
                    .draws
                    .iter()
                    .filter(|draw| draw.mesh.is_some())
                    .all(|draw| draw.recipe.textures().next().is_none()),
                "model requires caller-supplied textures"
            );
            Vec::new()
        }
    };
    let glb = Glb::read(&root.join(&scene.mesh))?;
    ensure!(
        glb.json["animations"].as_array().is_none_or(Vec::is_empty),
        "physical mesh already has animations"
    );
    let part = super::projection::project(&scene, &glb.json, textures)?;
    Ok((part, glb))
}

/// Project a decoded model directly; only terminal publication needs a path.
pub(crate) fn decoded(
    model: &crate::geometry::DecodedGeometry,
    textures: &crate::texture::Catalogue,
) -> Result<(ScenePart, Glb)> {
    let paths = textures
        .textures
        .iter()
        .map(|texture| {
            texture
                .as_ref()
                .context("incomplete decoded texture")?
                .image(0)
                .map(|image| image.path)
        })
        .collect::<Result<_>>()?;
    let glb = Glb {
        json: model.gltf.clone(),
        binary: std::sync::Arc::clone(&model.binary),
        motions: Vec::new(),
    };
    let mut part = super::projection::project(&model.scene, &glb.json, paths)?;
    part.secondary_motion = crate::secondary_motion::bind(
        model.model_name.as_deref().unwrap_or_default(),
        &glb.json,
        &part.bone_names,
    )?;
    ensure!(
        part.secondary_motion.chains.is_empty() || model.model_name.is_some(),
        "missing secondary-motion model name"
    );
    Ok((part, glb))
}

pub(crate) fn write_mesh(root: &Path, glb: &Glb) -> Result<String> {
    for motion in &glb.motions {
        crate::write_atomic(&root.join(&motion.path), &motion.bytes)?;
    }
    let bytes = resonance_asset_writer::gltf::pack_glb(&glb.json, &glb.binary)?;
    let path = format!("meshes/{}.glb", crate::digest(&bytes));
    crate::write_atomic(&root.join(&path), &bytes)?;
    Ok(path)
}

#[cfg(test)]
pub(crate) fn secondary_motion(
    root: &Path,
    directory: &str,
    glb: &Glb,
    part: &ScenePart,
) -> Result<resonance_content::secondary_motion::Definition> {
    #[derive(serde::Deserialize)]
    struct Model {
        name: Option<String>,
    }
    let mut definition = crate::secondary_motion::bind("", &glb.json, &part.bone_names)?;
    if !definition.chains.is_empty() {
        let model: Model = read(root, &format!("{directory}/model.json"))?;
        definition.model = model.name.context("missing secondary-motion model name")?;
    }
    Ok(definition)
}

#[cfg(test)]
pub(crate) fn bindings(root: &Path, directory: &str, part: &ScenePart) -> Result<ModelBindings> {
    let model: ModelBindings = read(root, &format!("{directory}/motion-bindings.json"))?;
    ensure!(
        model.node_ids.len() == part.bone_names.len(),
        "cooked motion bindings differ from mesh"
    );
    if let Some(names) = &model.names {
        ensure!(
            names.len() == part.bone_names.len()
                && names
                    .iter()
                    .zip(&part.bone_names)
                    .all(|(source, mesh)| source.is_empty() || source == mesh),
            "cooked model name bindings differ from mesh"
        );
    }
    Ok(model)
}

impl<'a> Map<'a> {
    pub(crate) fn open(root: &'a Path, source: &Path) -> Result<Self> {
        Self::new(
            root,
            Arc::new(crate::field::MapArchive::open(source)?),
            None,
        )
    }

    pub(crate) fn from_archive(
        root: &'a Path,
        archive: Arc<crate::field::MapArchive>,
        recovered: &'a super::recovered::RecoveredModels,
    ) -> Result<Self> {
        Self::new(root, archive, Some(recovered))
    }

    fn new(
        root: &'a Path,
        archive: Arc<crate::field::MapArchive>,
        recovered: Option<&'a super::recovered::RecoveredModels>,
    ) -> Result<Self> {
        let sections = FieldDirectory::new(&archive.bytes, &archive.sections);
        sections.validate()?;
        Ok(Self {
            root,
            directory: format!("assets/{}/{}", archive.source_sha256, archive.member),
            source_sha256: archive.source_sha256.clone(),
            sections,
            archive,
            recovered,
        })
    }

    pub(crate) fn source_section(&self, index: usize) -> Result<&[u8]> {
        self.archive.section(index)
    }

    // Paths identify final script publications.
    pub(crate) fn section(&self, index: usize) -> Option<String> {
        self.sections
            .directory
            .members
            .get(index)?
            .map(|canonical| format!("{}/{canonical}", self.directory))
    }

    pub(crate) fn sections(&self) -> impl Iterator<Item = (usize, MemberKind)> + '_ {
        self.sections
            .directory
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, member)| member.map(|_| (index, self.sections.kinds[index])))
    }

    pub(crate) fn publish_script(&self) -> Result<[String; 2]> {
        let directory = self.section(6).context("missing field script")?;
        let paths = ["script.ssb", "messages.json"].map(|name| format!("{directory}/{name}"));
        crate::write_atomic(&self.root.join(&paths[0]), &self.script()?)?;
        crate::write_atomic(
            &self.root.join(&paths[1]),
            &serde_json::to_vec(&self.messages()?)?,
        )?;
        Ok(paths)
    }

    pub(crate) fn script(&self) -> Result<Vec<u8>> {
        let bytes = self.archive.section(6)?;
        symphonia_script::Program::decode(bytes)?;
        Ok(bytes.to_vec())
    }

    pub(crate) fn messages(&self) -> Result<Vec<symphonia_script::message::Message>> {
        let bytes = self.archive.section(6)?;
        let header = symphonia_script::scenario::parse_header(bytes)?;
        Ok(symphonia_script::message::parse(
            bytes
                .get(header.auxiliary_offset()..)
                .context("field messages exceed script")?,
        )?)
    }

    pub(crate) fn collision(
        &self,
        index: usize,
    ) -> Result<Vec<resonance_content::field::CollisionGroup>> {
        use crate::field::collision_data::{Format, Mesh};
        Ok(Mesh::read(self.archive.section(index)?, Format::Detect)?.groups)
    }

    pub(crate) fn camera(&self, index: usize) -> Result<crate::all_assets::CameraTrack> {
        crate::all_assets::camera(self.archive.section(index)?)
    }

    pub(crate) fn models(&self) -> Result<Vec<(u16, &[u8])>> {
        let Some(bytes) = self.archive.optional_section(7) else {
            return Ok(Vec::new());
        };
        let mut ids = std::collections::BTreeSet::new();
        crate::character::field_model_entries(bytes)?
            .into_iter()
            .map(|(id, range)| {
                ensure!(
                    id > 9 && id != 24 && ids.insert(id),
                    "duplicate or reserved field model ID {id}"
                );
                Ok((id, &bytes[range]))
            })
            .collect()
    }

    pub(crate) fn part(
        &self,
        index: usize,
        extra: &[(u32, Arc<AuthoredAnimation>)],
    ) -> Result<(ScenePart, std::sync::Arc<Glb>)> {
        let stage = DrawStage::scenery(index.try_into()?).context("unknown scenery layer")?;
        let source = self.archive.section(index)?;
        let autoplay = self
            .archive
            .optional_section(index + 1)
            .map(|bytes| self.read_animation(bytes))
            .transpose()?;
        let mut models = super::source::Models::with_recovered(self.root, self.recovered);
        models.add(
            &format!("field/{index}"),
            source,
            source,
            move |geometry, _, part, glb| {
                field_layer(part, index, stage as u32)?;
                if autoplay.is_some() || !extra.is_empty() {
                    let model = geometry
                        .bindings
                        .as_ref()
                        .context("animated field model lacks bindings")?;
                    if let Some(animation) = &autoplay {
                        bind_clip(part, glb, model, animation, None)?;
                        part.autoplay = true;
                    }
                    for (resource, animation) in extra {
                        ensure!(
                            !part
                                .clips
                                .iter()
                                .any(|clip| clip.animation_resource == Some(*resource)),
                            "duplicate field animation resource {resource:#x}"
                        );
                        bind_clip(part, glb, model, animation, Some(*resource))?;
                    }
                }
                Ok(())
            },
        )?;
        let layer = models
            .finish()?
            .pop()
            .context("missing prepared field layer")?;
        Ok((layer.part, layer.glb))
    }

    pub(super) fn title_part(
        &self,
        index: usize,
        order: u32,
    ) -> Result<(ScenePart, std::sync::Arc<Glb>)> {
        let source = self.archive.section(index)?;
        let slots: &[u16] = match index {
            0 | 2 => &[],
            17 | 18 | 21 => &[12, 36],
            20 | 22 => &[12],
            _ => anyhow::bail!("unsupported title section {index}"),
        };
        let mut clips = Vec::new();
        let model = if slots.is_empty() {
            clips.push((0, self.read_animation(self.archive.section(index + 1)?)?));
            source
        } else {
            let members = crate::field::sections(source)?;
            let member = |index| -> Result<&[u8]> {
                Ok(&source[members
                    .get(index)
                    .and_then(Clone::clone)
                    .context("missing title package member")?])
            };
            for &slot in slots {
                clips.push((
                    slot,
                    self.read_animation(member(usize::from(slot / 4 - 1))?)?,
                ));
            }
            member(0)?
        };
        let mut models = super::source::Models::with_recovered(self.root, self.recovered);
        models.add(
            &format!("title/{index}"),
            model,
            model,
            move |geometry, _, part, glb| {
                field_layer(part, index, order)?;
                let bindings = geometry
                    .bindings
                    .as_ref()
                    .context("title model lacks bindings")?;
                for (slot, animation) in &clips {
                    part.clips.push(SceneClip {
                        resource_slot: *slot,
                        secondary_pose_nodes: Vec::new(),
                        ..glb.animate(animation.motion(bindings)?)?
                    });
                }
                part.autoplay = slots.is_empty();
                part.translation[2] = if index == 17 { 5. } else { 0. };
                Ok(())
            },
        )?;
        let layer = models
            .finish()?
            .pop()
            .context("missing prepared title layer")?;
        Ok((layer.part, layer.glb))
    }

    pub(crate) fn layers(
        &self,
        extra: &[(u32, Arc<AuthoredAnimation>)],
    ) -> Result<(Vec<ScenePart>, Vec<resonance_content::field::Door>)> {
        let mut parts = Vec::new();
        let mut doors = Vec::new();
        for index in FIELD_LAYERS
            .into_iter()
            .filter(|&index| self.section(index).is_some())
        {
            let (part, glb) = self
                .part(index, extra)
                .with_context(|| format!("bind field layer {index}"))?;
            if index == 0 {
                doors = crate::field_doors::cook(&glb.json)?;
            }
            parts.push(part);
        }
        Ok((parts, doors))
    }

    fn read_animation(&self, bytes: &[u8]) -> Result<Arc<AuthoredAnimation>> {
        super::recovered::animation(bytes, self.recovered)
    }
}

fn field_layer(part: &mut ScenePart, index: usize, order: u32) -> Result<()> {
    part.resource = index.try_into()?;
    for material in &mut part.materials {
        ensure!(
            material.draw_order < MODEL_DRAW_SPAN,
            "invalid field draw order"
        );
        material.draw_order += order * MODEL_DRAW_SPAN;
        material.depth_write = index != 2;
    }
    // Scenery uses its explicit clips; secondary actor dynamics bind separately.
    part.secondary_motion = Default::default();
    Ok(())
}

pub(crate) fn bind_clip(
    part: &mut ScenePart,
    glb: &mut Glb,
    model: &ModelBindings,
    animation: &AuthoredAnimation,
    resource: Option<u32>,
) -> Result<()> {
    let mut clip = SceneClip {
        resource_slot: if resource.is_some() { 12 } else { 0 },
        animation_resource: resource,
        ..glb.animate(animation.motion(model)?)?
    };
    if resource.is_none() {
        clip.secondary_pose_nodes.clear();
    }
    part.clips.push(clip);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    #[ignore = "requires original disc 1; no prior cooked assets"]
    fn original_title_and_field_layers_need_no_physical_intermediates() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let title = extracted
            .join("files")
            .join(crate::scene::title_source(&extracted, &executable)?);
        for (source, title) in [
            (title, true),
            (crate::field::source_for_id(&extracted, 332)?, false),
            (crate::field::source_for_id(&extracted, 340)?, false),
            (crate::field::source_for_id(&extracted, 344)?, false),
            (crate::field::source_for_id(&extracted, 372)?, false),
            (extracted.join("files/MAP/_custom.bin"), false),
        ] {
            let output = tempfile::tempdir()?;
            fs::write(
                output.path().join("sources.json"),
                b"not a cooked catalogue",
            )?;
            let map = Map::open(output.path(), &source)?;
            let indices: &[usize] = if title {
                &[0, 2, 17, 18, 20, 21, 22]
            } else {
                &FIELD_LAYERS
            };
            for (order, &index) in indices.iter().enumerate() {
                if map.archive.optional_section(index).is_none() {
                    continue;
                }
                let (part, glb) = if title {
                    map.title_part(index, order as u32)?
                } else {
                    map.part(index, &[])?
                };
                assert!(part.mesh.starts_with("meshes/"));
                assert!(glb.json.get("animations").is_none());
                assert_eq!(Glb::read(&output.path().join(&part.mesh))?.json, glb.json);
                for clip in &part.clips {
                    let motion = resonance_content::animation::Motion::decode(&fs::read(
                        output.path().join(&clip.motion),
                    )?)?;
                    motion.validate_bones(part.bone_names.len())?;
                }
            }
            if !title {
                for part in map.layers(&[])?.0 {
                    let stage =
                        DrawStage::scenery(part.resource).context("unknown scenery layer")?;
                    assert!(part.materials.iter().all(|material| {
                        material.draw_order / MODEL_DRAW_SPAN == stage as u32
                            && !(DrawStage::Actors.offset()..DrawStage::Foreground.offset())
                                .contains(&material.draw_order)
                            && material.depth_write == (part.resource != 2)
                    }));
                }
            }
            assert!(
                !output.path().join("assets").exists(),
                "scenery transformation published an intermediate resource"
            );
            let [script, messages] = map.publish_script()?;
            assert_eq!(
                fs::read(output.path().join(script))?,
                map.archive.section(6)?
            );
            assert_eq!(
                serde_json::to_value(read::<Vec<symphonia_script::message::Message>>(
                    output.path(),
                    &messages
                )?)?,
                serde_json::to_value(map.messages()?)?
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    fn fixture() -> Result<tempfile::TempDir> {
        let library = std::env::var_os("RESONANCE_COOKED")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"))
            .canonicalize()?;
        let stage = tempfile::tempdir()?;
        fs::copy(
            library.join("sources.json"),
            stage.path().join("sources.json"),
        )?;
        std::os::unix::fs::symlink(library.join("assets"), stage.path().join("assets"))?;
        Ok(stage)
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires original fields and cooked library; no codecs or renderer"]
    fn original_line_primitives_bind_to_runtime_field_layers() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let stage = fixture()?;
        let cooked = stage.path();
        for (source, section, expected) in [
            ("MAP/cot_i00.bin", 0, vec![12, 13]),
            ("MAP/hol_d00.bin", 2, vec![1]),
        ] {
            let original =
                crate::field::MapArchive::open(&local.join("extracted/disc1/files").join(source))?;
            let resource = original.section(section)?;
            crate::geometry::preflight_section(resource)?;
            let map = Map::open(cooked, &local.join("extracted/disc1/files").join(source))?;
            let (part, glb) = map.part(section, &[])?;
            let stage = DrawStage::scenery(section.try_into()?).context("unknown scenery layer")?;
            crate::geometry::compare_field_layer(resource, &part, &glb.json, stage as u32)?;
            let lines: Vec<_> = glb.json["meshes"]
                .as_array()
                .context("missing field meshes")?
                .iter()
                .enumerate()
                .filter_map(|(mesh, value)| {
                    value["primitives"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|primitive| primitive["mode"] == 1)
                        .then_some(mesh)
                })
                .collect();
            assert_eq!(lines, expected, "{source}/{section}");
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires original fields and cook-all records; writes derived meshes without codecs"]
    fn original_declared_animations_bind_to_scenery_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let stage = fixture()?;
        let cooked = stage.path();
        let mut bindings = 0;
        let mut coverage = BTreeMap::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let mut resources =
                crate::field_resources::binding::Resources::open(&extracted, &catalogue)?;
            for id in [330, 340] {
                let source = crate::field::source_for_id(&extracted, id)?;
                let map = crate::field::MapArchive::open(&source)?;
                let declarations = crate::field_resources::declarations(map.section(6)?)?;
                let sources =
                    crate::character::Sources::read(&mut resources, &declarations.resources)?;
                let clips = &sources.animations;
                assert_eq!(clips.is_empty(), id == 330);
                let physical = Map::open(cooked, &source)?;
                assert_eq!(physical.script()?, map.section(6)?);
                let header = symphonia_script::scenario::parse_header(map.section(6)?)?;
                assert_eq!(
                    serde_json::to_value(physical.messages()?)?,
                    serde_json::to_value(symphonia_script::message::parse(
                        &map.section(6)?[header.auxiliary_offset()..]
                    )?)?
                );
                for index in [4, 5] {
                    if let Some(bytes) = map.optional_section(index) {
                        assert_eq!(
                            serde_json::to_value(physical.collision(index)?)?,
                            serde_json::to_value(crate::field::collision(bytes)?)?
                        );
                    }
                }
                assert_eq!(
                    physical
                        .models()?
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>(),
                    crate::character::field_model_entries(map.section(7)?)?
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>()
                );
                let mut layers = 0;
                for index in FIELD_LAYERS {
                    let Some(source) = map.optional_section(index) else {
                        continue;
                    };
                    layers += 1;
                    let (original, original_glb) = physical.part(index, &[])?;
                    let (part, glb) = physical.part(index, clips)?;
                    if clips.is_empty() {
                        assert_eq!(serde_json::to_value(part)?, serde_json::to_value(original)?);
                        assert_eq!(glb.json, original_glb.json);
                        assert_eq!(glb.binary, original_glb.binary);
                        continue;
                    }
                    let range = crate::geometry::skeleton_range(source)?;
                    let model = ModelBindings::read(&source[range])?;
                    assert_eq!(part.bone_names, original.bone_names);
                    assert_eq!(part.autoplay, original.autoplay);
                    assert_eq!(part.textures, original.textures);
                    assert_eq!(part.clips.len(), original.clips.len() + clips.len());
                    for (offset, (resource, animation)) in clips.iter().enumerate() {
                        let at = original.clips.len() + offset;
                        let clip = &part.clips[at];
                        let motion = animation.motion(&model)?;
                        assert_eq!(clip.animation_resource, Some(*resource));
                        assert_eq!(clip.resource_slot, 12);
                        assert_eq!(
                            clip.duration_seconds,
                            motion.duration_frames / resonance_content::animation::FRAME_HZ
                        );
                        assert_eq!(
                            clip.secondary_pose_nodes,
                            motion
                                .tracks
                                .iter()
                                .filter(|track| track.times.len() > 2)
                                .map(|track| track.bone)
                                .collect::<Vec<_>>()
                        );
                        let stored = resonance_content::animation::Motion::decode(&fs::read(
                            cooked.join(&clip.motion),
                        )?)?;
                        assert_eq!(serde_json::to_value(stored)?, serde_json::to_value(motion)?);
                        assert!(glb.json.get("animations").is_none());
                        bindings += 1;
                    }
                }
                assert!(layers > 0);
                coverage.insert(
                    (disc, id),
                    (layers, clips.iter().map(|(id, _)| *id).collect::<Vec<_>>()),
                );
            }
        }
        for id in [330, 340] {
            assert_eq!(coverage[&(1, id)], coverage[&(2, id)]);
        }
        assert_eq!(
            bindings,
            coverage
                .values()
                .map(|(layers, clips)| layers * clips.len())
                .sum::<usize>()
        );
        assert!(bindings > 0);
        eprintln!("Prepared {bindings} declared animation/scenery bindings on both discs");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires original MAP archives and cook-all on both discs; writes derived meshes without codecs"]
    fn original_map_layers_match_native_motion_binding_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let stage = fixture()?;
        let cooked = stage.path();
        let sources: BTreeMap<String, Vec<String>> = read(cooked, "sources.json")?;
        let mut counts = [0; 5];
        for disc in [1, 2] {
            let prefix = format!("disc{disc}/");
            for source in sources
                .keys()
                .filter_map(|source| source.strip_prefix(&prefix))
                .filter(|source| source.starts_with("MAP/"))
            {
                let original = local
                    .join(format!("extracted/disc{disc}/files"))
                    .join(source);
                let bytes = fs::read(&original)?;
                let map = crate::field::MapArchive::decode(&bytes)?;
                let bound = Map::open(cooked, &original)?;
                assert_eq!(
                    bound.sections.directory.count,
                    map.sections.len(),
                    "{source}"
                );
                for (index, kind) in bound.sections() {
                    let expected = match crate::read::u32(map.section(index)?, 0).ok() {
                        Some(31) => MemberKind::Actor,
                        Some(0x0020af30) => MemberKind::Texture,
                        _ if crate::all_assets::geometry::is_model(map.section(index)?) => {
                            MemberKind::Model
                        }
                        _ => MemberKind::Other,
                    };
                    assert_eq!(kind, expected, "{source}/{index}");
                }
                let cabinet = cab::Cabinet::new(std::io::Cursor::new(&bytes))?;
                let members = cabinet
                    .folder_entries()
                    .flat_map(|folder| folder.file_entries())
                    .map(|entry| entry.name().replace('\\', "/"))
                    .collect::<Vec<_>>();
                assert_eq!(
                    bound.directory,
                    format!("assets/{}/{}", map.source_sha256, members[0])
                );
                counts[0] += 1;
                for index in FIELD_LAYERS {
                    let Some(resource) = map.optional_section(index) else {
                        continue;
                    };
                    if !matches!(
                        crate::read::u32(resource, 0x20).ok(),
                        Some(0x005bbc61 | 0x00b749e0)
                    ) {
                        continue;
                    }
                    let mut expected_motion = None;
                    if crate::read::u32(resource, 8)? != 0 {
                        let range = crate::geometry::skeleton_range(resource)?;
                        let model = &resource[range];
                        let directory = bound
                            .section(index)
                            .context("missing cooked model section")?;
                        let bindings: ModelBindings =
                            read(cooked, &format!("{directory}/motion-bindings.json"))?;
                        assert_eq!(
                            serde_json::to_value(&bindings)?,
                            serde_json::to_value(ModelBindings::read(model)?)?,
                            "{source}/{index}"
                        );
                        counts[1] += 1;
                        if let Some(animation) = map.optional_section(index + 1) {
                            let animation_directory = bound
                                .section(index + 1)
                                .context("missing cooked animation section")?;
                            let authored: AuthoredAnimation =
                                read(cooked, &format!("{animation_directory}/animation.json"))?;
                            let expected = crate::animation::motion(animation, model);
                            let actual = authored.motion(&bindings);
                            match (expected, actual) {
                                (Ok(expected), Ok(actual)) => {
                                    assert_eq!(
                                        serde_json::to_value(actual)?,
                                        serde_json::to_value(&expected)?,
                                        "{source}/{index}"
                                    );
                                    counts[2] += 1;
                                    expected_motion = Some(expected);
                                }
                                (Err(expected), Err(actual)) => {
                                    eprintln!(
                                        "unsupported motion {source}/{index}: source={expected:#}, cooked={actual:#}"
                                    );
                                    counts[3] += 1;
                                    continue;
                                }
                                (expected, actual) => anyhow::bail!(
                                    "native/cooked motion admission differs for {source}/{index}: native={expected:?}, cooked={actual:?}"
                                ),
                            }
                        }
                    } else if map.optional_section(index + 1).is_some() {
                        // Source preparation requires a model before autoplay.
                        assert!(bound.part(index, &[]).is_err());
                        counts[3] += 1;
                        continue;
                    }
                    if crate::geometry::preflight_section(resource).is_ok() {
                        let (part, glb) = bound
                            .part(index, &[])
                            .with_context(|| format!("{source}/{index}"))?;
                        crate::geometry::compare_field_layer(
                            resource,
                            &part,
                            &glb.json,
                            DrawStage::scenery(index.try_into()?)
                                .context("unknown scenery layer")?
                                as u32,
                        )
                        .with_context(|| format!("{source}/{index}"))?;
                        assert_eq!(part.autoplay, expected_motion.is_some());
                        if let Some(motion) = expected_motion {
                            assert_eq!(
                                part.clips[0].duration_seconds,
                                motion.duration_frames / resonance_content::animation::FRAME_HZ
                            );
                            let stored = resonance_content::animation::Motion::decode(&fs::read(
                                cooked.join(&part.clips[0].motion),
                            )?)?;
                            assert_eq!(
                                serde_json::to_value(stored)?,
                                serde_json::to_value(motion)?
                            );
                            assert!(glb.json.get("animations").is_none());
                        }
                        counts[4] += 1;
                    }
                }
            }
        }
        assert_eq!(counts[0], 1002);
        assert_eq!(counts[3], 0, "original MAP motions must all bind");
        assert!(
            counts[1] > 0 && counts[2] > 0 && counts[4] > 0,
            "incomplete MAP comparison: {counts:?}"
        );
        eprintln!(
            "MAP source/curve comparison: {} archives, {} model bindings, {} matching motions, {} shared unsupported motions, {} bound layers",
            counts[0], counts[1], counts[2], counts[3], counts[4]
        );
        Ok(())
    }
}
