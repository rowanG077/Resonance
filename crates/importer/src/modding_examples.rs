//! Prepare an ordinary additive arte mod against the current cooked library.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Channel, Interpolation, Motion, NodeTrack, Skeleton, Track, Transform},
    arte, characters,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    /// A fixed catalogue number. Leave it out: the game then gives the arte a
    /// free number when it starts, and mods never compete for one.
    #[serde(default)]
    id: Option<u16>,
    name: String,
    character: String,
    initially_known: bool,
    technique: arte::document::Entry,
    motion_reference: MotionReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionReference {
    skeleton: String,
    motion: String,
    frame: f32,
}

/// Admit only a new ID and name; a mod would otherwise silently replace an arte.
fn check_addition(document: &arte::document::Document, example: &Example) -> Result<()> {
    document.characters.id(&example.character)?;
    if let Some(fixed) = example.id {
        ensure!(
            document
                .mappings
                .iter()
                .all(|(id, _)| id != u32::from(fixed)),
            "modding example catalogue ID {fixed} is already used"
        );
    }
    ensure!(
        document
            .mappings
            .iter()
            .all(|(_, name)| name != example.name)
            && !document.techniques.contains_key(&example.name),
        "modding example {} already exists in the imported catalogue",
        example.name
    );
    Ok(())
}

/// Resolve reference-relative authored motion and publish only this mod's entries.
/// This needs the cooked library, never original discs or a fresh game import.
pub fn prepare_arte_mod(source: &Path, library: &Path, output: &Path) -> Result<()> {
    let source = std::fs::canonicalize(source)?;
    let base_root = std::fs::canonicalize(library)?;
    let example: Example = serde_json::from_slice(&std::fs::read(source.join("example.json"))?)?;
    let library = resonance_content::library::Library::open_paths(library, &[])?;
    let mut snapshot = library.snapshot();
    let techniques: arte::document::Document =
        snapshot.json(arte::PATH, resonance_content::preparation::JSON_LIMIT)?;
    check_addition(&techniques, &example)?;
    let original_learning = techniques
        .learning
        .get(&example.character)
        .context("modding example character has no learning list")?
        .len();
    let mut contribution = serde_json::json!({
        "techniques": {(example.name.clone()): example.technique},
        "learning": {(example.character.clone()): [example.name]}
    });
    if let Some(fixed) = example.id {
        contribution["mappings"] = serde_json::json!({(fixed.to_string()): example.name});
    }
    let mut files = contribution_files(
        arte::PATH,
        &contribution,
        original_learning as i64 * 1024 + 1024,
    )?;
    if example.initially_known {
        let characters: characters::Document =
            snapshot.json(characters::PATH, resonance_content::preparation::JSON_LIMIT)?;
        let known = characters
            .characters
            .get(&example.character)
            .and_then(|entry| entry.initial.as_ref())
            .context("modding example character has no initial state")?
            .techniques
            .len();
        let contribution = serde_json::json!({"characters":{(example.character.clone()):{"initial":{"techniques":[example.name]}}}});
        files.extend(contribution_files(
            characters::PATH,
            &contribution,
            known as i64 * 1024 + 1024,
        )?);
    }
    let action = example
        .technique
        .rules
        .action
        .as_deref()
        .context("arte mod needs an authored action")?;
    resonance_content::library::validate_path(action)?;
    let (scope, _) = action
        .rsplit_once('/')
        .context("arte assets need their own directory")?;
    // A directory alias is not a file and fails to read; a file alias must stay inside.
    for path in crate::authoring::file_paths(&source.join(scope))? {
        let logical = format!("{scope}/{path}");
        let physical = std::fs::canonicalize(source.join(&logical))?;
        ensure!(
            physical.starts_with(&source),
            "arte source escapes its project"
        );
        files.insert(logical, std::fs::read(physical)?);
    }
    let reference = &example.motion_reference;
    let skeleton = Skeleton::from_glb(&snapshot.read(
        &reference.skeleton,
        resonance_content::preparation::ASSET_LIMIT,
    )?)?;
    let pose = Motion::decode_for_skeleton(
        &snapshot.read(
            &reference.motion,
            resonance_content::preparation::ASSET_LIMIT,
        )?,
        &skeleton,
    )?;
    let action: serde_json::Value = serde_json::from_slice(&std::fs::read(source.join(action))?)?;
    for path in action["motions"]
        .as_object()
        .context("arte action needs motions")?
        .values()
    {
        let path = path.as_str().context("motion must be a GLB path")?;
        resonance_content::library::validate_path(path)?;
        let physical = std::fs::canonicalize(source.join(path))?;
        ensure!(
            physical.starts_with(&source),
            "arte motion escapes its project"
        );
        let motion = resolve_motion(&std::fs::read(physical)?, &skeleton, &pose, reference.frame)?;
        files.insert(path.to_owned(), motion);
    }
    let output = std::path::absolute(output)?;
    let parent = output
        .parent()
        .context("generated mod needs a parent directory")?;
    std::fs::create_dir_all(parent)?;
    let output = if output.exists() {
        std::fs::canonicalize(output)?
    } else {
        std::fs::canonicalize(parent)?.join(
            output
                .file_name()
                .context("generated mod needs a directory name")?,
        )
    };
    ensure!(
        !output.starts_with(&source)
            && !source.starts_with(&output)
            && !output.starts_with(&base_root)
            && !base_root.starts_with(&output),
        "generated mod output must be separate from sources and the base library"
    );
    const MARKER: &str = ".resonance-arte-mod";
    const GENERATED: &[u8] = b"resonance generated arte mod v1\n";
    if output.exists() {
        ensure!(output.is_dir(), "generated mod output must be a directory");
        ensure!(
            std::fs::read_dir(&output)?.next().is_none()
                || std::fs::read(output.join(MARKER)).is_ok_and(|bytes| bytes == GENERATED),
            "output is not a generated arte mod; choose a new output directory"
        );
    }
    let staged = tempfile::Builder::new()
        .prefix(".resonance-arte-")
        .tempdir_in(output.parent().unwrap())?;
    for (path, bytes) in files {
        crate::output::write(&staged.path().join(path), &bytes)?;
    }
    crate::authoring::publish(staged.path())?;
    let validated =
        resonance_content::library::Library::open_paths(&base_root, &[staged.path().to_owned()])?;
    validated
        .snapshot()
        .json::<arte::document::Document>(arte::PATH, resonance_content::preparation::JSON_LIMIT)?
        .prepare()?;
    crate::output::write(&staged.path().join(MARKER), GENERATED)?;
    // Only a previously generated directory is replaceable. All compilation and
    // admission has succeeded before retiring that reproducible output.
    if output.exists() {
        std::fs::remove_dir_all(&output)?;
    }
    std::fs::rename(staged.path(), output)?;
    Ok(())
}

fn contribution_files(
    document: &str,
    value: &serde_json::Value,
    order: i64,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for (path, mut value) in resonance_content::authoring::split(document, value)?
        .context("missing arte ownership schema")?
    {
        // The base owns its collections' markers; two additive mods must not collide on them.
        if path.ends_with("/collection.json") {
            continue;
        }
        if path.ends_with("/order.json") {
            value = order.into();
        }
        files.insert(path, resonance_asset_writer::json::to_vec(&value)?);
    }
    Ok(files)
}

/// The distributable clip contains component deltas on flat, identity nodes.
/// Restore the locally cooked reference pose before writing a normal runtime GLB.
fn resolve_motion(
    bytes: &[u8],
    skeleton: &Skeleton,
    reference: &Motion,
    frame: f32,
) -> Result<Vec<u8>> {
    let reference = skeleton.sample(reference, frame)?.local;
    let relative = Motion::decode_for_skeleton(bytes, skeleton)?;
    let mut tracks: BTreeMap<_, _> = relative
        .tracks
        .into_iter()
        .map(|track| (track.bone, track.nodes))
        .collect();
    let tracks = reference
        .into_iter()
        .enumerate()
        .map(|(bone, reference)| {
            let mut nodes = tracks
                .remove(&(bone as u16))
                .unwrap_or_else(|| vec![NodeTrack::new(Transform::default()).into()]);
            ensure!(
                nodes.len() == 1 && nodes[0].bind == Transform::default(),
                "relative motion must use flat identity nodes"
            );
            let mut node = std::sync::Arc::unwrap_or_clone(nodes.remove(0));
            node.bind = reference;
            node.translation = Some(offset_channel(node.translation, reference.translation));
            let rotation = glam::Quat::from_array(reference.rotation).normalize();
            node.rotation = Some(reference_channel(
                node.rotation,
                [0., 0., 0., 1.],
                |delta, _| (rotation * glam::Quat::from_array(delta)).to_array(),
            ));
            // Additive scale also represents restoring a part hidden by zero scale.
            node.scale = Some(offset_channel(node.scale, reference.scale));
            Ok(Track {
                bone: bone as u16,
                nodes: vec![node.into()],
            })
        })
        .collect::<Result<_>>()?;
    Motion {
        duration_frames: relative.duration_frames,
        tracks,
    }
    .encode(skeleton)
}

fn offset_channel(channel: Option<Channel<3>>, reference: [f32; 3]) -> Channel<3> {
    reference_channel(channel, [0.; 3], |delta, tangent| {
        std::array::from_fn(|axis| delta[axis] + if tangent { 0. } else { reference[axis] })
    })
}

fn reference_channel<const N: usize>(
    channel: Option<Channel<N>>,
    identity: [f32; N],
    apply: impl Fn([f32; N], bool) -> [f32; N],
) -> Channel<N> {
    let mut channel = channel.unwrap_or(Channel {
        interpolation: Interpolation::Step,
        times: vec![0.],
        values: vec![identity],
    });
    for (index, value) in channel.values.iter_mut().enumerate() {
        let tangent = channel.interpolation == Interpolation::CubicSpline && index % 3 != 1;
        *value = apply(*value, tangent);
    }
    channel
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn relative_motion_resolves_a_local_pose_without_changing_interpolation() -> Result<()> {
        let mut skeleton = Skeleton::single("root");
        skeleton.bones[0].bind.translation = [2., 3., 4.];
        skeleton.bones[0].bind.scale = [2., 0., 4.];
        let mut child = Skeleton::single("unkeyed").bones.remove(0);
        child.parent = Some(0);
        child.bind.translation = [0., 5., 0.];
        skeleton.bones.push(child);
        let reference = Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![std::sync::Arc::new(NodeTrack {
                    rotation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![
                            glam::Quat::IDENTITY.to_array(),
                            glam::Quat::from_rotation_y(1.2).to_array(),
                        ],
                    }),
                    ..NodeTrack::new(skeleton.bones[0].bind)
                })],
            }],
        };
        let relative = Motion {
            duration_frames: 30.,
            tracks: vec![Track {
                bone: 0,
                nodes: vec![std::sync::Arc::new(NodeTrack {
                    bind: Transform::default(),
                    translation: Some(Channel {
                        interpolation: Interpolation::CubicSpline,
                        times: vec![0., 1.],
                        values: vec![[1.; 3], [0.; 3], [1.; 3], [1.; 3], [1.; 3], [1.; 3]],
                    }),
                    rotation: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![
                            glam::Quat::IDENTITY.to_array(),
                            glam::Quat::from_rotation_x(0.9).to_array(),
                        ],
                    }),
                    scale: Some(Channel {
                        interpolation: Interpolation::Linear,
                        times: vec![0., 1.],
                        values: vec![[0.; 3], [1.; 3]],
                    }),
                })],
            }],
        };
        let mut flat = skeleton.clone();
        for bone in &mut flat.bones {
            bone.parent = None;
            bone.bind = Transform::default();
        }
        let output = resolve_motion(&relative.encode_nodes(&flat)?, &skeleton, &reference, 15.)?;
        let output = Motion::decode_for_skeleton(&output, &skeleton)?;
        assert_eq!(output.duration_frames, 30.);
        for frame in [0., 7.5, 15., 30.] {
            let fraction = frame / 30.;
            let actual = skeleton.sample(&output, frame)?.local;
            let expected = Transform {
                translation: [2. + fraction, 3. + fraction, 4. + fraction],
                rotation: (glam::Quat::from_rotation_y(0.6)
                    * glam::Quat::from_rotation_x(0.9 * fraction))
                .to_array(),
                scale: [2. + fraction, fraction, 4. + fraction],
            };
            assert!(
                actual[0]
                    .matrix()
                    .iter()
                    .flatten()
                    .zip(expected.matrix().iter().flatten())
                    .all(|(actual, expected)| (actual - expected).abs() < 0.00001)
            );
            assert_eq!(actual[1], skeleton.bones[1].bind);
        }
        assert!(
            resolve_motion(&relative.encode_nodes(&flat)?, &skeleton, &reference, 31.).is_err()
        );
        Ok(())
    }

    fn native_techniques() -> arte::document::Document {
        let names = [
            "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
        ];
        let characters = names
            .iter()
            .enumerate()
            .map(|(index, name)| ((index + 1).to_string(), *name))
            .collect::<BTreeMap<_, _>>();
        let learning = names
            .iter()
            .map(|name| (*name, vec!["native-arte"]))
            .collect::<BTreeMap<_, _>>();
        serde_json::from_value(json!({
            "mappings": {"1": "native-arte"}, "characters": characters,
            "techniques": {"native-arte": {"rules": {"tp_cost": 7}}},
            "learning": learning
        }))
        .unwrap()
    }

    #[test]
    fn prepared_mods_ship_no_shared_markers_and_guard_their_output_directory() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let [source, base, output] =
            ["source", "base", "mods/wolf"].map(|name| directory.path().join(name));
        crate::output::json(
            &base.join("content.json"),
            &resonance_content::library::Descriptor {
                version: resonance_content::library::VERSION,
                game: resonance_content::library::GAME.into(),
            },
        )?;
        let techniques = serde_json::to_value(native_techniques())?;
        resonance_content::authoring::publish(&base, arte::PATH, &techniques)?;
        let skeleton = Skeleton::single("root");
        let still = Motion {
            duration_frames: 1.,
            tracks: Vec::new(),
        };
        let rig = json!({"asset": {"version": "2.0"}, "nodes": [{"name": "root", "extras": {"resonance_bone": 0}}]});
        crate::output::write(
            &base.join("rig.glb"),
            &resonance_asset_writer::gltf::pack_glb(&rig, &[])?,
        )?;
        crate::output::write(&base.join("idle.glb"), &still.encode(&skeleton)?)?;
        let mut recipe: serde_json::Value =
            serde_json::from_str(include_str!("../../../mods/ghost-wolf/example.json"))?;
        recipe["motion_reference"] =
            json!({"skeleton": "rig.glb", "motion": "idle.glb", "frame": 0.});
        crate::output::json(&source.join("example.json"), &recipe)?;
        let scope = source.join("battle/artes/ghost-wolf");
        crate::output::json(
            &scope.join("action.json"),
            &json!({"motions": {"110": "battle/artes/ghost-wolf/ghost-wolf.glb"}}),
        )?;
        crate::output::write(
            &scope.join("ghost-wolf.glb"),
            &still.encode_nodes(&skeleton)?,
        )?;

        prepare_arte_mod(&source, &base, &output)?;
        let files = crate::authoring::file_paths(&output)?;
        // The arte is identified by name; the game numbers it at startup.
        assert!(files.contains("game/techniques/techniques/ghost-wolf-lloyd/rules.json"));
        assert!(!files.iter().any(|path| path.contains("/mappings/")));
        assert!(!files.iter().any(|path| path.ends_with("collection.json")));
        let allocated: arte::document::Document =
            resonance_content::library::Library::open_paths(&base, std::slice::from_ref(&output))?
                .snapshot()
                .json(arte::PATH, resonance_content::preparation::JSON_LIMIT)?;
        assert_eq!(allocated.mappings.id("ghost-wolf-lloyd")?, 2);
        assert_eq!(allocated.allocated_from, Some(2));

        // Only a previously generated directory is replaced.
        crate::output::write(&output.join("stale"), b"")?;
        prepare_arte_mod(&source, &base, &output)?;
        assert!(!output.join("stale").exists());
        let foreign = directory.path().join("foreign");
        crate::output::write(&foreign.join("notes.txt"), b"")?;
        for refused in [foreign, source.join("output"), base.join("output")] {
            assert!(prepare_arte_mod(&source, &base, &refused).is_err());
        }
        assert!(base.join("content.json").is_file() && scope.join("action.json").is_file());
        Ok(())
    }

    #[test]
    fn rejects_collisions_with_imported_techniques() -> Result<()> {
        let recipe: serde_json::Value =
            serde_json::from_str(include_str!("../../../mods/ghost-wolf/example.json"))?;
        let mut fixed = recipe.clone();
        fixed["id"] = json!(260);
        for (mappings, recipe, collides) in [
            // A name is taken whatever its number; a number only when requested.
            (
                json!({"1": "native-arte", "261": "ghost-wolf-lloyd"}),
                &recipe,
                true,
            ),
            (
                json!({"1": "native-arte", "260": "future-native-arte"}),
                &recipe,
                false,
            ),
            (
                json!({"1": "native-arte", "260": "future-native-arte"}),
                &fixed,
                true,
            ),
            (json!({"1": "native-arte"}), &fixed, false),
        ] {
            let mut techniques = serde_json::to_value(native_techniques())?;
            techniques["mappings"] = mappings.clone();
            let techniques: arte::document::Document = serde_json::from_value(techniques)?;
            let example = serde_json::from_value(recipe.clone())?;
            assert_eq!(
                check_addition(&techniques, &example).is_err(),
                collides,
                "{mappings}"
            );
        }
        Ok(())
    }
}
