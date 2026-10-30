//! Expression channels keep their artwork, poses and blink settings together.
use super::Profile;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expression {
    pub texture: ExpressionTexture,
    /// Number of vertically stacked frames in the texture.
    pub frames: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub idle: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub active: u8,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub rescue: u8,
    /// Half-closed and closed frames; omission disables blinking on this channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blink: Option<[u8; 2]>,
    #[serde(
        default = "blink_exclude",
        skip_serializing_if = "ordinary_blink_exclude"
    )]
    pub blink_exclude: Vec<u8>,
}

fn blink_exclude() -> Vec<u8> {
    vec![2, 3, 10]
}
fn ordinary_blink_exclude(value: &[u8]) -> bool {
    value == [2, 3, 10]
}

/// Named costume roles or an explicit palette slot, shared across the body's parts.
/// Distinct slots may deliberately reference the same image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExpressionTexture {
    Role(FacialTexture),
    Slot(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacialTexture {
    Eyes,
    Mouth,
}

impl ExpressionTexture {
    pub fn resolve(self, primary: &crate::ScenePart) -> Result<usize> {
        let role = match self {
            Self::Role(role) => role,
            // A slot can belong to another body part (for example Living Doll's eyes).
            Self::Slot(index) => return Ok(index),
        };
        let appearance = primary
            .appearance
            .as_ref()
            .context("body has no facial texture roles")?;
        let texture = match role {
            FacialTexture::Eyes => appearance.eyes,
            FacialTexture::Mouth => appearance.mouth,
        }
        .context("body is missing the selected facial texture")?;
        ensure!(
            texture < primary.textures.len(),
            "facial texture is outside the body palette"
        );
        Ok(texture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_edits_keep_each_channels_frames_and_blink_exclusions() -> Result<()> {
        let mut profile = crate::battle_enemy::rules::tests::fixture()?.profile;
        profile.expressions = serde_json::from_str(
            r#"[
            {"texture":"eyes","frames":16,"idle":7,"active":4,"rescue":10,"blink":[4,5],"blink_exclude":[]},
            {"texture":2,"frames":8,"active":6,"rescue":5,"blink":[1,2]}
        ]"#,
        )?;
        assert_eq!(profile.expression_frames(|e| e.idle), [7, 0, 0, 0]);
        assert_eq!(profile.expression_frames(|e| e.active), [4, 6, 0, 0]);
        assert_eq!(profile.expression_frames(|e| e.rescue), [10, 5, 0, 0]);
        let blinks = profile.blinks()?;
        assert_eq!(blinks.len(), 2);
        assert_eq!((blinks[0].channel, blinks[0].frames), (0, [4, 5]));
        assert!(blinks[0].excluded_expressions.is_empty());
        assert_eq!((blinks[1].channel, blinks[1].frames), (1, [1, 2]));
        assert_eq!(blinks[1].excluded_expressions, [2, 3, 10]);
        let saved = serde_json::to_value(&profile.expressions)?;
        assert_eq!(saved[0]["blink_exclude"], serde_json::json!([]));
        assert!(saved[1].get("blink_exclude").is_none());
        assert!(saved[1].get("idle").is_none());
        let restored: Vec<Expression> = serde_json::from_value(saved)?;
        assert_eq!(restored, profile.expressions);
        assert!(serde_json::from_str::<Expression>(r#"{"texture":"eyse","frames":16}"#).is_err());
        Ok(())
    }
}

/// Prepared blink overlay; the channel is determined by the expression's position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blink {
    pub channel: u8,
    pub frames: [u8; 2],
    pub excluded_expressions: Vec<u8>,
}

impl Profile {
    pub fn expression_frames(&self, select: impl Fn(&Expression) -> u8) -> [u8; 4] {
        std::array::from_fn(|index| self.expressions.get(index).map_or(0, &select))
    }

    pub fn blinks(&self) -> Result<Vec<Blink>> {
        ensure!(self.expressions.len() <= 4, "too many expression channels");
        self.expressions
            .iter()
            .enumerate()
            .filter_map(|(channel, expression)| {
                expression.blink.map(|frames| {
                    ensure!(
                        frames.iter().all(|frame| *frame < expression.frames),
                        "blink frame is outside its expression atlas"
                    );
                    Ok(Blink {
                        channel: channel as u8,
                        frames,
                        excluded_expressions: expression.blink_exclude.clone(),
                    })
                })
            })
            .collect()
    }
}
