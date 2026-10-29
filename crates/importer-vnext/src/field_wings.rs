//! Shared wing effects used by the Tower of Salvation field event.
use crate::{dol, field_core, field_npc_models, world_gpl, world_motion, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    field::document::{Model, ModelTarget},
};
use std::{collections::BTreeMap, fs, path::Path};

const WINGS: [(u32, &str); 3] = [
    (131434, "pink-sparkle-wings-2"),
    (131435, "purple-wings-1"),
    (131436, "purple-wings-2"),
];

pub fn field_wing_mappings() -> BTreeMap<u32, ModelTarget> {
    WINGS
        .into_iter()
        .map(|(id, name)| (id, ModelTarget::Model(format!("effects/{name}"))))
        .collect()
}

fn convert(library: &Path, id: u32, name: &str, package: &[u8]) -> Result<()> {
    let body = field_core::optional_section(package, 0)?.context("wing model has no body")?;
    ensure!(dol::u32(body, 0x20)? == 0x005b_bc61, "invalid wing model");
    let palette = body
        .get(dol::u32(body, 0)? as usize..dol::u32(body, 4)? as usize)
        .context("wing palette exceeds model")?;
    ensure!(dol::u32(palette, 0)? == 0x0020_af30, "wing model lacks TPL");
    let base = format!("meshes/effects/{name}");
    let mut textures = Vec::new();
    let mut alpha = Vec::new();
    for index in 0..dol::u32(palette, 4)? as usize {
        let (bytes, transparent) = world_texture::tpl_texture(palette, index)?;
        let path = format!("{base}/textures/body-texture-{index}.ktx2");
        crate::output::write(&library.join(&path), &bytes)?;
        textures.push(path);
        alpha.push(transparent);
    }
    let (mesh, mut part) = world_gpl::static_model(body, textures, alpha)
        .with_context(|| format!("decode wing model {id}"))?;
    part.mesh = format!("{base}/meshes/body.glb");
    crate::output::write(&library.join(&part.mesh), &mesh)?;
    let bindings = world_gpl::animation_bindings(body)?;
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    for index in 2..31 {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        let slot = 4 + index as u16 * 4;
        let motion = if let Some(existing) = unique.get(native) {
            existing.clone()
        } else {
            let path = format!(
                "{base}/animations/{}.glb",
                if slot == 12 {
                    "idle".to_owned()
                } else {
                    format!("motion-{slot}")
                }
            );
            let glb = world_motion::encode_bound(native, &mesh, &bindings)
                .with_context(|| format!("decode wing {id} motion slot {slot}"))?;
            crate::output::write(&library.join(&path), &glb)?;
            unique.insert(native.to_vec(), path.clone());
            path
        };
        part.clips.push(SceneClip {
            motion,
            joints: BTreeMap::new(),
            resource_slot: slot,
            duration_seconds: 0.,
            animation_resource: None,
            pose_chains: Vec::new(),
        });
    }
    let document = Model {
        parts: vec![part],
        collision: None,
        hidden_geometry: Vec::new(),
    };
    crate::output::json(&library.join(format!("{base}/model.json")), &document)
}

/// Convert the three shared wing models requested by field 0184.
pub fn cook_field_wings(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = field_npc_models::native_bank(extracted, &exe)?;
    for (id, name) in WINGS {
        convert(library, id, name, field_npc_models::member(&bank, id)?)?;
    }
    Ok(())
}
