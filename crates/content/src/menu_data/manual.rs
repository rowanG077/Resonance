use super::*;

pub const MANUAL_CHAPTERS: usize = 9;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingManual {
    pub title: String,
    pub chapters: Vec<ManualChapter>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualChapter {
    pub name: String,
    pub topics: Vec<ManualTopic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualTopic {
    pub name: String,
    pub learned_flag: u16,
    pub paragraphs: Vec<MenuText>,
}
impl TrainingManual {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.chapters.len() == MANUAL_CHAPTERS && !self.title.is_empty(),
            "incomplete training manual"
        );
        let mut flags = std::collections::BTreeSet::new();
        for chapter in &self.chapters {
            ensure!(
                !chapter.name.is_empty() && (1..=5).contains(&chapter.topics.len()),
                "invalid manual chapter"
            );
            for topic in &chapter.topics {
                ensure!(
                    !topic.name.is_empty()
                        && topic.learned_flag > 0
                        && flags.insert(topic.learned_flag)
                        && (1..=32).contains(&topic.paragraphs.len()),
                    "invalid manual topic"
                );
                for paragraph in &topic.paragraphs {
                    ensure!(
                        (1..=4).contains(&paragraph.lines.len()),
                        "manual paragraph exceeds panel"
                    );
                    paragraph.validate()?;
                }
            }
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.title.as_str()).chain(self.chapters.iter().flat_map(|c| {
            std::iter::once(c.name.as_str()).chain(c.topics.iter().flat_map(|t| {
                std::iter::once(t.name.as_str())
                    .chain(t.paragraphs.iter().flat_map(MenuText::texts))
            }))
        }))
    }
}
