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
    let (mut body, mut native) = model::cook(source, library, descriptor, owner, "body")?;
    let sizes = body
        .textures
        .iter()
        .map(|path| resonance_asset_writer::texture::dimensions(&fs::read(library.join(path))?))
        .collect::<Result<Vec<_>>>()?;
    body.appearance = model::tagged_appearance(source, descriptor, &sizes)?;
    let mut parts = Vec::new();
    if let Some(package) = &package {
        cook_clips(source, library, owner, "body", &mut body, &native, package)?;
    }
    let effect_body = unoutlined_effect(name, package.as_ref())?;
    if effect_body {
        if let Some(shape) = standalone_effect_shape(name) {
            ensure!(
                native.meshes.len() == 1
                    && native.meshes[0].name == shape
                    && native.materials.records.len() == 1,
                "unexpected native standalone energy geometry: {name}"
            );
            // These shared particle meshes already use the GC RGB range.
            // In particular COLUMN1's baked grey is ~0.6, not neutral RGB128.
        } else {
            crate::battle_effects::normalize_model_color(
                source,
                library,
                descriptor,
                &body,
                &mut native,
            )?;
        }
    }
    let outline = if effect_body {
        None
    } else if let Some(descriptor) = package
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
        Some(part)
    } else {
        let path = format!("{owner}/meshes/outline.glb");
        output::write(&library.join(&path), &native.outline_glb()?)?;
        let mut part = body.clone();
        part.mesh = path;
        Some(part)
    };
    if let Some(mut outline) = outline {
        style_outline(&mut outline);
        style_body(&mut body);
        parts.extend([body, outline]);
    } else {
        parts.push(body);
    }
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
    let hidden_geometry = native.rig.hidden_geometry();
    let document = Model {
        parts,
        collision,
        hidden_geometry,
    };
    document.validate()?;
    Ok((document, native))
}

fn standalone_effect_shape(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "d_obj012.tomdlb_d" => Some("COLUMN1SHAPE"),
        "d_obj018.tomdlb_d" => Some("G_EFF05SHAPE"),
        _ => None,
    }
}

fn unoutlined_effect(name: &str, package: Option<&Package<'_>>) -> Result<bool> {
    // Audited energy effects have one textured pass in GC and no native Steam
    // shell. Inventing one also enables character toon lighting on their bodies.
    // NULL slot 1 alone is not an effect tag: ordinary NPCs need GC outlines too.
    let source_name = name.to_ascii_lowercase();
    if standalone_effect_shape(&source_name).is_some() {
        // Global68604/68610 are standalone energy descriptors with no authored
        // shell. Model particles tint every part; a fabricated constant-color
        // character outline therefore becomes a solid white cylinder or ring.
        ensure!(package.is_none(), "energy ring is a standalone descriptor");
        return Ok(true);
    }
    if !matches!(
        source_name.as_str(),
        // Triet guardian light / arrival Warp.
        "fir_d03_b000.pac" | "fir_d03_b001.pac"
        // Salvation's interior/minori and surrounding aura.
        | "elc_d16_b000.pac" | "elc_d16_b010.pac"
        // Colette's energy wings and the shared wing-effect family.
        | "npc_all_b355.pac" | "npc_all_b360.pac" | "npc_all_b361.pac"
        | "npc_all_b362.pac" | "npc_all_b363.pac" | "npc_all_b364.pac"
        | "npc_all_b372.pac"
    ) {
        return Ok(false);
    }
    let descriptor = format!(
        "{}_OBJ000.TOMDLB_D",
        source_name.strip_suffix(".pac").unwrap()
    );
    let package = package.context("unoutlined effect needs its native actor package")?;
    ensure!(
        package.len() == 31
            && package.reference(0)? == Some(descriptor.as_str())
            && package.reference(1)?.is_none(),
        "unexpected native unoutlined effect package: {name}"
    );
    Ok(true)
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
        if matches!(slot, 29 | 30) && name.to_ascii_lowercase().ends_with(".bin") {
            continue;
        }
        let bytes = source.require(name)?;
        let motion = match name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("bin") => crate::legacy_motion::convert(
                &bytes,
                &fs::read(library.join(&part.mesh))?,
                &native
                    .rig
                    .nodes
                    .iter()
                    .map(|n| n.name.clone())
                    .collect::<Vec<_>>(),
            )?,
            Some("toanmb") => Animation::read(&bytes)?
                .bind(&native.rig, &skeleton)?
                .encode(&skeleton)?,
            _ => anyhow::bail!("unknown actor member {name}"),
        };
        let path = format!("{owner}/animations/{label}-motion-{}.glb", 4 + slot * 4);
        output::write(&library.join(&path), &motion)?;
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

pub(crate) fn style_body(part: &mut ScenePart) {
    // The GC body pass writes depth even when feather or wing edges blend.
    // Otherwise the later backface outline shell covers the shaded surface.
    for material in &mut part.materials {
        material.depth_write = true;
    }
}

pub(crate) fn style_outline(part: &mut ScenePart) {
    part.resource = 1;
    part.appearance = None;
    part.outline_color = Some([0, 0, 0, 127]);
    for material in &mut part.materials {
        material.color = None;
        material.multiply = None;
        material.blend = true;
        material.vertex_color = false;
        material.cull = CullFace::Front;
        material.draw_order += 65536;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_outline_normalization_is_limited_to_verified_source_effects() -> Result<()> {
        // A missing shell is normal for some NPCs too; it is not an effect tag.
        assert!(!unoutlined_effect("npc.pac", None)?);
        assert!(!unoutlined_effect("fir_d03_b002.pac", None)?);
        assert!(!unoutlined_effect("d_OBJ017.TOMDLB_D", None)?);
        assert!(unoutlined_effect("d_OBJ012.TOMDLB_D", None)?);
        assert!(unoutlined_effect("d_OBJ018.TOMDLB_D", None)?);
        assert!(unoutlined_effect("fir_d03_b000.pac", None).is_err());
        let mut bytes = vec![0; 4 + 31 * 4];
        bytes[..4].copy_from_slice(&31u32.to_le_bytes());
        let package = Package::parse(&bytes)?;
        assert!(unoutlined_effect("fir_d03_b001.pac", Some(&package)).is_err());
        Ok(())
    }
}
