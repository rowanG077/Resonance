use crate::error::{Result, bad, fail};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MIB: u64 = 1024 * 1024;
pub const PART_BYTES: u64 = 8 * MIB;
pub const SESSION_AGE: i64 = 7 * 24 * 60 * 60;
pub const UPLOAD_AGE: i64 = 24 * 60 * 60;
pub const CATEGORIES: &[&str] = &["Gameplay", "Visuals", "Audio", "Translation", "Other"];
pub const REPO: &str = "https://github.com/rowanG077/Resonance";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadLimits {
    pub zip_mib: u64,
    pub expanded_mib: u64,
}
impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            zip_mib: 64,
            expanded_mib: 256,
        }
    }
}
impl UploadLimits {
    pub fn validate(self) -> Result<Self> {
        if !(64..=16384).contains(&self.zip_mib)
            || !(256..=65536).contains(&self.expanded_mib)
            || self.expanded_mib < self.zip_mib
        {
            return Err(bad(
                "Use 64–16384 MiB for the ZIP and 256–65536 MiB for expanded files. The expanded limit must be at least the ZIP limit.",
            ));
        }
        Ok(self)
    }
    pub fn zip_bytes(self) -> u64 {
        self.zip_mib * MIB
    }
    pub fn expanded_bytes(self) -> u64 {
        self.expanded_mib * MIB
    }
    pub fn entries(self) -> usize {
        if self == Self::default() {
            4096
        } else {
            100_000
        }
    }
    pub fn check_size(self, size: u64) -> Result<()> {
        if size == 0 {
            return Err(bad("Choose a nonempty ZIP file."));
        }
        if size > self.zip_bytes() {
            return Err(fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "Your ZIP limit is {} MiB. Ask an administrator to raise it for larger uploads.",
                    self.zip_mib
                ),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModDraft {
    #[serde(default)]
    pub mod_id: Option<String>,
    pub title: String,
    pub summary: String,
    pub description: String,
    pub category: String,
    pub version: String,
    pub resonance_version: String,
    pub content_version: u32,
    pub rights: bool,
}
impl ModDraft {
    pub fn validate(mut self) -> Result<Self> {
        for (name, value, min, max) in [
            ("Mod name", &mut self.title, 3, 80),
            ("Summary", &mut self.summary, 10, 180),
            ("Description", &mut self.description, 20, 12000),
        ] {
            *value = value.trim().to_owned();
            if !(min..=max).contains(&value.len()) {
                return Err(bad(format!("{name} must contain {min}–{max} bytes.")));
            }
        }
        if !CATEGORIES.contains(&self.category.as_str()) {
            return Err(bad("Choose a valid category."));
        }
        for version in [&self.version, &self.resonance_version] {
            if version.len() > 40 || semver::Version::parse(version).is_err() {
                return Err(bad("Use major.minor.patch versions, for example 1.0.0."));
            }
        }
        if self.content_version == 0 {
            return Err(bad("Content format must be a positive integer."));
        }
        if !self.rights {
            return Err(bad("Confirm that you have the right to share these files."));
        }
        self.mod_id = self.mod_id.filter(|id| !id.is_empty());
        Ok(self)
    }
}
pub fn newer(version: &str, previous: &str) -> Result<()> {
    if !semver::Version::parse(version)
        .map_err(|_| bad("Invalid version."))?
        .cmp_precedence(&semver::Version::parse(previous).map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Stored release version is invalid.",
            )
        })?)
        .is_gt()
    {
        return Err(bad(
            "The release version must be greater than the current version.",
        ));
    }
    Ok(())
}
pub fn valid_token(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
pub fn hash(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}
pub fn part_length(size: u64, part: u32) -> Result<usize> {
    let offset = u64::from(
        part.checked_sub(1)
            .ok_or_else(|| bad("Invalid part number."))?,
    ) * PART_BYTES;
    if offset >= size {
        return Err(bad("Part number exceeds the archive size."));
    }
    Ok((size - offset).min(PART_BYTES) as usize)
}
pub fn fingerprint_valid(value: &str) -> bool {
    value.len() == 128 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
