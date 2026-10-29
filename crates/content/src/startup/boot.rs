//! Ordered, editable startup logos. Timing uses presentation updates.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sequence(pub Vec<Logo>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    pub image: String,
    /// Start fading out at this update, measured from this logo's entry.
    pub fade_out_at: u32,
    #[serde(default = "fade_ticks", skip_serializing_if = "is_default_fade")]
    pub fade_ticks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_after: Option<u32>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub background: [u8; 3],
}

fn fade_ticks() -> u32 {
    64
}
fn is_default_fade(value: &u32) -> bool {
    *value == fade_ticks()
}

impl Sequence {
    /// Validate authoring and return the duration without skips.
    pub fn duration(&self) -> Result<u32> {
        let mut total = 0u32;
        for logo in &self.0 {
            crate::library::validate_path(&logo.image)?;
            ensure!(
                logo.fade_ticks > 0 && logo.fade_out_at > logo.fade_ticks,
                "startup fade_out_at must follow a positive fade_ticks interval"
            );
            if let Some(skip) = logo.skip_after {
                ensure!(
                    skip > logo.fade_ticks && skip <= logo.fade_out_at,
                    "startup skip_after must follow fade-in and precede fade-out"
                );
            }
            total = total
                .checked_add(logo.fade_out_at)
                .and_then(|value| value.checked_add(logo.fade_ticks))
                .ok_or_else(|| anyhow::anyhow!("startup sequence duration exceeds its clock"))?;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests;
