//! Native figurine names directly into the editable Figure Collection catalogue.
use crate::{dol, text};
use anyhow::{Context, Result, ensure};
use resonance_content::figurine::{FIGURINE_COUNT, FIGURINE_VERSION};
use serde_json::json;
use std::{fs, path::Path};
use symphonia_constants::FigurineId;

const RECORDS: u32 = 0x8022_80c0;
const TITLE: u32 = 0x8019_d6f4;
const STRIDE: usize = 80;

pub fn cook_menu_figurines(extracted: &Path, library: &Path) -> Result<()> {
    let output = library.join("game/menu/figurines.json");
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let title = text::pointer(&exe, TITLE)?;
    let records = dol::slice(&exe, RECORDS, FIGURINE_COUNT * STRIDE)?
        .chunks_exact(STRIDE)
        .enumerate()
        .map(|(id, row)| {
            let name = text::string(&exe, dol::u32(row, 0)?)
                .with_context(|| format!("figurine {id} has no name"))?;
            let resource = dol::u32(row, 8)?;
            ensure!(resource < 0x8000_0000, "figurine {id} has no native model");
            let identity = FigurineId::try_from(u16::try_from(id)?)
                .with_context(|| format!("figurine {id} has no readable identity"))?;
            let path = identity
                .resource_name()
                .with_context(|| format!("figurine {id} has no readable path"))?;
            Ok(json!({
                "id": id,
                "name": name,
                "preview": format!("meshes/figurines/{path}/model.json"),
                "version": FIGURINE_VERSION,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let document = json!({"title": title, "records": records});
    crate::output::json(&output, &document)?;
    Ok(())
}
