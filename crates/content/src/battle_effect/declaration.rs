//! Cooked effect recipes. Binary controller layouts belong to the importer.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{
    ModelBinding, ParticleTemplate,
    visual::{ModelVisual, ParticleVisual},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Declaration {
    Particle {
        template: ParticleTemplate,
        visual: ParticleVisual,
    },
    ModelParticle {
        template: ParticleTemplate,
        binding: ModelBinding,
        visual: ModelVisual,
    },
    CameraShake {
        duration: u32,
        amplitude: u32,
    },
    /// Unsupported recipes remain loadable until a program requests them.
    Unsupported {
        reason: String,
    },
}

impl Declaration {
    pub fn template(&self) -> Result<&ParticleTemplate> {
        match self {
            Self::Particle { template, .. } | Self::ModelParticle { template, .. } => Ok(template),
            Self::Unsupported { reason } => bail!("{reason}"),
            Self::CameraShake { .. } => bail!("camera shake has no particle"),
        }
    }

    pub fn template_mut(&mut self) -> Result<&mut ParticleTemplate> {
        match self {
            Self::Particle { template, .. } | Self::ModelParticle { template, .. } => Ok(template),
            Self::Unsupported { reason } => bail!("{reason}"),
            Self::CameraShake { .. } => bail!("camera shake has no particle"),
        }
    }

    pub fn particle(&self) -> Result<ParticleTemplate> {
        let template = self.template()?;
        template.validate()?;
        Ok(template.clone())
    }

    pub fn particle_visual(&self) -> Result<&ParticleVisual> {
        match self {
            Self::Particle { visual, .. } => Ok(visual),
            Self::Unsupported { reason } => bail!("{reason}"),
            _ => bail!("effect has no sprite geometry"),
        }
    }

    pub fn model_visual(&self) -> Result<&ModelVisual> {
        match self {
            Self::ModelParticle { visual, .. } => Ok(visual),
            Self::Unsupported { reason } => bail!("{reason}"),
            _ => bail!("effect has no model"),
        }
    }
}
