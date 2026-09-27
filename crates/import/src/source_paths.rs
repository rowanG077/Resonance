//! Disc inventory keys use forward slashes on every host, just like declarations.
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn relative_source_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)?
        .to_str()
        .context("non-UTF8 disc path")?
        .replace('\\', "/");
    resonance_content::validate_asset_path(&relative)?;
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_inventory_paths_match_disc_declarations_on_every_host() -> Result<()> {
        let root = Path::new("extracted");
        for expected in [
            "FIELD/000.dat",
            "S/bgm_damy_start.song",
            "sys/main.dol",
            "sys/apploader.img",
            "sys/boot.bin",
            "sys/bi2.bin",
            "sys/fst.bin",
            "US_r_Top2field.rel",
        ] {
            let native = expected
                .split('/')
                .fold(root.to_path_buf(), |p, c| p.join(c));
            for path in [native, root.join(expected.replace('/', "\\"))] {
                assert_eq!(relative_source_path(root, &path)?, expected);
            }
        }
        assert!(relative_source_path(root, &root.join("../outside")).is_err());
        assert!(relative_source_path(root, Path::new("elsewhere/file")).is_err());
        Ok(())
    }
}
