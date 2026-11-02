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
        if text.chars().count() != 104 || text.chars().any(char::is_control) {
            return Err(serde::ser::Error::custom(
                "name keyboard needs eight rows of thirteen character cells",
            ));
        }
        text.chars()
            .collect::<Vec<_>>()
            .chunks(13)
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<String, D::Error> {
        let rows = <[String; 8]>::deserialize(deserializer)?;
        if rows
            .iter()
            .any(|row| row.chars().count() != 13 || row.chars().any(char::is_control))
        {
            return Err(serde::de::Error::custom(
                "name keyboard row needs thirteen character cells",
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
        ensure!(
            self.keyboard.chars().count() == 13 * 8,
            "invalid name keyboard"
        );
        ensure!(
            self.texts()
                .all(|s| !s.is_empty() && s.chars().all(|c| !c.is_control())),
            "invalid name editor text"
        );
        ensure!(
            self.defaults.iter().all(|n| n.chars().count() <= 12),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_name_editor_text_keeps_unicode_and_cell_limits() -> Result<()> {
        let mut data = RenameData {
            defaults: std::array::from_fn(|_| "Génis".into()),
            keyboard: "éあcdefghijklm".repeat(8),
            heading: "Personnage".into(),
            delete: "Effacer".into(),
            default: "Défaut".into(),
            commands: ["Confirmer".into(), "Rétablir".into(), "Annuler".into()],
        };
        data.defaults[0] = "é".repeat(12);
        data.validate()?;
        let restored: RenameData = serde_json::from_slice(&serde_json::to_vec(&data)?)?;
        restored.validate()?;
        assert_eq!(restored.defaults, data.defaults);
        assert_eq!(restored.keyboard, data.keyboard);
        data.defaults[0].push('é');
        assert!(data.validate().is_err());
        data.defaults[0] = "Lloyd".into();
        data.heading.push('\n');
        assert!(data.validate().is_err());
        Ok(())
    }
}
