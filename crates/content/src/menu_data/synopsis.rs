use super::*;

pub const SYNOPSIS_COUNT: usize = 200;
pub const SYNOPSIS_LIST_ROWS: usize = 12;
pub const SYNOPSIS_TEXT_ROWS: usize = 13;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynopsisData {
    pub entries: Vec<SynopsisEntry>,
    pub months: [String; 12],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynopsisEntry {
    pub heading: String,
    pub title: String,
    pub location: Option<SynopsisLocation>,
    pub text: [Option<Vec<SynopsisLine>>; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynopsisLocation {
    pub world: u8,
    pub point: Option<[i16; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynopsisLine(pub Vec<SynopsisSpan>);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynopsisSpan {
    pub text: String,
    pub color: u8,
}
impl SynopsisData {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.entries.len() == SYNOPSIS_COUNT,
            "invalid synopsis count"
        );
        for entry in &self.entries {
            ensure!(
                entry.location.as_ref().is_none_or(|p| p.world < 2)
                    && entry.text.iter().flatten().all(|lines| !lines.is_empty()
                        && lines.len() <= 512
                        && lines
                            .iter()
                            .flat_map(|line| &line.0)
                            .all(|span| span.color <= 10)),
                "invalid synopsis entry"
            );
        }
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .flat_map(|entry| {
                [&entry.heading, &entry.title].into_iter().chain(
                    entry
                        .text
                        .iter()
                        .flatten()
                        .flatten()
                        .flat_map(|line| line.0.iter().map(|s| &s.text)),
                )
            })
            .chain(&self.months)
            .map(String::as_str)
    }
}
impl SynopsisEntry {
    pub fn lines(&self, state: u8) -> &[SynopsisLine] {
        self.text
            .get(usize::from(state.saturating_sub(1)))
            .and_then(Option::as_ref)
            .or(self.text[0].as_ref())
            .map_or(&[], Vec::as_slice)
    }
}
