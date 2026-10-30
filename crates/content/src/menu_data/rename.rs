use super::*;

pub const RENAME_GEM: u16 = 499;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameData {
    /// Native reset presets are independent of ordinary character names.
    #[serde(with = "crate::ui_art::characters")]
    pub defaults: [String; 9],
    #[serde(with = "keyboard")]
    pub keyboard: String,
    pub heading: String,
    pub delete: String,
    pub default: String,
    #[serde(with = "commands")]
    pub commands: [String; 3],
}

mod keyboard {
    use super::*;
    pub fn serialize<S: serde::Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        if !text.is_ascii() || text.len() != 104 {
            return Err(serde::ser::Error::custom(
                "name keyboard needs eight rows of thirteen ASCII cells",
            ));
        }
        (0..8)
            .map(|row| &text[row * 13..(row + 1) * 13])
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<String, D::Error> {
        let rows = <[String; 8]>::deserialize(deserializer)?;
        if rows.iter().any(|row| !row.is_ascii() || row.len() != 13) {
            return Err(serde::de::Error::custom(
                "name keyboard row needs thirteen ASCII cells",
            ));
        }
        Ok(rows.concat())
    }
}

mod commands {
    use super::*;
    const NAMES: &[&str] = &["confirm", "restore", "cancel"];
    pub fn serialize<S: serde::Serializer>(
        values: &[String; 3],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        crate::ui_art::named(values, NAMES)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[String; 3], D::Error> {
        Ok(crate::ui_art::ordered(
            BTreeMap::<String, String>::deserialize(deserializer)?,
            NAMES,
        )
        .map_err(serde::de::Error::custom)?
        .try_into()
        .ok()
        .unwrap())
    }
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
            self.defaults.iter().all(|n| n.len() <= 12),
            "invalid character name"
        );
        Ok(())
    }
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.defaults
            .iter()
            .chain(&self.commands)
            .chain([&self.keyboard, &self.heading, &self.delete, &self.default])
            .map(String::as_str)
    }
}
