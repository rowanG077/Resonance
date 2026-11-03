//! Fixed GC atlas cells used by equipment trails and wing sparkles, outside programs.
use super::*;
use resonance_content::battle_effect::EffectTexture;

pub(super) fn append(
    source: &Source,
    library: &Path,
    resource: &str,
    table: Option<&effect_atlas::Table>,
    owner: &str,
    texture: &mut EffectTexture,
) -> Result<()> {
    let pixels = crate::ui_images::read(source, resource)?;
    ensure!(
        pixels.width() >= 512 && pixels.height() >= 512,
        "resident fixed atlas is smaller than GC UV space"
    );
    let trail = image::imageops::crop_imm(&pixels, 0, 0, 512, 512).to_image();
    let page = texture.images.len();
    texture.images.push(crate::ui_images::write(
        library,
        &format!("{owner}/textures/weapon-trails.ktx2"),
        &trail,
    )?);
    let default = texture
        .palette_sets
        .first_mut()
        .context("missing fixed atlas palette set")?;
    ensure!(
        default[0].is_none() && default[1].is_none(),
        "fixed atlas overlaps a particle palette set"
    );
    default[0] = Some(page);
    default[1] = Some(page);
    for selector in 27..=31u8 {
        let mut quad = [[[193., 385.], [255., 385.], [193., 447.], [255., 447.]]];
        if let Some(table) = table {
            table.remap(u16::from(selector), &mut quad);
        }
        let mapped = quad[0];
        ensure!(
            mapped
                .iter()
                .flatten()
                .all(|value| value.is_finite() && value.fract() == 0.)
                && mapped[3][0] - mapped[0][0] == 62.
                && mapped[3][1] - mapped[0][1] == 62.,
            "wing sparkle remapping changes texel scale"
        );
        let mut image = image::RgbaImage::new(512, 512);
        for y in -1..=62i32 {
            for x in -1..=62i32 {
                let sx = (mapped[0][0] as i32 + x).clamp(0, pixels.width() as i32 - 1) as u32;
                let sy = (mapped[0][1] as i32 + y).clamp(0, pixels.height() as i32 - 1) as u32;
                image.put_pixel(
                    (193 + x) as u32,
                    (385 + y) as u32,
                    *pixels.get_pixel(sx, sy),
                );
            }
        }
        let page = texture.images.len();
        texture.images.push(crate::ui_images::write(
            library,
            &format!("{owner}/textures/wing-sparkle-{selector}.ktx2"),
            &image,
        )?);
        texture.palette_sets[0][usize::from(selector)] = Some(page);
    }
    Ok(())
}
