//! Assemble source-backed layers and publish their final shared assets.
use super::glb::Glb;
use crate::{
    all_assets::{FieldDirectory, MemberKind},
    animation::{AuthoredAnimation, ModelBindings},
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
use resonance_content::{SceneClip, ScenePart};
#[cfg(test)]
use std::fs;
use std::{borrow::Cow, path::Path, sync::Arc};

// Background, secondary background, foreground and translucent scenery, in draw order.
const FIELD_LAYERS: [usize; 4] = [0, 10, 12, 2];

pub(crate) struct Map<'a> {
    root: &'a Path,
    directory: String,
    sections: FieldDirectory,
    archive: Arc<crate::field::MapArchive>,
    pub(crate) decoded: Cow<'a, super::decoded::Package>,
}

/// Project a decoded model directly; only terminal publication needs a path.
pub(crate) fn captured(model: &crate::scene::decoded::Model) -> Result<ScenePart> {
    let mut part = super::projection::project_image(
        &model.geometry.scene,
        &model.geometry.gltf,
        Vec::new(),
        resonance_content::SceneImage::Capture,
    )?;
    part.mesh.clone_from(&model.geometry.scene.mesh);
    Ok(part)
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

impl<'a> Map<'a> {
    pub(crate) fn panorama(&self) -> Result<Option<String>> {
        let Some(bytes) = self.archive.optional_section(15) else {
            return Ok(None);
        };
        let decoded = self.decoded.textures(bytes)?;
        decoded.validate()?;
        let mut result = Ok(());
        decoded.publish(self.root, |_, written| {
            if result.is_ok() {
                result = written;
            }
        });
        result?;
        Ok(Some(
            decoded
                .catalogue
                .textures
                .first()
                .and_then(Option::as_ref)
                .context("field panorama texture is missing")?
                .image(0)?
                .path,
        ))
    }

    pub(crate) fn camera_tracks(
        &self,
    ) -> Result<std::collections::BTreeMap<u32, Vec<resonance_content::CameraKey>>> {
        self.archive.camera_tracks()
    }
    #[cfg(test)]
    pub(crate) fn open(root: &'a Path, source: &Path) -> Result<Self> {
        let archive = Arc::new(crate::field::MapArchive::open(source)?);
        let decoded = super::decoded::Package::cook(
            &archive.bytes,
            &format!("assets/{}/{}", archive.source_sha256, archive.member),
            root,
            crate::all_assets::geometry::Input::Field,
        )?;
        Self::new(root, archive, Cow::Owned(decoded))
    }

    pub(crate) fn from_archive(
        root: &'a Path,
        archive: Arc<crate::field::MapArchive>,
        decoded: &'a super::decoded::Package,
    ) -> Result<Self> {
        Self::new(root, archive, Cow::Borrowed(decoded))
    }

    fn new(
        root: &'a Path,
        archive: Arc<crate::field::MapArchive>,
        decoded: Cow<'a, super::decoded::Package>,
    ) -> Result<Self> {
        let sections = FieldDirectory::new(&archive.bytes, &archive.sections);
        sections.validate()?;
        Ok(Self {
            root,
            directory: format!("assets/{}/{}", archive.source_sha256, archive.member),
            sections,
            archive,
            decoded,
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
        let bytes = crate::field_script::prepare(self.archive.section(6)?);
        symphonia_script::Program::decode(&bytes)?;
        Ok(bytes.into_owned())
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
        let mut models = super::source::Models::new(self.root, &self.decoded);
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
            .finish()
            .pop()
            .context("missing prepared field layer")?;
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
        self.decoded.animation(bytes)
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
    fn original_field_layers_need_no_physical_intermediates() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut panoramas = 0;
        for source in [
            crate::field::source_for_id(&extracted, 138)?,
            crate::field::source_for_id(&extracted, 332)?,
            crate::field::source_for_id(&extracted, 340)?,
            crate::field::source_for_id(&extracted, 344)?,
            crate::field::source_for_id(&extracted, 372)?,
            extracted.join("files/MAP/_custom.bin"),
        ] {
            let output = tempfile::tempdir()?;
            fs::write(
                output.path().join("sources.json"),
                b"not a cooked catalogue",
            )?;
            let map = Map::open(output.path(), &source)?;
            match (map.panorama()?, map.archive.optional_section(15)) {
                (Some(path), Some(bytes)) => {
                    let (width, height, rgba) = crate::tpl::decode(bytes)?.remove(0);
                    let image = crate::texture::pixels(&output.path().join(path))?;
                    assert_eq!(image.dimensions(), (width, height));
                    assert_eq!(image.as_raw(), &rgba);
                    panoramas += 1;
                }
                (None, None) => {}
                _ => panic!("field panorama presence differs from source"),
            }
            for index in FIELD_LAYERS {
                if map.archive.optional_section(index).is_none() {
                    continue;
                }
                let (part, glb) = map.part(index, &[])?;
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
            for part in map.layers(&[])?.0 {
                let stage = DrawStage::scenery(part.resource).context("unknown scenery layer")?;
                assert!(part.materials.iter().all(|material| {
                    material.draw_order / MODEL_DRAW_SPAN == stage as u32
                        && !(DrawStage::Actors.offset()..DrawStage::Foreground.offset())
                            .contains(&material.draw_order)
                        && material.depth_write == (part.resource != 2)
                }));
            }
            let [script, messages] = map.publish_script()?;
            assert_eq!(
                fs::read(output.path().join(script))?,
                map.archive.section(6)?
            );
            assert_eq!(
                serde_json::to_value(serde_json::from_slice::<
                    Vec<symphonia_script::message::Message>,
                >(&fs::read(
                    output.path().join(&messages)
                )?)?)?,
                serde_json::to_value(map.messages()?)?
            );
        }
        assert_eq!(panoramas, 1);
        Ok(())
    }

    #[test]
    #[ignore = "requires original fields; no prior cooked library or renderer"]
    fn original_line_primitives_bind_to_runtime_field_layers() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let stage = tempfile::tempdir()?;
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

    #[test]
    #[ignore = "requires original fields; binds native scenery and animation records directly"]
    fn original_declared_animations_bind_to_scenery_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let stage = tempfile::tempdir()?;
        let cooked = stage.path();
        let mut bindings = 0;
        let mut coverage = BTreeMap::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            for id in [330, 340] {
                let source = crate::field::source_for_id(&extracted, id)?;
                let map = crate::field::MapArchive::open(&source)?;
                let declarations = crate::field_resources::declarations(map.section(6)?)?;
                let decoded = super::super::decoded::Package::field_dependencies(
                    &extracted,
                    cooked,
                    &catalogue,
                    &declarations.resources,
                )?;
                let mut resources =
                    crate::field_resources::binding::Resources::decoded(&catalogue, &decoded);
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

    #[test]
    #[ignore = "requires both original discs and current RESONANCE_TEST_ASSETS"]
    fn original_prepared_map_layers_match_source_motion_on_both_discs() -> Result<()> {
        use std::collections::BTreeSet;

        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
        );
        let library = resonance_content::library::Library::open(&cooked, &[])?;
        let prepared: BTreeSet<_> = library
            .snapshot()
            .mappings(resonance_content::library::FIELDS)?
            .iter()
            .map(|(id, _)| id)
            .collect();
        let mut compared = BTreeSet::new();
        let mut archives = [0; 2];
        let mut layers = 0;
        let mut motions = 0;
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let executable = fs::read(extracted.join("sys/main.dol"))?;
            for phase in crate::field_catalogue::read(&executable)?.records {
                let Some(name) = phase.resource else { continue };
                let Some(source) = crate::field_resources::find_path(
                    &extracted.join("files"),
                    &format!("MAP/{name}"),
                )?
                else {
                    continue;
                };
                let id = u32::try_from(phase.id)?;
                let bytes = fs::read(extracted.join("files").join(&source))?;
                let map = crate::field::MapArchive::decode(&bytes)?;
                let Some(script) = map.optional_section(6) else {
                    continue;
                };
                ensure!(
                    prepared.contains(&id),
                    "original field {id} ({source}) is missing from current mappings"
                );
                archives[disc - 1] += 1;
                // Compare each game identity, even when several IDs deliberately
                // refer to the same native bytes. Disc variants remain distinct.
                if !compared.insert((id, crate::digest(&bytes))) {
                    continue;
                }
                let mut snapshot = library.snapshot();
                let (_, field) =
                    resonance_content::field::document::capture_scene(&mut snapshot, id, || false)?;
                assert_eq!(
                    &*snapshot.read(&field.script, 64 * 1024 * 1024)?,
                    crate::field_script::prepare(script).as_ref(),
                    "{source} scenario"
                );
                assert_eq!(
                    field
                        .parts
                        .iter()
                        .map(|part| usize::from(part.resource))
                        .collect::<Vec<_>>(),
                    FIELD_LAYERS
                        .into_iter()
                        .filter(|&index| map.optional_section(index).is_some())
                        .collect::<Vec<_>>(),
                    "{source} scenery layers"
                );
                for part in &field.parts {
                    let index = usize::from(part.resource);
                    let resource = map.section(index)?;
                    let glb = Glb::read(&cooked.join(&part.mesh))?;
                    crate::geometry::compare_field_layer(
                        resource,
                        part,
                        &glb.json,
                        DrawStage::scenery(part.resource).context("unknown scenery layer")? as u32,
                    )
                    .with_context(|| format!("{source}/{index}"))?;
                    let expected = map
                        .optional_section(index + 1)
                        .map(|animation| {
                            crate::animation::motion(
                                animation,
                                &resource[crate::geometry::skeleton_range(resource)?],
                            )
                        })
                        .transpose()
                        .with_context(|| format!("{source}/{index} autoplay"))?;
                    let clips = part
                        .clips
                        .iter()
                        .filter(|clip| clip.animation_resource.is_none())
                        .collect::<Vec<_>>();
                    assert_eq!(part.autoplay, expected.is_some(), "{source}/{index}");
                    assert_eq!(
                        clips.len(),
                        usize::from(expected.is_some()),
                        "{source}/{index}"
                    );
                    if let Some(motion) = expected {
                        let clip = clips[0];
                        assert_eq!(
                            clip.duration_seconds,
                            motion.duration_frames / resonance_content::animation::FRAME_HZ,
                            "{source}/{index} duration"
                        );
                        let stored = resonance_content::animation::Motion::decode(&fs::read(
                            cooked.join(&clip.motion),
                        )?)?;
                        stored.validate_bones(part.bone_names.len())?;
                        assert_eq!(
                            serde_json::to_value(stored)?,
                            serde_json::to_value(motion)?,
                            "{source}/{index}"
                        );
                        motions += 1;
                    }
                    assert!(glb.json.get("animations").is_none(), "{source}/{index}");
                    layers += 1;
                }
            }
        }
        assert!(archives.iter().all(|&count| count > 0));
        assert_eq!(
            compared.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>(),
            prepared
        );
        assert!(
            layers > 0 && motions > 0,
            "MAP comparison omitted geometry or autoplay"
        );
        eprintln!(
            "MAP source/runtime comparison: {archives:?} archives by disc, {} unique sources, {layers} layers, {motions} autoplay motions",
            compared.len()
        );
        Ok(())
    }
}
