//! Request IDs select explicit horizontal/vertical impulses; guard defaults belong
//! to named characters, using the ordinary party mapping.
use super::*;
use crate::library::Mappings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recoil {
    pub impulses: BTreeMap<u8, Impulse>,
    pub suppression_distance: f32,
    pub guard_speed: f32,
    pub light_vertical_scale: f32,
    pub heavy_vertical_scale: f32,
    pub default_guard_recovery_bonuses: BTreeMap<String, u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Impulse {
    pub horizontal: f32,
    pub vertical: f32,
}
impl Recoil {
    pub fn from_table(value: recoil::Table, names: &Mappings) -> Result<Self> {
        Ok(Self {
            impulses: value
                .impulses
                .into_iter()
                .enumerate()
                .map(|(id, [horizontal, vertical])| {
                    Ok((
                        u8::try_from(id)?,
                        Impulse {
                            horizontal,
                            vertical,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            suppression_distance: value.suppression_distance,
            guard_speed: value.guard_speed,
            light_vertical_scale: value.light_vertical_scale,
            heavy_vertical_scale: value.heavy_vertical_scale,
            default_guard_recovery_bonuses: value
                .default_guard_recovery_bonuses
                .into_iter()
                .map(|(id, bonus)| Ok((names.target(id.into())?.into(), bonus)))
                .collect::<Result<_>>()?,
        })
    }
    pub fn prepare(self, names: &Mappings) -> Result<recoil::Table> {
        ensure!(
            !self.impulses.is_empty()
                && self
                    .impulses
                    .keys()
                    .enumerate()
                    .all(|(index, &id)| index == usize::from(id)),
            "recoil request IDs must be contiguous from zero"
        );
        Ok(recoil::Table {
            impulses: self
                .impulses
                .into_values()
                .map(|v| [v.horizontal, v.vertical])
                .collect(),
            suppression_distance: self.suppression_distance,
            guard_speed: self.guard_speed,
            light_vertical_scale: self.light_vertical_scale,
            heavy_vertical_scale: self.heavy_vertical_scale,
            default_guard_recovery_bonuses: self
                .default_guard_recovery_bonuses
                .into_iter()
                .map(|(name, bonus)| Ok((u8::try_from(names.id(&name)?)?, bonus)))
                .collect::<Result<_>>()?,
        })
    }
}
