use super::*;

#[cfg(test)]
mod tests;

/// Menu prose with explicit line breaks, palette colors and controller icons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuText {
    pub lines: Vec<Vec<MenuSpan>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuSpan {
    Text { text: String, color: u8 },
    Button { sprite: u8 },
}

// Names describe the visible palette roles and glyphs, not input bindings.
const COLORS: [&str; 11] = [
    "black",
    "blue",
    "red",
    "purple",
    "green",
    "cyan",
    "yellow",
    "gray",
    "gold",
    "white",
    "pale_green",
];
const BUTTONS: [&str; 32] = [
    "stick",
    "stick_up",
    "stick_down",
    "stick_right",
    "stick_left",
    "a",
    "a_pressed",
    "b",
    "b_pressed",
    "x",
    "x_pressed",
    "y",
    "y_pressed",
    "l",
    "l_pressed",
    "r",
    "r_pressed",
    "start",
    "start_pressed",
    "panel_left",
    "panel_middle",
    "panel_right",
    "z",
    "z_pressed",
    "c_stick",
    "c_stick_up",
    "c_stick_down",
    "d_pad",
    "d_pad_up",
    "d_pad_down",
    "d_pad_right",
    "d_pad_left",
];

/// Resolve a visible controller glyph independently of its input assignment.
pub fn button_sprite(name: &str) -> Result<u8> {
    BUTTONS
        .iter()
        .position(|&value| value == name)
        .map(|id| id as u8)
        .with_context(|| format!("unknown menu button glyph {name}"))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Control {
    Color(String),
    Button(String),
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

fn append(parts: &mut Vec<Part>, text: &str) {
    if let Some(Part::Text(previous)) = parts.last_mut() {
        previous.push_str(text);
    } else if !text.is_empty() {
        parts.push(Part::Text(text.into()));
    }
}

impl Serialize for MenuText {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut parts = Vec::new();
        let mut current_color = 9;
        for (index, line) in self.lines.iter().enumerate() {
            if index != 0 {
                append(&mut parts, "\n");
            }
            for span in line {
                match span {
                    MenuSpan::Text { text, color } => {
                        if *color != current_color {
                            parts.push(Part::Control(Control::Color(
                                COLORS[usize::from(*color)].into(),
                            )));
                            current_color = *color;
                        }
                        append(&mut parts, text);
                    }
                    MenuSpan::Button { sprite } => parts.push(Part::Control(Control::Button(
                        BUTTONS[usize::from(*sprite)].into(),
                    ))),
                }
            }
        }
        match parts.as_slice() {
            [] => serializer.serialize_str(""),
            [Part::Text(text)] => serializer.serialize_str(text),
            _ => parts.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for MenuText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let parts = match Document::deserialize(deserializer)? {
            Document::Text(text) => vec![Part::Text(text)],
            Document::Parts(parts) => parts,
        };
        let mut result = Self {
            lines: vec![Vec::new()],
        };
        let mut color = 9;
        for part in parts {
            match part {
                Part::Text(text) => {
                    for (index, text) in text.split('\n').enumerate() {
                        if index != 0 {
                            result.lines.push(Vec::new());
                        }
                        if text.is_empty() {
                            continue;
                        }
                        let line = result.lines.last_mut().unwrap();
                        if let Some(MenuSpan::Text {
                            text: previous,
                            color: previous_color,
                        }) = line.last_mut()
                            && *previous_color == color
                        {
                            previous.push_str(text);
                        } else {
                            line.push(MenuSpan::Text {
                                text: text.into(),
                                color,
                            });
                        }
                    }
                }
                Part::Control(Control::Color(name)) => {
                    color = COLORS
                        .iter()
                        .position(|&value| value == name)
                        .ok_or_else(|| D::Error::custom(format!("unknown menu color {name}")))?
                        as u8;
                }
                Part::Control(Control::Button(name)) => {
                    let sprite = button_sprite(&name).map_err(D::Error::custom)?;
                    result
                        .lines
                        .last_mut()
                        .unwrap()
                        .push(MenuSpan::Button { sprite });
                }
            }
        }
        result.validate().map_err(D::Error::custom)?;
        Ok(result)
    }
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

    pub fn single_line(&self) -> Result<&[MenuSpan]> {
        self.validate()?;
        let [line] = self.lines.as_slice() else {
            anyhow::bail!("menu prompt must have one line");
        };
        ensure!(!line.is_empty(), "menu prompt has no spans");
        Ok(line)
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().flatten().filter_map(|s| match s {
            MenuSpan::Text { text, .. } => Some(text.as_str()),
            MenuSpan::Button { .. } => None,
        })
    }
}
