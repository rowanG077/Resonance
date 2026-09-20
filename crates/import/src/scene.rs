use crate::read::u32 as u32_at;
use crate::{animation, digest, geometry, glow, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{CameraKey, SceneClip, ScenePart, TitleGlow, TitleScene};
use std::{fs, path::Path};
pub(crate) mod binding;
pub(crate) mod glb;
mod projection;
pub(crate) mod title;

fn camera(track: crate::all_assets::CameraTrack) -> Result<Vec<CameraKey>> {
    ensure!(
        track.transforms.len() >= 2 && track.transforms.len() == track.targets.len(),
        "title camera needs matching position and target tracks"
    );
    track
        .transforms
        .into_iter()
        .zip(track.targets)
        .map(|(position, target)| {
            ensure!(
                position.time == target.time,
                "camera tracks have different times"
            );
            Ok(CameraKey {
                time: position.time,
                position: position.position,
                target: target.position,
            })
        })
        .collect()
}

pub(crate) fn title_source(extracted: &Path, executable: &[u8]) -> Result<String> {
    // The title field owns this renderer in the native phase catalogue.
    let phases = crate::field_catalogue::read(executable)?;
    let mut title = phases
        .records
        .iter()
        .filter(|phase| phase.render_before_objects.as_deref() == Some("fn_8002F440"));
    let resource = title
        .next()
        .context("missing title field declaration")?
        .resource
        .as_deref()
        .context("title field has no resource")?;
    ensure!(title.next().is_none(), "ambiguous title field declarations");
    resonance_content::validate_asset_path(resource)?;
    crate::field_resources::resolve_path(&extracted.join("files"), &format!("MAP/{resource}"))
}

pub(crate) fn bind_title(output: &Path, disc: u8, recipe: &title::Recipe) -> Result<TitleScene> {
    let map = binding::Map::open(output, disc, &recipe.field.path, &recipe.field.sha256)?;
    let mut parts = Vec::new();
    let mut feather = None;
    let mut reflection = None;
    let mut landing = None;
    let mut landing_loop = None;
    for index in [0, 2, 17, 18, 20, 21, 22] {
        let order = [0, 20, 21, 17, 18, 22, 2]
            .iter()
            .position(|&part| part == index)
            .unwrap();
        let (part, glb) = map.title_part(index, order as u32)?;
        let (gltf, binary) = (&glb.json, &glb.binary);
        if index == 17 {
            feather = Some(glow::positions(
                gltf,
                binary,
                "Fz_Bone01",
                glam::Vec3::Z * 5.,
                0,
                730,
            )?);
            let points = glow::positions(gltf, binary, "Dummy", glam::Vec3::Z * 5., 0, 730)?;
            ensure!(
                points.iter().all(|p| *p == points[0]),
                "landing attachment unexpectedly moves"
            );
            landing = Some(points[0]);
            landing_loop = Some(glow::positions(
                gltf,
                binary,
                "Dummy",
                glam::Vec3::Z * 5.,
                1,
                360,
            )?);
        }
        if index == 18 {
            reflection = Some(glow::positions(
                gltf,
                binary,
                "Rf_Fez_Ref_120",
                glam::Vec3::ZERO,
                0,
                730,
            )?);
        }
        parts.push(part);
    }
    let texture = glow::texture(output, disc, &recipe.effects)?;
    let script_path = format!(
        "{}/script.ssb",
        map.section(6).context("missing title script")?
    );
    let script_bytes = fs::read(output.join(&script_path))?;
    symphonia_script::Program::decode(&script_bytes)?;
    parts
        .iter_mut()
        .find(|part| part.resource == 2)
        .context("missing title lighting layer")?
        .texture_animations = crate::texture_animation::bind_title(output, disc, &script_bytes)?;
    let cameras = [16, 23]
        .into_iter()
        .map(|index| {
            let path = format!(
                "{}/camera.json",
                map.section(index).context("missing title camera")?
            );
            camera(serde_json::from_slice(&fs::read(output.join(path))?)?)
        })
        .collect::<Result<_>>()?;
    Ok(TitleScene {
        script: resonance_content::ScriptAsset {
            path: script_path,
            sha256: digest(&script_bytes),
        },
        source_sha256: recipe.field.sha256.clone(),
        code_source_sha256: recipe.executable_sha256.clone(),
        parts,
        glow: TitleGlow {
            texture,
            source_sha256: recipe.effects.sha256.clone(),
            feather: feather.context("missing feather")?,
            reflection: reflection.context("missing reflection")?,
            landing: landing.context("missing landing")?,
            landing_loop: landing_loop.context("missing landing loop")?,
        },
        cameras,
        fov_degrees: 27.,
    })
}

pub(crate) struct PartSource<'a> {
    pub name: &'a str,
    pub source: &'a [u8],
    pub resource: u16,
    pub draw_order: u32,
    pub depth_write: bool,
    pub translation: [f32; 3],
    pub autoplay: Option<&'a [u8]>,
    pub animation_slots: &'a [usize],
    pub clip_prefix: &'a str,
    pub extra_clips: &'a [SourceClip<'a>],
    pub shared_clips: &'a [(u32, animation::AuthoredAnimation)],
    pub texture_animations: Vec<resonance_content::TextureAnimation>,
}

pub(crate) struct SourceClip<'a> {
    pub slot: u16,
    pub bytes: &'a [u8],
    pub resource: Option<u32>,
}

enum ClipData<'a> {
    Source(&'a [u8]),
    Cooked(&'a animation::AuthoredAnimation),
}

/// Compile source geometry and materials into ordinary runtime assets.
/// The extra glTF and vertex data are available for offline attachment baking.
pub(crate) fn cook_part(
    spec: PartSource<'_>,
    output: &Path,
) -> Result<(ScenePart, serde_json::Value, Vec<u8>)> {
    let name = spec.name;
    let intermediate = output.join("intermediate").join(name);
    fs::create_dir_all(&intermediate)?;
    let geometry::DecodedGeometry {
        manifest,
        mut gltf,
        mut binary,
    } = geometry::decode_section(
        spec.source,
        geometry::DecodeMode::Runtime,
        |texture, rgba| {
            geometry::write_png(
                &intermediate.join(&texture.image),
                texture.width,
                texture.height,
                rgba,
            )
        },
    )?;
    let physical = crate::all_assets::physical_scene::from_geometry(
        &manifest,
        &mut gltf,
        crate::all_assets::physical_scene::TextureSource::Local {
            catalogue: String::new(),
        },
    )?;
    let mut textures = Vec::new();
    for texture in &manifest.textures {
        let path = format!("{name}/texture_{:03}.ktx2", texture.index);
        let destination = output.join(&path);
        fs::create_dir_all(destination.parent().context("texture directory")?)?;
        crate::texture::cook_png(&intermediate.join(&texture.image), &destination)?;
        fs::remove_file(intermediate.join(&texture.image))?;
        textures.push(path);
    }
    let mut part = projection::project(physical, &mut gltf, textures)?;
    for material in &mut part.materials {
        material.draw_order += spec.draw_order * 65536;
        material.depth_write = spec.depth_write;
    }
    let mut clips = Vec::new();
    let autoplay = spec.autoplay.is_some();
    let bindings = if autoplay
        || !spec.animation_slots.is_empty()
        || !spec.extra_clips.is_empty()
        || !spec.shared_clips.is_empty()
    {
        let (_, resource) = geometry::model_resource(spec.source)?;
        let range = geometry::skeleton_range(resource)?;
        Some(animation::ModelBindings::read(&resource[range])?)
    } else {
        None
    };
    if let Some(animation) = spec.autoplay {
        let duration_seconds = animation::bake(
            animation,
            bindings.as_ref().unwrap(),
            &mut gltf,
            &mut binary,
            &format!("field-{}", spec.resource),
        )?;
        clips.push(SceneClip {
            resource_slot: 0,
            duration_seconds,
            animation_resource: None,
            secondary_pose_nodes: Vec::new(),
        });
    }
    if !spec.animation_slots.is_empty() {
        let source = spec.source;
        let schedule = spec.animation_slots;
        for (clip, slot) in schedule.iter().enumerate() {
            let offset = u32_at(source, *slot)? as usize;
            let duration_seconds = animation::bake(
                source.get(offset..).context("animation outside section")?,
                bindings.as_ref().unwrap(),
                &mut gltf,
                &mut binary,
                &format!("{}-{clip}", spec.clip_prefix),
            )?;
            clips.push(SceneClip {
                resource_slot: *slot as u16,
                duration_seconds,
                animation_resource: None,
                secondary_pose_nodes: Vec::new(),
            });
        }
    }
    let mut extra: Vec<_> = spec
        .extra_clips
        .iter()
        .map(|clip| (clip.resource, clip.slot, ClipData::Source(clip.bytes)))
        .chain(
            spec.shared_clips
                .iter()
                .map(|(id, animation)| (Some(*id), 12, ClipData::Cooked(animation))),
        )
        .collect();
    if !spec.shared_clips.is_empty() {
        extra.sort_by_key(|(resource, slot, _)| (*resource, *slot));
        ensure!(
            extra
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) != (pair[1].0, pair[1].1)),
            "duplicate shared animation binding"
        );
    }
    for (resource, slot, data) in extra {
        let bindings = bindings.as_ref().unwrap();
        let motion = match data {
            ClipData::Source(bytes) => bindings.motion(bytes)?,
            ClipData::Cooked(animation) => animation.motion(bindings)?,
        };
        let duration_seconds = animation::bake_motion(
            &motion,
            &mut gltf,
            &mut binary,
            &format!("{}-{slot}", spec.clip_prefix),
        )?;
        clips.push(SceneClip {
            resource_slot: slot,
            duration_seconds,
            animation_resource: resource,
            secondary_pose_nodes: motion
                .tracks
                .iter()
                .filter(|track| track.times.len() > 2)
                .map(|track| track.bone)
                .collect(),
        });
    }
    let glb = pack_glb(&gltf, &mut binary)?;
    // Fields share character packages; a new clip set must not overwrite the
    // mesh still referenced by another field's manifest.
    let mesh = format!("{name}/{}.glb", digest(&glb));
    write_atomic(&output.join(&mesh), &glb)?;
    part.resource = spec.resource;
    part.mesh = mesh;
    part.clips = clips;
    part.autoplay = autoplay;
    part.texture_animations = spec.texture_animations;
    part.translation = spec.translation;
    Ok((part, gltf, binary))
}

pub(crate) fn pack_glb(gltf: &serde_json::Value, binary: &mut Vec<u8>) -> Result<Vec<u8>> {
    let mut json = serde_json::to_vec(gltf)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + binary.len();
    ensure!(
        length <= u32::MAX as usize,
        "GLB exceeds its 32-bit size limit"
    );
    let mut glb = Vec::with_capacity(length);
    for value in [0x46546c67, 2, length as u32, json.len() as u32, 0x4e4f534a] {
        glb.extend(value.to_le_bytes());
    }
    glb.extend(json);
    for value in [binary.len() as u32, 0x004e4942] {
        glb.extend(value.to_le_bytes());
    }
    glb.extend_from_slice(binary);
    Ok(glb)
}

#[test]
#[ignore = "requires both original extracted discs; camera projection only"]
fn original_title_camera_projection_preserves_positions_targets_and_times() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in ["disc1", "disc2"] {
        let extracted = root.join(disc);
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let source = title_source(&extracted, &executable)?;
        let map = crate::field::MapArchive::open(&extracted.join("files").join(source))?;
        for index in [16, 23] {
            let bytes = map.section(index)?;
            let keys = camera(crate::all_assets::camera(bytes)?)?;
            assert_eq!(keys.len(), u32_at(bytes, 8)? as usize);
            for (index, key) in keys.iter().enumerate() {
                let position = 12 + index * 36;
                let target = u32_at(bytes, 4)? as usize + 12 + index * 20;
                assert_eq!(key.time, crate::read::f32(bytes, position)?);
                assert_eq!(key.time, crate::read::f32(bytes, target)?);
                for axis in 0..3 {
                    assert_eq!(
                        key.position[axis],
                        crate::read::f32(bytes, position + 4 + axis * 4)?
                    );
                    assert_eq!(
                        key.target[axis],
                        crate::read::f32(bytes, target + 4 + axis * 4)?
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires both original discs and cook-all; binds shared assets without codecs"]
fn original_title_shared_binding_preserves_native_clips_and_assets() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let output = root.join("all-assets");
    for disc in [1, 2] {
        let extracted = root.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let source = title_source(&extracted, &executable)?;
        let raw = crate::field::MapArchive::open(&extracted.join("files").join(&source))?;
        let physical = binding::Map::open(&output, disc, &source, &raw.source_sha256)?;
        let recipe = title::Recipe::bind(&output, disc)?;
        assert_eq!(
            serde_json::to_value(&recipe)?,
            serde_json::to_value(title::Recipe::read(&extracted, &executable)?)?
        );
        let title = bind_title(&output, disc, &recipe)?;
        assert_eq!(title.source_sha256, raw.source_sha256);
        assert_eq!(fs::read(output.join(&title.script.path))?, raw.section(6)?);
        assert!(title.glow.texture.starts_with("assets/"));
        assert_eq!(title.parts.len(), 7);
        for part in &title.parts {
            assert!(part.textures.iter().all(|path| path.starts_with("assets/")));
            let index = usize::from(part.resource);
            let source = raw.section(index)?;
            let (_, model) = geometry::model_resource(source)?;
            let bindings =
                animation::ModelBindings::read(&model[geometry::skeleton_range(model)?])?;
            let directory = physical.section(index).context("title section")?;
            let directory = if part.autoplay {
                directory
            } else {
                format!("{directory}/0")
            };
            let (shared, mut expected) = binding::model(&output, &directory)?;
            assert_eq!(part.textures, shared.textures);
            for (clip, binding) in part.clips.iter().enumerate() {
                let bytes = if binding.resource_slot == 0 {
                    raw.section(index + 1)?
                } else {
                    &source[u32_at(source, usize::from(binding.resource_slot))? as usize..]
                };
                let name = if binding.resource_slot == 0 {
                    format!("field-{index}")
                } else {
                    format!("title-{index}-{clip}")
                };
                let duration = animation::bake(
                    bytes,
                    &bindings,
                    &mut expected.json,
                    &mut expected.binary,
                    &name,
                )?;
                assert_eq!(binding.duration_seconds, duration);
            }
            let expected = pack_glb(&expected.json, &mut expected.binary)?;
            assert_eq!(
                fs::read(output.join(&part.mesh))?,
                expected,
                "disc{disc} title part {index}"
            );
        }
        for (camera_index, section) in [16, 23].into_iter().enumerate() {
            assert_eq!(
                serde_json::to_value(&title.cameras[camera_index])?,
                serde_json::to_value(camera(crate::all_assets::camera(raw.section(section)?)?)?)?
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires an original executable; mutates only a temporary source declaration"]
fn title_declaration_resolves_renamed_case_alias_and_rejects_missing_source() -> Result<()> {
    let root = crate::temporary_path(&std::env::temp_dir().join("title-declaration"));
    let result = (|| -> Result<()> {
        let mut executable = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
        )?;
        let declaration = crate::dol::slice(&executable, 0x8017be78, 12)?;
        let offset = declaration.as_ptr() as usize - executable.as_ptr() as usize;
        executable[offset..offset + 12].copy_from_slice(b"renamed.bin\0");
        fs::create_dir_all(root.join("files/Map"))?;
        fs::write(root.join("files/Map/ReNaMeD.bin"), [])?;
        assert_eq!(title_source(&root, &executable)?, "Map/ReNaMeD.bin");
        fs::remove_file(root.join("files/Map/ReNaMeD.bin"))?;
        assert!(title_source(&root, &executable).is_err());
        Ok(())
    })();
    if root.exists() {
        fs::remove_dir_all(root)?;
    }
    result
}
