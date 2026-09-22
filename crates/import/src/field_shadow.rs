//! Contact shadow defaults, bound to the shared effect atlas during preparation.
use crate::{dol, field_effects::Atlas};
use anyhow::{Result, ensure};
use resonance_content::field::ContactShadow;

pub(crate) fn read(executable: &[u8]) -> Result<ContactShadow<Atlas>> {
    let value = |address| -> Result<f32> {
        Ok(f32::from_be_bytes(
            dol::slice(executable, address, 4)?.try_into()?,
        ))
    };
    let immediate = |address, opcode| -> Result<u16> {
        let word = u32::from_be_bytes(dol::slice(executable, address, 4)?.try_into()?);
        ensure!(word >> 16 == opcode, "unexpected shadow initializer");
        Ok(word as u16)
    };
    let recipe = ContactShadow {
        texture: Atlas::Effect(2),
        uv_size: [f32::from(immediate(0x800183F0, 0x3880)?) / 256.; 2],
        // The actor supplies a diameter; the quad renderer truncates half-extents.
        half_size: (value(0x8035B080)? * value(0x8035B18C)? * value(0x8035AFFC)?).trunc(),
        height_offset: value(0x8035B080)?,
        alpha: immediate(0x80024C00, 0x3880)?.try_into()?,
        anchor_node: immediate(0x80024BEC, 0x38A0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted executables; no cooking or devices"]
    fn original_shadow_defaults_preserve_geometry_and_shared_atlas() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let executable = std::fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let recipe = read(&executable)?;
            let recipe: ContactShadow<Atlas> =
                serde_json::from_slice(&serde_json::to_vec(&recipe)?)?;
            assert!(matches!(recipe.texture, Atlas::Effect(2)));
            assert_eq!(recipe.uv_size, [0.25; 2]);
            assert_eq!(recipe.half_size, 42.);
            assert_eq!(recipe.height_offset, 2.);
            assert_eq!(recipe.alpha, 64);
            assert_eq!(recipe.anchor_node, 1);
        }
        Ok(())
    }
}
