use super::*;

pub const RENAME_GEM: u16 = 499;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameData {
    pub initial_names: [String; 9],
    pub defaults: [String; 9],
    pub keyboard: String,
    pub heading: String,
    pub delete: String,
    pub default: String,
    pub commands: [String; 3],
}

impl RenameData {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.keyboard.len() == 13 * 8, "invalid name keyboard");
        ensure!(
            self.texts()
                .all(|s| !s.is_empty() && s.bytes().all(|b| (32..127).contains(&b))),
            "invalid name editor text"
        );
        ensure!(
            self.initial_names
                .iter()
                .chain(&self.defaults)
                .all(|n| n.len() <= 12),
            "invalid character name"
        );
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.initial_names
            .iter()
            .chain(&self.defaults)
            .chain(&self.commands)
            .chain([&self.keyboard, &self.heading, &self.delete, &self.default])
            .map(String::as_str)
    }
}
