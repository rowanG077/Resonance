//! Native world markers directly into named models, textures and loop clips.
use crate::{compression, dol, native_archive::member, world_gpl, world_motion, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    library::Mappings,
    overworld::document::{self, Model},
};
use std::{collections::BTreeMap, fs, path::Path};
use symphonia_constants::{SylvarantMarkerId, TetheAllaMarkerId};

fn texture_path(name: &str, index: usize, count: usize) -> String {
    let shared = match (name, index) {
        ("portal-sylvarant" | "portal-tethe-alla", 1) => Some("portal-glow"),
        ("luin-ruins" | "luin-rebuilding" | "luin-rebuilt", 1) | ("luin", 0) => Some("luin-cliffs"),
        ("ozette" | "ozette-destroyed", 0) => Some("ozette-ground"),
        _ => None,
    };
    if let Some(shared) = shared {
        format!("world/markers/{shared}.ktx2")
    } else if count == 1 {
        format!("world/markers/{name}/texture.ktx2")
    } else {
        format!("world/markers/{name}/texture-{}.ktx2", index + 1)
    }
}

fn insert(files: &mut BTreeMap<String, Vec<u8>>, path: String, bytes: Vec<u8>) -> Result<()> {
    if let Some(previous) = files.get(&path) {
        ensure!(*previous == bytes, "conflicting shared marker asset {path}");
    } else {
        files.insert(path, bytes);
    }
    Ok(())
}

pub fn cook_world_markers(root: &Path, library: &Path) -> Result<()> {
    let mut files = BTreeMap::new();
    let mut owners = BTreeMap::<String, Vec<u8>>::new();
    let mut mappings = BTreeMap::new();
    for (world, source) in ["s1.dat", "s2.dat"].into_iter().enumerate() {
        let compressed = fs::read(root.join("disc1/files/FIELD").join(source))?;
        let archive = compression::cabinet(&compressed)?;
        for id in 1..=17u8 {
            let name = if world == 0 {
                SylvarantMarkerId::try_from(id)
                    .ok()
                    .and_then(SylvarantMarkerId::resource_name)
            } else {
                TetheAllaMarkerId::try_from(id)
                    .ok()
                    .and_then(TetheAllaMarkerId::resource_name)
            };
            let Some(name) = name else { continue };
            mappings.insert(world as u32 * 256 + u32::from(id), name.to_owned());
            let package = member(&archive, usize::from(id - 1), 17)?
                .with_context(|| format!("missing marker {world}:{id}"))?;
            let resource = if id == 1 {
                member(package, 0, 19)?.context("missing portal model")?
            } else {
                package
            };
            if let Some(previous) = owners.insert(name.into(), resource.to_vec()) {
                ensure!(
                    previous == resource,
                    "shared marker {name} differs by world"
                );
                continue;
            }
            let start = dol::u32(resource, 0)? as usize;
            let end = dol::u32(resource, 4)? as usize;
            let tpl = resource
                .get(start..end)
                .context("marker TPL exceeds model")?;
            ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing marker TPL");
            let count = dol::u32(tpl, 4)? as usize;
            let mut paths = Vec::new();
            let mut alpha = Vec::new();
            for index in 0..count {
                let path = texture_path(name, index, count);
                let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
                insert(&mut files, path.clone(), texture)?;
                paths.push(path);
                alpha.push(transparent);
            }
            let (glb, mut part) = world_gpl::static_model(resource, paths, alpha)
                .with_context(|| format!("convert marker {name}"))?;
            let mesh = format!("world/markers/{name}/model.glb");
            if id == 1 {
                let motion = member(package, 2, 19)?.context("missing portal motion")?;
                let path = format!("world/markers/{name}/portal-loop.glb");
                insert(
                    &mut files,
                    path.clone(),
                    world_motion::encode(motion, &glb)?,
                )?;
                part.clips.push(SceneClip {
                    motion: path,
                    joints: BTreeMap::new(),
                    resource_slot: 0,
                    duration_seconds: 0.,
                    animation_resource: None,
                    pose_chains: Vec::new(),
                });
            }
            insert(&mut files, mesh.clone(), glb)?;
            part.mesh = mesh;
            insert(
                &mut files,
                format!("world/markers/{name}/model.json"),
                resonance_asset_writer::json::encode(&Model { parts: vec![part] })?,
            )?;
        }
    }
    insert(
        &mut files,
        document::MARKERS.into(),
        resonance_asset_writer::json::encode(&Mappings::new(mappings)?)?,
    )?;
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
