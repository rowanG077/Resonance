pub const GLOW_SPRITE: u16 = 0;
pub const SMOKE_SPRITE: u16 = 1;
pub const STATION_GLOW_SPRITE: u16 = 4;
pub const CAMERA_DISC_SPRITE: u16 = 5;
pub const WORLD_GLOW_SPRITE: u16 = 6;
pub const STAR_SPRITE: u16 = 7;
pub const SPINNING_STAR_SPRITE: u16 = 8;
pub const ORB_SPRITE: u16 = 10;
pub const FLAME_SPRITE: u16 = 11;
pub const TRAIL_GLOW_SPRITE: u16 = 12;
pub const BURST_SPRITE: u16 = 13;
pub const ELECTRIC_ARC_SPRITE: u16 = 14;
pub const SWIRL_SPRITE: u16 = 18;
pub const SPARKLE_CLUSTER_SPRITE: u16 = 21;
pub const STATION_HALO_SPRITE: u16 = 22;
pub const STREAK_SPRITE: u16 = 23;
pub const RING_SPRITE: u16 = 41;
pub const ELECTRIC_SPARK_SPRITE: u16 = 42;
pub const RISING_LIGHT_SPRITE: u16 = 44;
pub const DEBRIS_SPRITES: [u16; 3] = [52, 53, 54];
pub const SEAL_STAR_SPRITE: u16 = 68;
pub const SEAL_SPARK_SPRITE: u16 = 69;
pub const SQUARE_GLOW_SPRITE: u16 = 70;
pub const LIGHTNING_BOLT_SPRITE: u16 = 74;
pub const PLANE_LIGHT_SPRITES: [u16; 4] = [75, 76, 77, 78];
pub const FLAME_PLUME_SPRITE: u16 = 80;
pub const CYLINDER_RAY_SPRITE: u16 = 82;
pub const LIGHT_SHEET_SPRITE: u16 = 95;
pub const COOKING_CLOUDS: std::ops::RangeInclusive<u16> = 501..=505;

/// These are engine drawing roles, selected by the scenario effect protocol.
/// Their names belong in the editable definitions; numeric dispatch stays here.
const NAMES: [(&str, u16); 33] = [
    ("glow", GLOW_SPRITE),
    ("smoke", SMOKE_SPRITE),
    ("station-glow", STATION_GLOW_SPRITE),
    ("camera-disc", CAMERA_DISC_SPRITE),
    ("world-glow", WORLD_GLOW_SPRITE),
    ("star", STAR_SPRITE),
    ("spinning-star", SPINNING_STAR_SPRITE),
    ("orb", ORB_SPRITE),
    ("flame", FLAME_SPRITE),
    ("trail-glow", TRAIL_GLOW_SPRITE),
    ("burst", BURST_SPRITE),
    ("electric-arc", ELECTRIC_ARC_SPRITE),
    ("swirl", SWIRL_SPRITE),
    ("sparkle-cluster", SPARKLE_CLUSTER_SPRITE),
    ("station-halo", STATION_HALO_SPRITE),
    ("streak", STREAK_SPRITE),
    ("ring", RING_SPRITE),
    ("electric-spark", ELECTRIC_SPARK_SPRITE),
    ("rising-light", RISING_LIGHT_SPRITE),
    ("debris-1", DEBRIS_SPRITES[0]),
    ("debris-2", DEBRIS_SPRITES[1]),
    ("debris-3", DEBRIS_SPRITES[2]),
    ("seal-star", SEAL_STAR_SPRITE),
    ("seal-spark", SEAL_SPARK_SPRITE),
    ("square-glow", SQUARE_GLOW_SPRITE),
    ("lightning-bolt", LIGHTNING_BOLT_SPRITE),
    ("plane-light-1", PLANE_LIGHT_SPRITES[0]),
    ("plane-light-2", PLANE_LIGHT_SPRITES[1]),
    ("plane-light-3", PLANE_LIGHT_SPRITES[2]),
    ("plane-light-4", PLANE_LIGHT_SPRITES[3]),
    ("flame-plume", FLAME_PLUME_SPRITE),
    ("cylinder-ray", CYLINDER_RAY_SPRITE),
    ("light-sheet", LIGHT_SHEET_SPRITE),
];

pub fn all() -> impl ExactSizeIterator<Item = u16> {
    NAMES.iter().map(|&(_, id)| id)
}

pub fn name(id: u16) -> Option<&'static str> {
    NAMES
        .iter()
        .find_map(|&(name, value)| (value == id).then_some(name))
}

pub(super) mod named {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer, V: Serialize>(
        values: &BTreeMap<u16, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(values.len()))?;
        for (&id, value) in values {
            let name = super::name(id).ok_or_else(|| {
                serde::ser::Error::custom(format!("unknown field sprite role {id}"))
            })?;
            map.serialize_entry(name, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, V: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<u16, V>, D::Error> {
        BTreeMap::<String, V>::deserialize(deserializer)?
            .into_iter()
            .map(|(name, value)| {
                let id = super::NAMES
                    .iter()
                    .find_map(|&(label, id)| (label == name).then_some(id))
                    .ok_or_else(|| {
                        serde::de::Error::custom(format!("unknown field sprite role {name}"))
                    })?;
                Ok((id, value))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Roles(#[serde(with = "named")] BTreeMap<u16, u16>);

    #[test]
    fn named_roles_preserve_every_engine_selector() -> anyhow::Result<()> {
        let roles = Roles(all().map(|id| (id, id)).collect());
        let names: BTreeSet<_> = all().map(|id| name(id).unwrap()).collect();
        assert_eq!(names.len(), 33);
        let serialized = serde_json::to_value(&roles)?;
        assert_eq!(serialized["electric-spark"], ELECTRIC_SPARK_SPRITE);
        assert_eq!(serde_json::from_value::<Roles>(serialized)?.0, roles.0);
        assert!(serde_json::from_str::<Roles>(r#"{"42":42}"#).is_err());
        assert!(serde_json::from_str::<Roles>(r#"{"sparrk":42}"#).is_err());
        Ok(())
    }
}
