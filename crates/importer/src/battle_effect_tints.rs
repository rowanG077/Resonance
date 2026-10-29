//! Original battle effect colors directly into one editable document.
use crate::rel::Rel;
use anyhow::{Result, ensure};
use resonance_content::battle_effect::{ActorColors, AdmissionColors, Tints};
use std::{fs, path::Path};

fn colors<const N: usize>(module: &Rel, section: usize, offset: usize) -> Result<[[u8; 4]; N]> {
    let bytes = module.slice(section, offset, N * 4)?;
    let mut colors = [[0; 4]; N];
    for (index, color) in colors.iter_mut().enumerate() {
        color.copy_from_slice(&bytes[index * 4..index * 4 + 4]);
    }
    Ok(colors)
}

fn convert(root: &Path) -> Result<Tints> {
    let module = Rel::read(fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
    let actors = colors::<12>(&module, 4, 0x1564)?;
    let admission = colors::<4>(&module, 4, 0x11b0)?;
    Ok(Tints {
        palettes: module.slice(4, 0x2174, 10)?.try_into()?,
        colors: colors::<10>(&module, 4, 0x2180)?,
        actors: ActorColors {
            recovery: actors[0],
            buff: actors[2],
            scan: actors[7],
            debuff: actors[8],
        },
        contact_effects: module.slice(5, 0x13f8, 10)?.try_into()?,
        contact_colors: colors::<10>(&module, 5, 0x13d0)?,
        admission_colors: AdmissionColors {
            basic: admission[0],
            advanced: admission[1],
            arcane: admission[2],
        },
    })
}

pub fn cook_battle_effect_tints(root: &Path, library: &Path) -> Result<()> {
    let document = convert(root)?;
    let output = library.join("battle/effects/tints.json");
    ensure!(!output.exists(), "battle effect tints already exist");
    crate::output::json(&output, &document)?;
    Ok(())
}
