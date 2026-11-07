//! Editable appearance controls, independent of the source release's storage.
use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentAnimation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<AttachmentTextureAnimation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent: Option<AttachmentTextureAnimation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_every_ticks: Option<NonZeroU16>,
}

impl AttachmentAnimation {
    pub fn is_empty(&self) -> bool {
        self.primary.is_none() && self.independent.is_none() && self.effect_every_ticks.is_none()
    }

    /// Shared rig layers inherit controls unless they only draw a constant-color outline.
    pub fn texture_for_layer(
        &self,
        scene: &crate::ScenePart,
        independent: bool,
    ) -> Option<&AttachmentTextureAnimation> {
        if independent {
            self.independent.as_ref()
        } else if scene.outline_color.is_some()
            && scene
                .materials
                .iter()
                .all(|material| scene.outline_color_for(material).is_some())
        {
            None
        } else {
            self.primary.as_ref()
        }
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    #[test]
    fn constant_color_outlines_skip_only_inherited_texture_controls() {
        let mut scene: crate::ScenePart = serde_json::from_value(serde_json::json!({
            "mesh":"outline.glb", "outline_color":[0,0,0,127],
            "materials":[{"meshes":[0]}]
        }))
        .unwrap();
        let scroll = AttachmentTextureAnimation::Scroll {
            texture: 0,
            step_milli_uv: [0, 20],
            offset_milli_uv: [0; 2],
        };
        let controls = AttachmentAnimation {
            primary: Some(scroll.clone()),
            independent: Some(scroll.clone()),
            ..Default::default()
        };
        assert!(controls.texture_for_layer(&scene, false).is_none());
        assert_eq!(controls.texture_for_layer(&scene, true), Some(&scroll));
        scene.outline_color = None;
        assert_eq!(controls.texture_for_layer(&scene, false), Some(&scroll));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachmentTextureAnimation {
    Scroll {
        #[serde(default, skip_serializing_if = "zero")]
        texture: u16,
        /// Thousandths of one texture repeat per simulation tick.
        step_milli_uv: [i16; 2],
        #[serde(default, skip_serializing_if = "origin")]
        offset_milli_uv: [i16; 2],
    },
    Atlas {
        #[serde(default, skip_serializing_if = "zero")]
        texture: u16,
        rows: u8,
        ticks_per_frame: NonZeroU16,
        /// When omitted, visit every row in order, starting at zero.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        frames: Vec<u8>,
    },
}

/// Prepared ribbon endpoints follow the explicit joint order in the document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trail {
    pub bones: Vec<u16>,
    pub material: TrailMaterial,
}

/// Blade ribbon appearance; history and visibility belong to combat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrailMaterial {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<TrailTexture>,
    #[serde(default, skip_serializing_if = "zero")]
    pub palette: u8,
    #[serde(default = "additive", skip_serializing_if = "is_additive")]
    pub additive: bool,
    /// Linear RGB multipliers; 1.0 is full intensity, values above 1.0 brighten.
    pub color: [f32; 3],
    /// Pixel rectangle [left, top, width, height] in the selected texture.
    #[serde(default, rename = "rectangle", skip_serializing_if = "empty_rectangle")]
    pub uv: [i16; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrailTexture {
    Common {
        #[serde(default, skip_serializing_if = "zero")]
        slot: u8,
    },
    Enemy {
        #[serde(default, skip_serializing_if = "zero")]
        slot: u8,
    },
}

fn zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
fn origin(value: &[i16; 2]) -> bool {
    *value == [0; 2]
}
fn empty_rectangle(value: &[i16; 4]) -> bool {
    *value == [0; 4]
}
fn additive() -> bool {
    true
}
fn is_additive(value: &bool) -> bool {
    *value
}
