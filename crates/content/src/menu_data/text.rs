use super::*;

/// Menu prose with explicit line breaks, palette colors and controller icons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuText {
    pub lines: Vec<Vec<MenuSpan>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuSpan {
    Text { text: String, color: u8 },
    Button { sprite: u8 },
}

impl MenuText {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.lines.is_empty(), "menu text has no lines");
        for span in self.lines.iter().flatten() {
            ensure!(
                match span {
                    MenuSpan::Text { text, color } =>
                        !text.is_empty() && *color <= 10 && text.chars().all(|c| !c.is_control()),
                    MenuSpan::Button { sprite } => *sprite < 32,
                },
                "invalid menu text span"
            );
        }
        Ok(())
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().flatten().filter_map(|s| match s {
            MenuSpan::Text { text, .. } => Some(text.as_str()),
            MenuSpan::Button { .. } => None,
        })
    }
}
