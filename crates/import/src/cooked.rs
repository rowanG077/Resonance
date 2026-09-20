//! Resolve original source identities to their shared cooked publications.
use anyhow::{Context, Result, ensure};
use resonance_content::validate_asset_path;
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) struct Source<'a> {
    root: &'a Path,
    paths: Vec<String>,
}

impl<'a> Source<'a> {
    /// Embedded tables are published as files rather than archive directories.
    #[cfg(test)]
    pub(crate) fn document<T: serde::de::DeserializeOwned>(&self, suffix: &str) -> Result<T> {
        serde_json::from_slice(&self.document_bytes(suffix)?)
            .with_context(|| format!("invalid cooked {suffix}"))
    }

    #[cfg(test)]
    fn document_bytes(&self, suffix: &str) -> Result<Vec<u8>> {
        validate_asset_path(suffix)?;
        let mut result = None;
        for path in &self.paths {
            if path != suffix && !path.ends_with(&format!("/{suffix}")) {
                continue;
            }
            let bytes = fs::read(self.root.join(path))?;
            if let Some(previous) = &result {
                ensure!(previous == &bytes, "conflicting cooked {suffix}");
            }
            result = Some(bytes);
        }
        result.with_context(|| format!("missing cooked {suffix}; rerun cook-all"))
    }

    #[cfg(test)]
    pub(crate) fn publications(&self) -> &[String] {
        &self.paths
    }

    #[cfg(test)]
    pub(crate) fn open(root: &'a Path, disc: u8, source: &str) -> Result<Self> {
        let sources: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(root.join("sources.json")).with_context(|| {
                format!(
                    "missing cooked source index; run cook-all --output {} first",
                    root.display()
                )
            })?)?;
        Self::new(root, &sources, disc, source)
    }

    pub(crate) fn new(
        root: &'a Path,
        sources: &BTreeMap<String, Vec<String>>,
        disc: u8,
        source: &str,
    ) -> Result<Self> {
        let source = format!("disc{disc}/{source}");
        let directories = sources
            .get(&source)
            .with_context(|| format!("missing cooked {source}; rerun cook-all"))?;
        for directory in directories {
            validate_asset_path(directory)?;
        }
        let mut paths = directories.clone();
        paths.sort();
        paths.dedup();
        Ok(Self { root, paths })
    }

    #[cfg(test)]
    pub(crate) fn resolve(&self, relative: &str) -> Result<(&str, Vec<u8>)> {
        let (directories, bytes) = self.candidates(relative)?;
        Ok((directories[0], bytes))
    }

    /// Archive records use paths relative to their publication's root.
    #[cfg(test)]
    pub(crate) fn textures(&self, resource: &str) -> Result<Vec<crate::texture::Texture>> {
        let (directories, _) = self.candidates(&format!("{resource}/textures.json"))?;
        let mut textures = crate::texture::bind(&self.root.join(directories[0]), resource)?;
        for image in textures.iter_mut().flat_map(|texture| &mut texture.images) {
            self.verify_file(&directories, image)?;
            if !directories[0].is_empty() {
                *image = format!("{}/{image}", directories[0]);
            }
        }
        Ok(textures)
    }

    /// Embedded banks are published as named directories inside a shared namespace.
    #[cfg(test)]
    pub(crate) fn published_textures(&self, suffix: &str) -> Result<Vec<crate::texture::Texture>> {
        validate_asset_path(suffix)?;
        let paths = self
            .paths
            .iter()
            .filter_map(|path| {
                if path == suffix {
                    Some(String::new())
                } else {
                    path.strip_suffix(&format!("/{suffix}")).map(str::to_owned)
                }
            })
            .collect::<Vec<_>>();
        ensure!(
            !paths.is_empty(),
            "missing cooked texture bank {suffix}; rerun cook-all"
        );
        Self {
            root: self.root,
            paths,
        }
        .textures(suffix)
    }

    /// Standalone images already use paths relative to the whole library.
    #[cfg(test)]
    pub(crate) fn standalone_textures(&self) -> Result<Vec<crate::texture::Texture>> {
        let (directory, _) = self.resolve("textures.json")?;
        crate::texture::bind(self.root, directory)
    }

    /// The common effect loader accepts one decompressed texture payload.
    #[cfg(test)]
    pub(crate) fn cabinet_textures(&self) -> Result<Vec<crate::texture::Texture>> {
        let (directory, bytes) = self.resolve("cabinet.json")?;
        let members: Vec<String> = serde_json::from_slice(&bytes)?;
        let [member] = members.as_slice() else {
            anyhow::bail!("effect archive needs one texture payload");
        };
        validate_asset_path(member)?;
        crate::texture::bind(self.root, &format!("{directory}/{member}"))
    }

    pub(crate) fn verify_file(&self, directories: &[&str], path: &str) -> Result<()> {
        validate_asset_path(path)?;
        let directory = directories[0];
        ensure!(
            self.root.join(directory).join(path).is_file(),
            "missing cooked file {path}; rerun cook-all"
        );
        if directories.len() > 1 {
            let hash = crate::media::hash_file(&self.root.join(directory).join(path))?;
            for other in &directories[1..] {
                ensure!(
                    crate::media::hash_file(&self.root.join(other).join(path))? == hash,
                    "conflicting cooked file {path} in {directory} and {other}"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn candidates(&self, relative: &str) -> Result<(Vec<&str>, Vec<u8>)> {
        validate_asset_path(relative)?;
        let mut found: Option<(&str, Vec<u8>)> = None;
        let mut directories = Vec::new();
        for directory in &self.paths {
            let path = self.root.join(directory).join(relative);
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).with_context(|| path.display().to_string()),
            };
            if let Some((previous, data)) = &found {
                ensure!(
                    data == &bytes,
                    "conflicting cooked {relative} in {previous} and {directory}"
                );
            } else {
                found = Some((directory, bytes));
            }
            directories.push(directory.as_str());
        }
        let (_, bytes) = found.with_context(|| {
            format!(
                "missing cooked {relative}; rerun cook-all --output {}",
                self.root.display()
            )
        })?;
        Ok((directories, bytes))
    }
}
