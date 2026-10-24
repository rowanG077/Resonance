//! Disc inventory keys use forward slashes on every host, just like declarations.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

/// Resolve one native resource across all supplied inputs. Identical copies
/// share an owner; conflicting definitions need an importer decision, never a
/// source-order preference or another runtime namespace.
pub(crate) fn shared_source<'a>(
    roots: impl IntoIterator<Item = &'a Path>,
    source: &str,
) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    for root in roots {
        let files = root.join("files");
        if let Some(path) = crate::field_resources::find_path(&files, source)? {
            paths.push(files.join(path));
        }
    }
    let Some(first) = paths.first() else {
        return Ok(None);
    };
    if paths.len() > 1 {
        let expected = crate::media::hash_file(first)?;
        for path in &paths[1..] {
            ensure!(
                crate::media::hash_file(path)? == expected,
                "conflicting definitions of source {source}: {} and {}",
                first.display(),
                path.display()
            );
        }
    }
    Ok(paths.into_iter().next())
}

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
    fn repeated_inputs_share_sources_and_conflicts_are_order_independent() -> Result<()> {
        let first = tempfile::tempdir()?;
        let second = tempfile::tempdir()?;
        for root in [first.path(), second.path()] {
            std::fs::create_dir(root.join("files"))?;
            std::fs::write(root.join("files/Resource.bin"), b"one logical resource")?;
        }
        for roots in [[first.path(), second.path()], [second.path(), first.path()]] {
            let path = shared_source(roots, "resource.bin")?.unwrap();
            assert_eq!(std::fs::read(path)?, b"one logical resource");
            assert!(shared_source(roots, "missing.bin")?.is_none());
        }
        std::fs::write(
            second.path().join("files/Resource.bin"),
            b"conflicting content",
        )?;
        for roots in [[first.path(), second.path()], [second.path(), first.path()]] {
            let error = shared_source(roots, "resource.bin").unwrap_err();
            assert!(error.to_string().contains("conflicting definitions"));
        }
        Ok(())
    }

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
