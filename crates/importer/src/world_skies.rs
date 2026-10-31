//! Two embedded native sky models directly into editable model files.
use crate::{compression, dol, rel::Rel, world_gpl, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::document::Model;
use std::{fs, path::Path};

pub fn cook_world_skies(root: &Path, library: &Path) -> Result<()> {
    let rel = Rel::read(fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    for (name, pointer) in [("normal", 0x2a56c), ("late", 0x2a570)] {
        let (section, offset) = rel.pointer(6, pointer)?;
        let resource = compression::cabinet(rel.tail(section, offset)?)?;
        let start = dol::u32(&resource, 0)? as usize;
        let end = dol::u32(&resource, 4)? as usize;
        let tpl = resource
            .get(start..end)
            .context("sky texture catalogue exceeds model")?;
        ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing sky TPL");
        let owner = format!("world/skies/{name}");
        let mut files = Vec::<(String, Vec<u8>)>::new();
        let mut paths = Vec::new();
        let mut alpha = Vec::new();
        for index in 0..dol::u32(tpl, 4)? as usize {
            let path = format!("{owner}/textures/part-0-texture-{index}.ktx2");
            let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
            files.push((path.clone(), texture));
            paths.push(path);
            alpha.push(transparent);
        }
        let (glb, mut part) = world_gpl::sky(&resource, paths, alpha)?;
        let mesh = format!("{owner}/meshes/part-0.glb");
        files.push((mesh.clone(), glb));
        part.mesh = mesh;
        files.push((
            format!("{owner}/model.json"),
            resonance_asset_writer::json::encode(&Model { parts: vec![part] })?,
        ));
        for (path, bytes) in files {
            let output = library.join(path);
            crate::output::write(&output, &bytes)?;
        }
    }
    Ok(())
}
