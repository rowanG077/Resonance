//! Read named resources directly from the installed Steam archive.
use crate::{archive::Directory, pe};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

const ARCHIVES: &str = "P12E/Data/TOS_Server/Archive/Release";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    French,
    Spanish,
    German,
    Italian,
}

impl Language {
    pub(crate) fn slot(self) -> u32 {
        match self {
            Self::English => 1,
            Self::French => 2,
            Self::Spanish => 3,
            Self::German => 4,
            Self::Italian => 5,
        }
    }

    pub(crate) fn region(self) -> &'static str {
        match self {
            Self::English => "R01",
            Self::French => "R02",
            Self::Spanish => "R03",
            Self::German => "R04",
            Self::Italian => "R05",
        }
    }
}

impl std::str::FromStr for Language {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(match value {
            "english" => Self::English,
            "french" => Self::French,
            "spanish" => Self::Spanish,
            "german" => Self::German,
            "italian" => Self::Italian,
            _ => anyhow::bail!(
                "unknown text language {value}; expected english, french, spanish, german or italian"
            ),
        })
    }
}

struct Archive {
    directory: Directory,
    data: PathBuf,
}

impl Archive {
    fn open(root: &Path, region: &str) -> Result<Self> {
        let root = root.join(ARCHIVES).join(region).join("WIN");
        let data = root.join("TLFILE.TLDAT");
        let directory = Directory::parse(
            &fs::read(root.join("FILEHEADER.TOFHDB"))?,
            fs::metadata(&data)?.len(),
        )
        .with_context(|| format!("read Steam archive {}", root.display()))?;
        Ok(Self { directory, data })
    }

    fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.directory.find(name) else {
            return Ok(None);
        };
        let extension = name
            .rsplit_once('.')
            .context("resource needs an extension")?
            .1;
        ensure!(
            entry.extension().eq_ignore_ascii_case(extension),
            "Steam resource hash matched a different extension: {name}"
        );
        entry
            .read(&mut fs::File::open(&self.data)?)
            .map(Some)
            .with_context(|| format!("read Steam resource {name}"))
    }
}

pub(crate) struct Source {
    pub executable: Vec<u8>,
    pub language: Language,
    base: Archive,
    localized: Option<Archive>,
}

impl Source {
    pub fn open(root: &Path, language: Language) -> Result<Self> {
        let executable = fs::read(root.join("TOS.exe")).context("Steam input needs TOS.exe")?;
        pe::Image::parse(&executable).context("read Steam executable")?;
        if root.join("steam_appid.txt").try_exists()? {
            ensure!(
                fs::read_to_string(root.join("steam_appid.txt"))?.trim() == "372360",
                "expected Steam app 372360"
            );
        }
        Ok(Self {
            executable,
            language,
            base: Archive::open(root, "R01")?,
            localized: (language != Language::English)
                .then(|| Archive::open(root, language.region()))
                .transpose()?,
        })
    }

    pub fn contains(&self, name: &str) -> Result<bool> {
        validate_name(name)?;
        Ok(self
            .localized
            .as_ref()
            .is_some_and(|a| a.directory.find(name).is_some())
            || self.base.directory.find(name).is_some())
    }

    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        validate_name(name)?;
        if let Some(localized) = &self.localized {
            if let Some(bytes) = localized.read(name)? {
                return Ok(Some(bytes));
            }
        }
        self.base.read(name)
    }

    pub fn require(&self, name: &str) -> Result<Vec<u8>> {
        self.read(name)?
            .with_context(|| format!("missing Steam resource {name}"))
    }
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 255
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid Steam resource name {name:?}"
    );
    Ok(())
}
