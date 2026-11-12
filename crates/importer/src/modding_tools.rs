//! Authoring helpers for mods: texture conversion in the library's own
//! profile, and finding what refers to a resource.
use anyhow::{Context, Result};
use resonance_asset_writer::{gpu, ktx2, texture};
use resonance_content::library::Library;
use std::collections::BTreeSet;

/// What a PNG became. BC7 stores whole 4×4 blocks, so any other size is kept
/// as lossless RGBA8 instead of being padded: padding would move every
/// texture coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureProfile {
    Bc7,
    Rgba8,
}

/// Convert artwork to a KTX2 the game accepts on every supported GPU.
pub fn png_to_ktx2(png: &[u8]) -> Result<(Vec<u8>, TextureProfile)> {
    let pixels = texture::png(png)?;
    if pixels.width.is_multiple_of(4) && pixels.height.is_multiple_of(4) {
        Ok((gpu::encode(&[pixels])?, TextureProfile::Bc7))
    } else {
        Ok((
            ktx2::encode_rgba8(pixels.width, pixels.height, &pixels.rgba)?,
            TextureProfile::Rgba8,
        ))
    }
}

/// Decode the largest level of a library texture for editing.
pub fn ktx2_to_png(bytes: &[u8]) -> Result<Vec<u8>> {
    texture::ktx2(bytes)?
        .first()
        .context("texture has no image levels")?
        .png()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Documents which name the path, as the game resolves them.
    pub references: BTreeSet<String>,
    /// Other files with exactly the same bytes: separate copies of the same
    /// artwork, each with its own references.
    pub copies: BTreeSet<String>,
}

/// Find what a replacement of `path` affects, and which identical copies it
/// would leave untouched.
pub fn usage(library: &Library, path: &str) -> Result<Usage> {
    resonance_content::library::validate_path(path)?;
    let mut snapshot = library.snapshot();
    let limit = resonance_content::preparation::ASSET_LIMIT;
    let target = snapshot
        .read_optional(path, limit)?
        .with_context(|| format!("{path} is not in the library"))?;
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    let needle = serde_json::to_string(path)?;
    let mut usage = Usage::default();
    for candidate in library.paths() {
        if candidate == path {
            continue;
        }
        if candidate.ends_with(".json") {
            let bytes = snapshot.read(candidate, limit)?;
            if bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
            {
                usage.references.insert(candidate.to_owned());
            }
        } else if candidate.rsplit_once('.').map(|(_, extension)| extension) == extension
            && library.size(candidate)? == target.len() as u64
            && *snapshot.read(candidate, limit)? == *target
        {
            usage.copies.insert(candidate.to_owned());
        }
    }
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_round_trip_in_the_profile_their_size_allows() -> Result<()> {
        for (width, height, expected) in
            [(8, 4, TextureProfile::Bc7), (5, 4, TextureProfile::Rgba8)]
        {
            // Flat colors survive BC7 exactly.
            let pixels = texture::Pixels {
                width,
                height,
                rgba: [32, 64, 128, 255].repeat((width * height) as usize),
            };
            let (encoded, profile) = png_to_ktx2(&pixels.png()?)?;
            assert_eq!(profile, expected);
            assert_eq!(
                gpu::is_native_compressed(&encoded)?,
                expected == TextureProfile::Bc7
            );
            assert_eq!(texture::png(&ktx2_to_png(&encoded)?)?, pixels);
        }
        assert!(png_to_ktx2(b"not a png").is_err());
        Ok(())
    }

    #[test]
    fn usage_lists_referring_documents_and_identical_copies() -> Result<()> {
        let root = tempfile::tempdir()?;
        let write = |path: &str, bytes: &[u8]| {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        write(
            "content.json",
            format!(
                r#"{{"version":{},"game":"{}"}}"#,
                resonance_content::library::VERSION,
                resonance_content::library::GAME
            )
            .as_bytes(),
        );
        write("meshes/a/jacket.ktx2", b"jacket pixels");
        write("meshes/b/jacket-copy.ktx2", b"jacket pixels");
        write("meshes/b/other.ktx2", b"other  pixels");
        write("meshes/b/same-size.glb", b"jacket pixels");
        write(
            "meshes/a/model.json",
            br#"{"textures":["meshes/a/jacket.ktx2"]}"#,
        );
        write(
            "meshes/b/model.json",
            br#"{"textures":["meshes/b/other.ktx2"]}"#,
        );
        let library = Library::open_paths(root.path(), &[])?;
        let found = usage(&library, "meshes/a/jacket.ktx2")?;
        assert_eq!(
            found.references,
            BTreeSet::from(["meshes/a/model.json".to_owned()])
        );
        assert_eq!(
            found.copies,
            BTreeSet::from(["meshes/b/jacket-copy.ktx2".to_owned()])
        );
        assert!(usage(&library, "meshes/a/missing.ktx2").is_err());
        Ok(())
    }
}
