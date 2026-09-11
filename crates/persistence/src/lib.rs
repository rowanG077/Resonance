//! Shared JSON files and independent slots for player saves and field quicksaves.
mod slots;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub use slots::{SlotId, Store, WriteTask, default_directory, read_bounded};

pub const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Save,
    Quicksave,
}
impl Kind {
    pub fn directory(self) -> &'static str {
        match self {
            Self::Save => "saves",
            Self::Quicksave => "quicksaves",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub schema: u32,
    pub content: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub identity: Identity,
    pub label: String,
    pub location: String,
    pub played_ticks: u64,
    pub saved_unix_seconds: u64,
}
impl Header {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.identity.schema > 0 && self.label.len() <= 256 && self.location.len() <= 256,
            "invalid save metadata"
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Save<T> {
    header: Header,
    state: T,
}

pub fn encode<T: Serialize>(header: &Header, state: &T) -> Result<Vec<u8>> {
    header.validate()?;
    let bytes = serde_json::to_vec_pretty(&Save {
        header: header.clone(),
        state,
    })?;
    ensure!(bytes.len() <= MAX_FILE_BYTES, "save exceeds 1 MiB");
    Ok(bytes)
}

pub fn inspect(bytes: &[u8]) -> Result<Header> {
    ensure!(bytes.len() <= MAX_FILE_BYTES, "save exceeds 1 MiB");
    let header = serde_json::from_slice::<Save<serde::de::IgnoredAny>>(bytes)?.header;
    header.validate()?;
    Ok(header)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8], expected: &Identity) -> Result<(Header, T)> {
    let header = inspect(bytes)?;
    ensure!(
        &header.identity == expected,
        "incompatible save schema or content"
    );
    let state = serde_json::from_slice::<Save<T>>(bytes)?.state;
    Ok((header, state))
}
