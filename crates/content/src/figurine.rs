//! Figurine names and converted model variations.
use crate::model_preview::ModelPreview;
use serde::{Deserialize, Serialize};

pub const FIGURINE_COUNT: usize = 288;
pub const FIGURINE_VERSION: u32 = 3;

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
        self.preview.validate_definition()
    }
}
impl FigurineBook {
    pub fn load(files: &crate::prepared::Files) -> anyhow::Result<Self> {
        let value = files.json(crate::menu_data::FIGURINES_PATH)?;
        let (value, paths) =
            crate::model_preview::document::expand_catalogue(value, |path| files.read(path))?;
        let mut book: Self = serde_json::from_value(value)?;
        for (record, path) in book.records.iter_mut().zip(paths) {
            record.preview.document = Some(path);
        }
        Ok(book)
    }

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
