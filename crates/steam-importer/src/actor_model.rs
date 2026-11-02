//! One actor package writes its ordinary model, clips, textures and collision.
use crate::{animation::Animation, field_core, model, output, package::Package, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip, ScenePart,
    animation::Skeleton,
    field::{ModelCollision, collision_mesh, document::Model},
};
use std::{fs, path::Path};

pub(crate) fn cook(
    source: &Source,
    library: &Path,
    name: &str,
    owner: &str,
) -> Result<(Model, model::Model)> {
    let bytes = source.require(name)?;
    let package = (!name.to_ascii_lowercase().ends_with(".tomdlb_d"))
        .then(|| Package::parse(&bytes))
        .transpose()?;
    let descriptor = match &package {
        Some(package) => package.reference(0)?.context("actor package has no body")?,
        None => name,
    };
    let (mut body, native) = model::cook(source, library, descriptor, owner, "body")?;
    let mut parts = Vec::new();
    if let Some(package) = &package {
        cook_clips(source, library, owner, "body", &mut body, &native, package)?;
    }
    let mut outline = if let Some(descriptor) = package
        .as_ref()
        .map(|p| p.reference(1))
        .transpose()?
        .flatten()
    {
        let (mut part, native) = model::cook(source, library, descriptor, owner, "outline")?;
        if let Some(package) = &package {
            cook_clips(
                source, library, owner, "outline", &mut part, &native, package,
            )?;
        }
        part
    } else {
        let path = format!("{owner}/meshes/outline.glb");
        output::write(&library.join(&path), &native.outline_glb()?)?;
        let mut part = body.clone();
        part.mesh = path;
        part
    };
    outline.resource = 1;
    outline.appearance = None;
    outline.outline_color = Some([0, 0, 0, 127]);
    for material in &mut outline.materials {
        material.color = None;
        material.multiply = None;
        material.blend = true;
        material.vertex_color = false;
        material.cull = CullFace::Front;
        material.draw_order += 65536;
    }
    parts.extend([body, outline]);
    let mut collision = ModelCollision::default();
    if let Some(package) = &package {
        for slot in [29, 30].into_iter().filter(|slot| *slot < package.len()) {
            let Some(name) = package.reference(slot)? else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".bin") {
                continue;
            }
            let groups = field_core::collision_groups(&source.require(name)?, true)?;
            if slot == 29 {
                collision.floors = groups;
            } else {
                collision.solids = groups;
            }
        }
    }
    let collision = if collision
        .floors
        .iter()
        .chain(&collision.solids)
        .any(|g| !g.triangles.is_empty())
    {
        let path = format!("{owner}/collision.glb");
        output::write(
            &library.join(&path),
            &collision_mesh::encode_model(&collision)?,
        )?;
        Some(path)
    } else {
        None
    };
    let hidden_geometry = native
        .nodes
        .iter()
        .zip(native.joint_names())
        .filter_map(|(node, name)| {
            (!node.visible || name.to_ascii_lowercase().starts_with("kk")).then_some(name)
        })
        .collect();
    let document = Model {
        parts,
        collision,
        hidden_geometry,
    };
    document.validate()?;
    Ok((document, native))
}

fn cook_clips(
    source: &Source,
    library: &Path,
    owner: &str,
    label: &str,
    part: &mut ScenePart,
    native: &model::Model,
    package: &Package<'_>,
) -> Result<()> {
    let skeleton = Skeleton::from_glb(&fs::read(library.join(&part.mesh))?)?;
    for slot in 2..package.len() {
        let Some(name) = package.reference(slot)? else {
            continue;
        };
        if name.to_ascii_lowercase().ends_with(".bin") {
            // Actor collision occupies the last two slots; a few older packages
            // also retain legacy motion streams, handled by their own decoder.
            ensure!(
                slot == 29 || slot == 30,
                "legacy actor motion {name} is not converted yet"
            );
            continue;
        }
        ensure!(
            name.to_ascii_lowercase().ends_with(".toanmb"),
            "unknown actor member {name}"
        );
        let animation = Animation::read(&source.require(name)?)?;
        let path = format!("{owner}/animations/{label}-motion-{}.glb", 4 + slot * 4);
        output::write(
            &library.join(&path),
            &animation.bind(native, &skeleton)?.encode(&skeleton)?,
        )?;
        part.clips.push(SceneClip {
            motion: path,
            resource_slot: (4 + slot * 4).try_into()?,
            joints: Default::default(),
            duration_seconds: 0.,
            animation_resource: None,
            pose_chains: Vec::new(),
        });
    }
    Ok(())
}
