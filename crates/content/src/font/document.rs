//! Characters that use the same bitmap and spacing share one editable entry.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    texture: String,
    width: u32,
    height: u32,
    line_height: u32,
    glyph_size: [u32; 2],
    advance: u32,
    glyphs: Vec<Group>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    characters: Characters,
    position: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    advance: Option<u32>,
}

/// Long repertoires can be split across strings without adding glyphs or lines.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Characters {
    Text(String),
    Lines(Vec<String>),
}
impl Characters {
    fn new(text: String) -> Self {
        let chars: Vec<_> = text.chars().collect();
        if chars.len() <= 64 {
            Self::Text(text)
        } else {
            Self::Lines(chars.chunks(64).map(|line| line.iter().collect()).collect())
        }
    }
    fn text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Lines(lines) => lines.concat(),
        }
    }
}

fn most_common<T: Copy + Ord>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut counts = BTreeMap::<T, usize>::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(value, count)| (*count, std::cmp::Reverse(*value)))
        .map(|(value, _)| value)
}

impl From<&BitmapFont> for Definition {
    fn from(font: &BitmapFont) -> Self {
        let glyph_size = most_common(
            font.glyphs
                .values()
                .map(|glyph| [glyph.rect[2], glyph.rect[3]]),
        )
        .unwrap_or([font.line_height; 2]);
        let advance = most_common(font.glyphs.values().map(|glyph| glyph.advance)).unwrap_or(0);
        let mut groups = BTreeMap::<Glyph, String>::new();
        for (&character, glyph) in &font.glyphs {
            groups.entry(glyph.clone()).or_default().push(character);
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by(|a, b| a.1.cmp(&b.1));
        Self {
            texture: font.texture.clone(),
            width: font.width,
            height: font.height,
            line_height: font.line_height,
            glyph_size,
            advance,
            glyphs: groups
                .into_iter()
                .map(|(glyph, characters)| {
                    let [x, y, w, h] = glyph.rect;
                    Group {
                        characters: Characters::new(characters),
                        position: [x, y],
                        size: ([w, h] != glyph_size).then_some([w, h]),
                        advance: (glyph.advance != advance).then_some(glyph.advance),
                    }
                })
                .collect(),
        }
    }
}

impl TryFrom<Definition> for BitmapFont {
    type Error = anyhow::Error;
    fn try_from(definition: Definition) -> Result<Self> {
        let mut glyphs = BTreeMap::new();
        for group in definition.glyphs {
            let characters = group.characters.text();
            ensure!(!characters.is_empty(), "font glyph entry has no characters");
            let [x, y] = group.position;
            let [w, h] = group.size.unwrap_or(definition.glyph_size);
            let glyph = Glyph {
                rect: [x, y, w, h],
                advance: group.advance.unwrap_or(definition.advance),
            };
            for character in characters.chars() {
                ensure!(glyphs.len() < 16384, "too many font characters");
                ensure!(
                    glyphs.insert(character, glyph.clone()).is_none(),
                    "duplicate font character {character:?}"
                );
            }
        }
        Ok(Self {
            texture: definition.texture,
            width: definition.width,
            height: definition.height,
            line_height: definition.line_height,
            glyphs,
        })
    }
}

impl Serialize for BitmapFont {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Definition::from(self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for BitmapFont {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Definition::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
