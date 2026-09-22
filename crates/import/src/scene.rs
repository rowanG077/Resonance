#[cfg(test)]
use crate::{animation, geometry, read::u32 as u32_at};
use crate::{digest, glow};
use anyhow::{Context, Result, ensure};
use resonance_content::{CameraKey, TitleGlow, TitleScene};
#[cfg(test)]
use std::fs;
use std::path::Path;
pub(crate) mod binding;
pub(crate) mod decoded;
pub(crate) mod glb;
mod projection;
pub(crate) mod source;
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
    let mut title = phases.records.iter().filter(|phase| {
        phase.render_before_objects == Some(crate::field_catalogue::NativeCallback::TITLE_SCENE)
    });
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

pub(crate) fn bind_title(
    extracted: &Path,
    output: &Path,
    recipe: &title::Recipe,
    executable: &[u8],
) -> Result<TitleScene> {
    ensure!(
        crate::digest(executable) == recipe.executable_sha256,
        "title executable digest mismatch"
    );
    let map = binding::Map::open(output, &extracted.join("files").join(&recipe.field.path))?;
    ensure!(
        map.source_sha256 == recipe.field.sha256,
        "title field source digest mismatch"
    );
    let mut parts = Vec::new();
    let mut skeletons = std::collections::BTreeMap::new();
    for (order, index) in [0, 20, 21, 17, 18, 22, 2].into_iter().enumerate() {
        let (part, glb) = map.title_part(index, order as u32)?;
        if !part.autoplay {
            skeletons.insert(
                part.resource,
                glow::skeleton(&glb.json, part.bone_names.len())?,
            );
        }
        parts.push(part);
    }
    let texture = glow::texture(extracted, output, &recipe.effects)?;
    let script_bytes = map.script()?;
    let [script_path, _] = map.publish_script()?;
    parts
        .iter_mut()
        .find(|part| part.resource == 2)
        .context("missing title lighting layer")?
        .texture_animations = crate::texture_animation::bind_title(executable, &script_bytes)?;
    let cameras = [16, 23]
        .into_iter()
        .map(|index| camera(map.camera(index)?))
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
            skeletons,
        },
        cameras,
        fov_degrees: 27.,
    })
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
#[ignore = "requires both original discs; fresh output, no cooked intermediates"]
fn original_title_cooks_from_sources_and_preserves_native_clips_and_assets() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    for disc in [1, 2] {
        let temporary = tempfile::tempdir()?;
        let output = temporary.path();
        let extracted = root.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let source = title_source(&extracted, &executable)?;
        let raw = crate::field::MapArchive::open(&extracted.join("files").join(&source))?;
        crate::cook_title(&extracted, output)?;
        let manifest: resonance_content::TitleAssets =
            serde_json::from_slice(&fs::read(output.join("title.json"))?)?;
        let title = manifest.scene.context("missing title scene")?;
        assert!(!output.join("sources.json").exists());
        assert!(!output.join("data").exists());
        assert_eq!(title.source_sha256, raw.source_sha256);
        assert_eq!(fs::read(output.join(&title.script.path))?, raw.section(6)?);
        assert!(title.glow.texture.starts_with("textures/"));
        assert_eq!(title.parts.len(), 7);
        for part in &title.parts {
            assert!(
                part.textures
                    .iter()
                    .all(|path| path.starts_with("textures/"))
            );
            let index = usize::from(part.resource);
            let source = raw.section(index)?;
            let (_, model) = geometry::model_resource(source)?;
            let bindings =
                animation::ModelBindings::read(&model[geometry::skeleton_range(model)?])?;
            // Build the semantic physical reference privately; frozen libraries may
            // use an older model schema and are never rewritten by this test.
            let reference = tempfile::tempdir()?;
            let mut failures = Vec::new();
            assert!(crate::all_assets::geometry::cook(
                model,
                "model",
                reference.path(),
                None,
                crate::all_assets::geometry::Input::Member,
                &mut crate::scene::decoded::Package::default(),
                &mut |path, result| {
                    if let Err(error) = result {
                        failures.push(format!("{path}: {error:#}"));
                    }
                }
            ));
            assert!(failures.is_empty(), "{}", failures.join("\n"));
            let (shared, expected) = binding::model(reference.path(), "model")?;
            assert_eq!(part.textures.len(), shared.textures.len());
            for (actual, expected) in part.textures.iter().zip(&shared.textures) {
                assert_eq!(
                    fs::read(output.join(actual))?,
                    fs::read(reference.path().join(expected))?
                );
            }
            for binding in &part.clips {
                let bytes = if binding.resource_slot == 0 {
                    raw.section(index + 1)?
                } else {
                    &source[u32_at(source, usize::from(binding.resource_slot))? as usize..]
                };
                let motion = bindings.motion(bytes)?;
                let published = resonance_content::animation::Motion::decode(&fs::read(
                    output.join(&binding.motion),
                )?)?;
                assert_eq!(
                    serde_json::to_value(&published)?,
                    serde_json::to_value(&motion)?
                );
                assert_eq!(
                    binding.duration_seconds,
                    motion.duration_frames / resonance_content::animation::FRAME_HZ,
                );
                if let Some(skeleton) = title.glow.skeletons.get(&part.resource) {
                    let original = glow::skeleton(&expected.json, part.bone_names.len())?;
                    assert_eq!(
                        serde_json::to_value(skeleton)?,
                        serde_json::to_value(&original)?
                    );
                    let bones: Vec<_> = ["Fz_Bone01", "Dummy", "Rf_Fez_Ref_120"]
                        .into_iter()
                        .filter_map(|name| original.bone(name).map(|bone| (name, bone)))
                        .collect();
                    if bones.is_empty() {
                        continue;
                    }
                    // Check every native emitter tick against the original
                    // full-pose evaluator, without publishing sampled paths.
                    for tick in 0..=binding.duration_ticks() {
                        let frame = (tick as f32 * resonance_content::animation::FRAME_HZ
                            / resonance_content::ANIMATION_HZ)
                            .min(motion.duration_frames);
                        let pose = original.sample(&motion, frame)?;
                        for &(name, bone) in &bones {
                            let actual = skeleton.sample_point(&published, frame, bone, [0.; 3])?;
                            let expected = pose.point(bone, [0.; 3])?;
                            let world = |p: [f32; 3]| {
                                std::array::from_fn::<_, 3, _>(|i| {
                                    (p[i] + part.translation[i]).trunc()
                                })
                            };
                            assert_eq!(
                                world(actual),
                                world(expected),
                                "part {index} clip {} bone {name} tick {tick}",
                                binding.resource_slot
                            );
                        }
                    }
                }
            }
            let expected =
                resonance_asset_writer::gltf::pack_glb(&expected.json, &expected.binary)?;
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
