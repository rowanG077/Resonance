//! Four native world dust sprites directly into editable KTX2 files.
use crate::{dol, rel::Rel, world_texture::tpl_texture};
use anyhow::{Result, ensure};
use std::{fs, path::Path};

pub fn cook_world_dust(root: &Path, library: &Path) -> Result<()> {
    let rel = Rel::read(fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    let tpl = rel.tail(6, 0x2f100)?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing world dust TPL");
    ensure!(dol::u32(tpl, 4)? == 4, "unexpected world dust count");
    let outputs = (0..4)
        .map(|index| library.join(format!("world/effects/dust-{index}.ktx2")))
        .collect::<Vec<_>>();
    ensure!(
        outputs.iter().all(|path| !path.exists()),
        "world dust output already exists"
    );
    for (index, output) in outputs.iter().enumerate() {
        let (texture, _) = tpl_texture(tpl, index)?;
        crate::output::write(&output, &texture)?;
    }
    Ok(())
}
