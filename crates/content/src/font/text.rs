//! Literal system prose uses the same named color controls as dialogue.
use super::TextSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemText(pub Vec<TextSpan>);

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Color {
    White,
    Blue,
    Red,
    Purple,
    Green,
    Cyan,
    Yellow,
}
const COLORS: [Color; 7] = [
    Color::White,
    Color::Blue,
    Color::Red,
    Color::Purple,
    Color::Green,
    Color::Cyan,
    Color::Yellow,
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    color: Color,
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Part {
    Text(String),
    Control(Control),
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Document {
    Text(String),
    Parts(Vec<Part>),
}

impl Serialize for SystemText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut parts = Vec::new();
        let mut color = 0;
        for span in &self.0 {
            span.validate().map_err(serde::ser::Error::custom)?;
            if span.color != color {
                color = span.color;
                let name = COLORS[usize::from(color)];
                parts.push(Part::Control(Control { color: name }));
            }
            if let Some(Part::Text(text)) = parts.last_mut() {
                text.push_str(&span.text);
            } else if !span.text.is_empty() {
                parts.push(Part::Text(span.text.clone()));
            }
        }
        let document = if parts.is_empty() {
            Document::Text(String::new())
        } else if parts.len() == 1 && matches!(parts[0], Part::Text(_)) {
            let Part::Text(text) = parts.pop().unwrap() else {
                unreachable!()
            };
            Document::Text(text)
        } else {
            Document::Parts(parts)
        };
        document.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SystemText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let parts = match Document::deserialize(deserializer)? {
            Document::Text(text) => vec![Part::Text(text)],
            Document::Parts(parts) => parts,
        };
        let mut spans: Vec<TextSpan> = Vec::new();
        let mut color = 0;
        for part in parts {
            match part {
                Part::Control(control) => color = control.color as u8,
                Part::Text(text) if !text.is_empty() => {
                    let span = TextSpan { text, color };
                    span.validate().map_err(serde::de::Error::custom)?;
                    if let Some(previous) = spans.last_mut().filter(|s| s.color == color) {
                        previous.text.push_str(&span.text);
                    } else {
                        spans.push(span);
                    }
                }
                _ => {}
            }
        }
        Ok(Self(spans))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_notices_keep_colors_line_breaks_and_empty_text() -> anyhow::Result<()> {
        let source =
            r#"["Press",{"color":"green"}," A Button",{"color":"white"}," to\ncontinue.\fNext"]"#;
        let text: SystemText = serde_json::from_str(source)?;
        assert_eq!(
            text.0
                .iter()
                .map(|s| (s.text.as_str(), s.color))
                .collect::<Vec<_>>(),
            [
                ("Press", 0),
                (" A Button", 4),
                (" to\ncontinue.\u{c}Next", 0)
            ]
        );
        assert_eq!(serde_json::to_string(&text)?, source);
        let empty: SystemText = serde_json::from_str(r#""""#)?;
        assert!(empty.0.is_empty());
        assert_eq!(serde_json::to_string(&empty)?, r#""""#);
        for invalid in [
            r#"[{"color":"flashing"},"text"]"#,
            r#"[{"number":12}]"#,
            r#"[{"color":"white","unknown":1}]"#,
        ] {
            assert!(serde_json::from_str::<SystemText>(invalid).is_err());
        }
        Ok(())
    }
}
