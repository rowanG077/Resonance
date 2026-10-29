//! Battle controls reference an ordinary named font and owned artwork.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::Context;
use std::sync::Arc;

pub const PATH: &str = "ui/battle/battle.json";
pub const FONT: &str = "fonts/battle.json";

pub fn read(
    read: &mut impl FnMut(&str) -> Result<Arc<[u8]>>,
    diagnostics: &Diagnostics,
) -> Result<Art> {
    let bytes = read(PATH)?;
    ensure!(bytes.len() <= JSON_LIMIT, "battle UI exceeds size limit");
    let definition =
        Art::<String>::decode_fields(&bytes, diagnostics).with_context(|| PATH.to_owned())?;
    crate::library::validate_path(&definition.font)?;
    let bytes = read(&definition.font)?;
    ensure!(bytes.len() <= JSON_LIMIT, "battle font exceeds size limit");
    let font: BitmapFont =
        serde_json::from_slice(&bytes).with_context(|| definition.font.clone())?;
    let mut art = definition.with_font(font);
    for image in art.required_images_mut() {
        crate::ui_art::prepare_image(image, &mut *read)?;
    }
    for portrait in &mut art.portraits {
        if let Some(image) = portrait
            && diagnostics
                .attempt(
                    "battle portrait image",
                    (|| {
                        crate::ui_art::prepare_image(image, &mut *read)?;
                        validate_portrait(image)
                    })(),
                )?
                .is_none()
        {
            *portrait = None;
        }
    }
    art.validate_layout().with_context(|| PATH.to_owned())?;
    Ok(art)
}

pub fn capture(
    snapshot: &mut Snapshot,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Art> {
    let result = read(
        &mut |path| {
            ensure!(!cancelled(), "battle UI preparation cancelled");
            snapshot.read(path, ASSET_LIMIT)
        },
        diagnostics,
    );
    // Optional portraits must never swallow cancellation.
    ensure!(!cancelled(), "battle UI preparation cancelled");
    let art = result?;
    snapshot.read(&art.font.texture, ASSET_LIMIT)?;
    ensure!(!cancelled(), "battle UI preparation cancelled");
    Ok(art)
}
