use super::*;
mod document;

pub const SYNOPSIS_COUNT: usize = 200;
pub const SYNOPSIS_LIST_ROWS: usize = 12;
pub const SYNOPSIS_TEXT_ROWS: usize = 13;

#[derive(Debug, Clone)]
pub struct SynopsisData {
    pub entries: Vec<SynopsisEntry>,
    pub months: [String; 12],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SynopsisEntry {
    pub heading: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<SynopsisLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<SynopsisLine>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub updates: BTreeMap<u8, Vec<SynopsisLine>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynopsisLocation {
    #[serde(with = "document::world")]
    pub world: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[i16; 2]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynopsisLine(pub Vec<SynopsisSpan>);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynopsisSpan {
    pub text: String,
    pub color: u8,
}

impl Serialize for SynopsisLine {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        MenuText {
            lines: vec![
                self.0
                    .iter()
                    .map(|span| MenuSpan::Text {
                        text: span.text.clone(),
                        color: span.color,
                    })
                    .collect(),
            ],
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SynopsisLine {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let text = MenuText::deserialize(deserializer)?;
        if text.lines.len() != 1 {
            return Err(D::Error::custom(
                "a synopsis line cannot contain a line break",
            ));
        }
        let spans = text
            .lines
            .into_iter()
            .next()
            .unwrap()
            .into_iter()
            .map(|span| match span {
                MenuSpan::Text { text, color } => Ok(SynopsisSpan { text, color }),
                MenuSpan::Button { .. } => Err(D::Error::custom(
                    "synopsis lines do not display button glyphs",
                )),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self(spans))
    }
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
                    && entry.updates.keys().all(|state| (2..=3).contains(state))
                    && std::iter::once(&entry.text)
                        .chain(entry.updates.values())
                        .all(|lines| lines.len() <= 512
                            && lines
                                .iter()
                                .flat_map(|line| &line.0)
                                .all(|span| span.color <= 10
                                    && !span.text.is_empty()
                                    && span.text.chars().all(|ch| !ch.is_control()))),
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
                    std::iter::once(&entry.text)
                        .chain(entry.updates.values())
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
        self.updates.get(&state).unwrap_or(&self.text)
    }
}
