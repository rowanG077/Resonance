//! Named journal entries retain their saved event-record IDs in one mapping.
use super::*;
use crate::library::Mappings;

const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    mappings: Mappings,
    entries: BTreeMap<String, SynopsisEntry>,
    months: BTreeMap<String, String>,
}

impl Serialize for SynopsisData {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let mut entries = BTreeMap::<String, SynopsisEntry>::new();
        let mut mappings = BTreeMap::new();
        for (id, entry) in self.entries.iter().enumerate() {
            let key = if let Some((key, _)) = entries.iter().find(|(_, prior)| *prior == entry) {
                key.clone()
            } else {
                let key = crate::menu_data::document::unique_name(&entry.title, &entries);
                entries.insert(key.clone(), entry.clone());
                key
            };
            mappings.insert(u32::try_from(id).map_err(S::Error::custom)?, key);
        }
        Document {
            mappings: Mappings::new(mappings).map_err(S::Error::custom)?,
            entries,
            months: MONTHS
                .iter()
                .zip(&self.months)
                .map(|(key, value)| ((*key).into(), value.clone()))
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SynopsisData {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let document = Document::deserialize(deserializer)?;
        if document.mappings.iter().count() != SYNOPSIS_COUNT {
            return Err(D::Error::custom("synopsis needs all 200 saved event IDs"));
        }
        let entries = (0..SYNOPSIS_COUNT)
            .map(|id| {
                let key = document
                    .mappings
                    .target(id as u32)
                    .map_err(D::Error::custom)?;
                document
                    .entries
                    .get(key)
                    .cloned()
                    .ok_or_else(|| D::Error::custom(format!("missing synopsis entry {key}")))
            })
            .collect::<Result<_, _>>()?;
        let months = crate::ui_art::ordered(document.months, MONTHS)
            .map_err(D::Error::custom)?
            .try_into()
            .ok()
            .unwrap();
        Ok(Self { entries, months })
    }
}

pub(super) mod world {
    use super::*;
    use crate::overworld::World;
    pub fn serialize<S: serde::Serializer>(id: &u8, serializer: S) -> Result<S::Ok, S::Error> {
        let world = match id {
            0 => World::Sylvarant,
            1 => World::TetheAlla,
            _ => return Err(serde::ser::Error::custom("invalid synopsis world")),
        };
        world.serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
        Ok(World::deserialize(deserializer)?.index() as u8)
    }
}
