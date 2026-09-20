//! Assemble field layers from physical meshes and curves without source conversion.
use super::{SceneClip, ScenePart, glb::Glb};
use crate::{
    all_assets::{
        PhysicalDirectory,
        physical_scene::{Scene, TextureSource},
    },
    animation::{AuthoredAnimation, ModelBindings},
};
use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) struct Map<'a> {
    root: &'a Path,
    directory: String,
    sections: PhysicalDirectory,
}

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
    owned(&scene.mesh)?;
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
        TextureSource::Caller => anyhow::bail!("model requires caller-supplied textures"),
    };
    let mut glb = Glb::read(&root.join(&scene.mesh))?;
    ensure!(
        glb.json["animations"].as_array().is_none_or(Vec::is_empty),
        "physical mesh already has animations"
    );
    let mut part = super::projection::project(scene, &mut glb.json, textures)?;
    let bytes = super::pack_glb(&glb.json, &mut glb.binary)?;
    part.mesh = format!("game/meshes/{}.glb", crate::digest(&bytes));
    crate::write_atomic(&root.join(&part.mesh), &bytes)?;
    Ok((part, glb))
}

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
    pub(crate) fn open(
        root: &'a Path,
        disc: u8,
        source: &str,
        source_sha256: &str,
    ) -> Result<Self> {
        ensure!((1..=2).contains(&disc), "invalid field source disc");
        resonance_content::validate_asset_path(source)?;
        let sources: BTreeMap<String, Vec<String>> = read(root, "sources.json")?;
        let key = format!("disc{disc}/{source}");
        let paths = sources
            .get(&key)
            .with_context(|| format!("missing cooked {key}; rerun cook-all"))?;
        let [directory] = paths.as_slice() else {
            anyhow::bail!("expected one cooked field directory for {key}");
        };
        ensure!(
            *directory == format!("assets/{source_sha256}"),
            "cooked field source digest mismatch"
        );
        let members: Vec<String> = read(root, &format!("{directory}/cabinet.json"))?;
        let [member] = members.as_slice() else {
            anyhow::bail!("field archive needs one cooked payload");
        };
        resonance_content::validate_asset_path(member)?;
        let directory = format!("{directory}/{member}");
        let sections: PhysicalDirectory = read(root, &format!("{directory}/members.json"))?;
        sections.validate()?;
        ensure!(sections.count <= 256, "invalid cooked field section count");
        Ok(Self {
            root,
            directory,
            sections,
        })
    }

    pub(crate) fn section(&self, index: usize) -> Option<String> {
        self.sections
            .members
            .get(index)?
            .map(|canonical| format!("{}/{canonical}", self.directory))
    }

    fn model(&self, directory: &str, index: usize, order: u32) -> Result<(ScenePart, Glb)> {
        let (mut part, glb) = model(self.root, directory)?;
        part.resource = index.try_into()?;
        for material in &mut part.materials {
            ensure!(
                material.draw_order < 65536,
                "invalid physical field draw order"
            );
            material.draw_order += order * 65536;
            material.depth_write = index != 2;
        }
        Ok((part, glb))
    }

    fn part(
        &self,
        index: usize,
        order: u32,
        extra: &[(u32, AuthoredAnimation)],
    ) -> Result<(ScenePart, Glb, Option<Vec<u8>>)> {
        let directory = self.section(index).context("missing cooked field layer")?;
        let (mut part, mut glb) = self.model(&directory, index, order)?;
        let autoplay = self.section(index + 1);
        let bytes = if autoplay.is_some() || !extra.is_empty() {
            let model = self.bindings(&directory, &part)?;
            if let Some(animation) = autoplay {
                let animation: AuthoredAnimation =
                    read(self.root, &format!("{animation}/animation.json"))?;
                bind_clip(&mut part, &mut glb, &model, &animation, None)?;
                part.autoplay = true;
            }
            // Script targets and animation handles are independent. Each
            // scenery rig needs its own binding of every declared standalone clip.
            for (resource, animation) in extra {
                ensure!(
                    !part
                        .clips
                        .iter()
                        .any(|clip| clip.animation_resource == Some(*resource)),
                    "duplicate field animation resource {resource:#x}"
                );
                bind_clip(&mut part, &mut glb, &model, animation, Some(*resource))?;
            }
            let bytes = super::pack_glb(&glb.json, &mut glb.binary)?;
            part.mesh = format!("fields/scenes/{}.glb", crate::digest(&bytes));
            Some(bytes)
        } else {
            None
        };
        Ok((part, glb, bytes))
    }

    fn bindings(&self, directory: &str, part: &ScenePart) -> Result<ModelBindings> {
        bindings(self.root, directory, part)
    }

    pub(super) fn title_part(&self, index: usize, order: u32) -> Result<(ScenePart, Glb)> {
        let directory = self
            .section(index)
            .context("missing cooked title section")?;
        let slots: &[u16] = match index {
            0 | 2 => &[],
            17 | 18 | 21 => &[12, 36],
            20 | 22 => &[12],
            _ => anyhow::bail!("unsupported title section {index}"),
        };
        let mut clips = Vec::new();
        let model = if slots.is_empty() {
            clips.push((
                0,
                self.section(index + 1).context("missing title autoplay")?,
            ));
            directory.clone()
        } else {
            let members: PhysicalDirectory = read(self.root, &format!("{directory}/members.json"))?;
            members.validate()?;
            let member = |index| -> Result<String> {
                Ok(format!(
                    "{directory}/{}",
                    members
                        .members
                        .get(index)
                        .copied()
                        .flatten()
                        .context("missing title package member")?
                ))
            };
            for &slot in slots {
                clips.push((slot, member(usize::from(slot / 4 - 1))?));
            }
            member(0)?
        };
        let (mut part, mut glb) = self.model(&model, index, order)?;
        let bindings = self.bindings(&model, &part)?;
        for (clip, (slot, directory)) in clips.into_iter().enumerate() {
            let animation: AuthoredAnimation =
                read(self.root, &format!("{directory}/animation.json"))?;
            let name = if slot == 0 {
                format!("field-{index}")
            } else {
                format!("title-{index}-{clip}")
            };
            let duration_seconds = crate::animation::bake_motion(
                &animation.motion(&bindings)?,
                &mut glb.json,
                &mut glb.binary,
                &name,
            )?;
            part.clips.push(SceneClip {
                resource_slot: slot,
                duration_seconds,
                animation_resource: None,
                secondary_pose_nodes: Vec::new(),
            });
        }
        part.autoplay = slots.is_empty();
        part.translation[2] = if index == 17 { 5. } else { 0. };
        let bytes = super::pack_glb(&glb.json, &mut glb.binary)?;
        part.mesh = format!("title-scene/{index:02}/{}.glb", crate::digest(&bytes));
        crate::write_atomic(&self.root.join(&part.mesh), &bytes)?;
        Ok((part, glb))
    }

    pub(crate) fn layers(
        &self,
        extra: &[(u32, AuthoredAnimation)],
    ) -> Result<(Vec<ScenePart>, Vec<resonance_content::field::Door>)> {
        let mut parts = Vec::new();
        let mut doors = Vec::new();
        for (order, index) in [0, 2, 12]
            .into_iter()
            .filter(|&index| index != 12 || self.section(index).is_some())
            .enumerate()
        {
            let (part, glb, bytes) = self
                .part(index, order as u32, extra)
                .with_context(|| format!("bind field layer {index}"))?;
            if let Some(bytes) = bytes {
                crate::write_atomic(&self.root.join(&part.mesh), &bytes)?;
            }
            if index == 0 {
                doors = crate::field_doors::cook(&glb.json)?;
            }
            parts.push(part);
        }
        Ok((parts, doors))
    }
}

fn bind_clip(
    part: &mut ScenePart,
    glb: &mut Glb,
    model: &ModelBindings,
    animation: &AuthoredAnimation,
    resource: Option<u32>,
) -> Result<()> {
    let name = match resource {
        Some(id) => format!("field-{}-{id:x}", part.resource),
        None => format!("field-{}", part.resource),
    };
    let duration_seconds = crate::animation::bake_motion(
        &animation.motion(model)?,
        &mut glb.json,
        &mut glb.binary,
        &name,
    )?;
    let secondary_pose_nodes = if resource.is_some() {
        serde_json::from_value(
            glb.json["animations"]
                .as_array()
                .context("cooked animations")?
                .last()
                .context("cooked field clip")?["extras"]["secondary_pose_nodes"]
                .clone(),
        )?
    } else {
        Vec::new()
    };
    part.clips.push(SceneClip {
        resource_slot: if resource.is_some() { 12 } else { 0 },
        duration_seconds,
        animation_resource: resource,
        secondary_pose_nodes,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "requires original fields and cooked library; no codecs or renderer"]
    fn original_line_primitives_bind_to_runtime_field_layers() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = local.join("all-assets");
        for (source, section, expected) in [
            ("MAP/cot_i00.bin", 0, vec![12, 13]),
            ("MAP/hol_d00.bin", 2, vec![1]),
        ] {
            let original =
                crate::field::MapArchive::open(&local.join("extracted/disc1/files").join(source))?;
            let resource = original.section(section)?;
            crate::geometry::preflight_section(resource)?;
            let map = Map::open(&cooked, 1, source, &original.source_sha256)?;
            let (part, glb, _) = map.part(section, 0, &[])?;
            crate::geometry::compare_field_layer(resource, &part, &glb.json, 0)?;
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
    fn exact_cabinet_names_and_aliases_bind_shared_geometry_and_reject_missing_files() -> Result<()>
    {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-map-binding"));
        fs::create_dir(&root)?;
        let result = (|| -> Result<()> {
            let hash = crate::digest(b"field archive");
            let directory = format!("assets/{hash}");
            let model = format!("{directory}/LONGNA~1.BIN/0");
            let write = |path: &str, value: serde_json::Value| {
                crate::write_atomic(&root.join(path), &serde_json::to_vec(&value)?)
            };
            write(
                "sources.json",
                json!({"disc1/MAP/long_name.bin":[directory]}),
            )?;
            write(
                &format!("{directory}/cabinet.json"),
                json!(["LONGNA~1.BIN"]),
            )?;
            write(
                &format!("{directory}/LONGNA~1.BIN/members.json"),
                json!({"count":3,"members":[0,null,0]}),
            )?;
            let mesh = format!("{model}/body.glb");
            let texture = format!("{model}/palettes/texture.ktx2");
            let catalogue = format!("{model}/palettes/textures.json");
            write(
                &catalogue,
                json!({"textures":[{
                    "dimensions":[1,1], "format":"rgba8", "palette":null, "images":[texture],
                    "sampler":{"wrap":["clamp","clamp"],"min_filter":"linear","mag_filter":"linear",
                        "lod":{"bias":0.,"min":0,"max":0,"edge":false}}
                }]}),
            )?;
            write(
                &format!("{model}/scene.json"),
                json!({"mesh":mesh,"textures":{"kind":"local","catalogue":catalogue},
                "draws":[{"source_index":0,"mesh":0,"model_node":1,"draw_order":7,
                    "name":"surface","index_count":3,"color":[1.,1.,1.,1.],"preview_blend":false,
                    "recipe":crate::geometry::MaterialRecipe::parse(&[5],0)?,"textures":[]}],
                "bone_names":["frame","hinge"]}),
            )?;
            let gltf = json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":36}],
                "nodes":[{"name":"frame"},{"name":"hinge","mesh":0}],
                "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
                "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
                "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}]});
            let bytes = super::super::pack_glb(&gltf, &mut vec![0; 36])?;
            crate::write_atomic(&root.join(&mesh), &bytes)?;
            crate::write_atomic(&root.join(&texture), b"shared texture")?;
            let map = Map::open(&root, 1, "MAP/long_name.bin", &hash)?;
            let (bound, glb, rewritten) = map.part(2, 1, &[])?;
            assert!(bound.mesh.starts_with("game/meshes/"));
            assert_eq!(Glb::read(&root.join(&bound.mesh))?.json, glb.json);
            assert_eq!(bound.textures.as_slice(), std::slice::from_ref(&texture));
            assert_eq!(bound.materials[0].draw_order, 65543);
            assert!(!bound.materials[0].depth_write);
            assert_eq!(glb.binary, [0; 36]);
            assert!(rewritten.is_none());
            let animation = json!({"duration_frames":2.,"flags":0,"names":["hinge"],"declared_name_bytes":6,
                "descriptors":[{"name":null,"first_track":0,"track_count":1,"force_index_binding":false,"flags":0}],
                "tracks":[{"node":0,"kind":1,"flags":1,"period_frames":2.,"times":[0.,1.,2.],
                    "channels":[{"component":"translation","interpolation":"linear","scalar":"f32","scale":1.,
                        "values":[[0.,0.,0.],[2.,0.,0.],[4.,0.,0.]]}]}]});
            let extra = [(0x10004, serde_json::from_value(animation.clone())?)];
            assert!(map.part(2, 1, &extra).is_err(), "missing rig must fail");
            write(
                &format!("{model}/motion-bindings.json"),
                json!({"node_ids":[0,1],"names":["frame","hinge"]}),
            )?;
            let (external, glb, rewritten) = map.part(2, 1, &extra)?;
            assert!(!external.autoplay);
            assert_eq!(external.clips.len(), 1);
            assert_eq!(external.clips[0].animation_resource, Some(0x10004));
            assert_eq!(external.clips[0].resource_slot, 12);
            assert_eq!(external.clips[0].secondary_pose_nodes, [1]);
            assert_eq!(
                glb.json["animations"][0]["channels"][0]["target"]["node"],
                1
            );
            assert_eq!(Glb::parse(&rewritten.unwrap())?.json, glb.json);
            assert_eq!(
                fs::read(root.join(&mesh))?,
                bytes,
                "physical mesh was mutated"
            );
            write(
                &format!("{directory}/LONGNA~1.BIN/members.json"),
                json!({"count":3,"members":[0,1,0]}),
            )?;
            write(
                &format!("{directory}/LONGNA~1.BIN/1/animation.json"),
                animation,
            )?;
            let paired = Map::open(&root, 1, "MAP/long_name.bin", &hash)?;
            let (part, glb, _) = paired.part(0, 0, &extra)?;
            assert!(part.autoplay);
            assert_eq!(part.clips.len(), 2);
            assert_eq!(part.clips[0].animation_resource, None);
            assert_eq!(part.clips[0].resource_slot, 0);
            assert_eq!(part.clips[1].animation_resource, Some(0x10004));
            assert_eq!(glb.json["animations"][0]["name"], "field-0");
            assert_eq!(glb.json["animations"][1]["name"], "field-0-10004");
            assert!(Map::open(&root, 2, "MAP/long_name.bin", &hash).is_err());
            assert!(Map::open(&root, 1, "MAP/long_name.bin", &"a".repeat(64)).is_err());
            let mut unsupported = gltf.clone();
            unsupported["meshes"][0]["primitives"][0]["attributes"]["_NORMAL_BASIS_1"] = json!(0);
            crate::write_atomic(
                &root.join(&mesh),
                &super::super::pack_glb(&unsupported, &mut vec![0; 36])?,
            )?;
            let error = map
                .part(0, 0, &[])
                .err()
                .context("normal basis was silently discarded")?;
            assert!(format!("{error:#}").contains("normal-basis consumer"));
            crate::write_atomic(&root.join(&mesh), &bytes)?;
            fs::remove_file(root.join(&texture))?;
            assert!(map.part(0, 0, &[]).is_err());
            write(
                &format!("{directory}/LONGNA~1.BIN/members.json"),
                json!({"count":3,"members":[2,null,0]}),
            )?;
            assert!(Map::open(&root, 1, "MAP/long_name.bin", &hash).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "requires original fields and cook-all records; writes derived meshes without codecs"]
    fn original_declared_animations_bind_to_scenery_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = local.join("all-assets");
        let mut bindings = 0;
        let mut coverage = BTreeMap::new();
        for disc in [1, 2] {
            let extracted = local.join(format!("extracted/disc{disc}"));
            let files = extracted.join("files");
            let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
            let mut resources =
                crate::field_resources::binding::Resources::open(&cooked, &extracted, &catalogue)?;
            for id in [330, 340] {
                let source = crate::field::source_for_id(&extracted, id)?;
                let map = crate::field::MapArchive::open(&source)?;
                let declarations = crate::field_resources::declarations(map.section(6)?)?;
                let sources =
                    crate::character::Sources::read(&files, &catalogue, &declarations.resources)?;
                let clips = sources.scene_clips(&mut resources)?;
                assert_eq!(clips.is_empty(), id == 330);
                let physical = Map::open(
                    &cooked,
                    disc,
                    source
                        .strip_prefix(&files)?
                        .to_str()
                        .context("field path")?,
                    &map.source_sha256,
                )?;
                let mut layers = 0;
                for (order, index) in [0, 2, 12].into_iter().enumerate() {
                    let Some(source) = map.optional_section(index) else {
                        continue;
                    };
                    layers += 1;
                    let (original, original_glb, original_bytes) =
                        physical.part(index, order as u32, &[])?;
                    let (part, glb, bytes) = physical.part(index, order as u32, &clips)?;
                    if clips.is_empty() {
                        assert_eq!(serde_json::to_value(part)?, serde_json::to_value(original)?);
                        assert_eq!(glb.json, original_glb.json);
                        assert_eq!(glb.binary, original_glb.binary);
                        assert_eq!(bytes, original_bytes);
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
                            motion.duration_frames / resonance_content::battle::pose::FRAME_HZ
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
                        assert_eq!(
                            glb.json["animations"][at]["name"],
                            format!("field-{index}-{resource:x}")
                        );
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
    #[ignore = "requires original MAP archives and cook-all on both discs; writes derived meshes without codecs"]
    fn original_map_layers_match_native_motion_binding_on_both_discs() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = local.join("all-assets");
        let sources: BTreeMap<String, Vec<String>> = read(&cooked, "sources.json")?;
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
                let bound = Map::open(&cooked, disc, source, &map.source_sha256)?;
                assert_eq!(bound.sections.count, map.sections.len(), "{source}");
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
                for (order, index) in [0, 2, 12].into_iter().enumerate() {
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
                            read(&cooked, &format!("{directory}/motion-bindings.json"))?;
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
                                read(&cooked, &format!("{animation_directory}/animation.json"))?;
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
                        assert!(bound.part(index, order as u32, &[]).is_err());
                        counts[3] += 1;
                        continue;
                    }
                    if crate::geometry::preflight_section(resource).is_ok() {
                        let (part, glb, _) = bound
                            .part(index, order as u32, &[])
                            .with_context(|| format!("{source}/{index}"))?;
                        crate::geometry::compare_field_layer(
                            resource,
                            &part,
                            &glb.json,
                            order as u32,
                        )
                        .with_context(|| format!("{source}/{index}"))?;
                        assert_eq!(part.autoplay, expected_motion.is_some());
                        if let Some(motion) = expected_motion {
                            assert_eq!(
                                part.clips[0].duration_seconds,
                                motion.duration_frames / resonance_content::battle::pose::FRAME_HZ
                            );
                            assert_eq!(glb.json["animations"][0]["name"], format!("field-{index}"));
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
