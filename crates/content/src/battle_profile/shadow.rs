//! An omitted shadow uses the ordinary footprint; null disables it.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shadow {
    #[serde(skip_serializing_if = "crate::defaults::is_unit")]
    pub scale: f32,
    #[serde(skip_serializing_if = "ordinary_color")]
    pub color: [u8; 4],
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            scale: 1.,
            color: [16, 16, 16, 176],
        }
    }
}

fn ordinary_color(color: &[u8; 4]) -> bool {
    *color == Shadow::default().color
}
pub(super) fn default() -> Option<Shadow> {
    Some(Shadow::default())
}
pub(super) fn is_default(value: &Option<Shadow>) -> bool {
    *value == default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_profile::Profile;

    #[test]
    fn omitted_shadow_null_and_custom_color_remain_distinct() -> anyhow::Result<()> {
        let mut document =
            serde_json::to_value(crate::battle_enemy::rules::tests::fixture()?.profile)?;
        assert!(
            serde_json::from_value::<Profile>(document.clone())?
                .shadow
                .is_none()
        );
        document.as_object_mut().unwrap().remove("shadow");
        let ordinary: Profile = serde_json::from_value(document.clone())?;
        assert_eq!(ordinary.shadow, Some(Shadow::default()));
        assert!(serde_json::to_value(ordinary)?.get("shadow").is_none());
        document["shadow"] = serde_json::json!({"color":[64,0,64,160],"scale":1.5});
        let edited: Profile = serde_json::from_value(document)?;
        assert_eq!(
            edited.shadow,
            Some(Shadow {
                color: [64, 0, 64, 160],
                scale: 1.5
            })
        );
        Ok(())
    }
}
