//! Figurine names and converted model variations.
use crate::model_preview::ModelPreview;
use serde::{Deserialize, Serialize};

pub const FIGURINE_COUNT: usize = 288;
pub const FIGURINE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FigurineBook {
    pub title: String,
    pub records: Vec<Figurine>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figurine {
    pub version: u32,
    pub id: u16,
    pub name: String,
    pub preview: ModelPreview,
}
impl Figurine {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == FIGURINE_VERSION
                && usize::from(self.id) < FIGURINE_COUNT
                && !self.name.is_empty(),
            "invalid figurine record"
        );
        self.preview.validate()
    }
}
impl FigurineBook {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.title.is_empty() && self.records.len() == FIGURINE_COUNT,
            "incomplete figurine catalogue"
        );
        for (id, record) in self.records.iter().enumerate() {
            anyhow::ensure!(usize::from(record.id) == id, "unordered figurine catalogue");
            record.validate()?;
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.as_str()).chain(self.records.iter().map(|r| r.name.as_str()))
    }
}
