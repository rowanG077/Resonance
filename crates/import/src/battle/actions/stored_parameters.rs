//! Parameters for stored elemental spells that retain one emission origin.
use super::*;
use resonance_content::battle::{
    actions::{
        earth_field::{EarthField, EarthFieldPulse, EarthFieldRecipe},
        lightning::GroundSpellOrigin,
    },
    effects::{EffectBank, EffectId},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub tidal_wave: water::Parameters,
    pub aqua_laser: water::Parameters,
    pub eruption: Ground,
    pub explosion: Ground,
    pub flame_lance: Ground,
    pub cyclone: wind_field::Parameters,
    pub air_blade: wind_field::Parameters,
    pub stalagmite: Ground,
    pub ground_dasher: Ground,
    pub grave: Ground,
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        Ok(Self {
            tidal_wave: water::read_parameters(rel, 202)?,
            aqua_laser: water::read_parameters(rel, 203)?,
            eruption: eruption::read_parameters(rel)?,
            explosion: explosion::read_parameters(rel)?,
            flame_lance: flame_lance::read_parameters(rel)?,
            cyclone: wind_field::read_parameters(rel, 210)?,
            air_blade: wind_field::read_parameters(rel, 211)?,
            stalagmite: stalagmite::read_parameters(rel)?,
            ground_dasher: earth_field::read_parameters(rel, 214)?,
            grave: earth_field::read_parameters(rel, 215)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Ground {
    pub lifetime: u16,
    pub origin: GroundSpellOrigin,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
    pub pulses: Vec<Pulse>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Pulse {
    pub tick: u16,
    pub projectile: u8,
    pub rule: usize,
}

pub(super) fn presentation(settings: &[u8]) -> Result<StoredSpellPresentation> {
    Ok(StoredSpellPresentation {
        color: settings
            .get(..4)
            .context("missing stored spell color")?
            .try_into()?,
        camera_distance: float(settings, 4)?,
        camera_elevation: float(settings, 8)?,
    })
}

impl Ground {
    pub fn single_pulse(&self) -> Result<&Pulse> {
        ensure!(self.pulses.len() == 1, "expected one stored spell pulse");
        Ok(&self.pulses[0])
    }
    pub fn read(
        rel: &Rel,
        settings: usize,
        lifetime: u16,
        pulses: &[(u16, u8, usize)],
    ) -> Result<Self> {
        let settings = rel.at((4, settings))?;
        Ok(Self {
            lifetime,
            origin: GroundSpellOrigin {
                height: float(rel.at((4, 0x1c80))?, 0)?,
                nudge: 1.,
                direction_threshold: float(rel.at((4, 0x2800))?, 0)?,
            },
            effect_scale: float(settings, 12)?,
            presentation: presentation(settings)?,
            pulses: pulses
                .iter()
                .map(|&(tick, projectile, rule)| Pulse {
                    tick,
                    projectile,
                    rule,
                })
                .collect(),
        })
    }

    pub fn earth(&self, kind: EarthField, bundle: &Bundle) -> Result<EarthFieldRecipe> {
        let recipe = EarthFieldRecipe {
            kind,
            lifetime: self.lifetime,
            effect_scale: self.effect_scale,
            presentation: self.presentation,
            target_height: self.origin.height,
            target_nudge_distance: self.origin.nudge,
            target_nudge_threshold: self.origin.direction_threshold,
            pulses: self
                .pulses
                .iter()
                .map(|pulse| {
                    Ok(EarthFieldPulse {
                        tick: pulse.tick,
                        projectile: kind.effect(pulse.projectile),
                        rule: bundle.phase_rule(0, pulse.rule)?,
                    })
                })
                .collect::<Result<_>>()?,
        };
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn fire(&self, native_id: u16, bundle: &Bundle) -> Result<FireFieldRecipe> {
        let bank = EffectBank::Magic(native_id - 200);
        let recipe = FireFieldRecipe {
            native_id,
            effect: EffectId { bank, id: 1 },
            lifetime: self.lifetime,
            effect_scale: self.effect_scale,
            presentation: self.presentation,
            target_height: self.origin.height,
            target_nudge_distance: self.origin.nudge,
            target_nudge_threshold: self.origin.direction_threshold,
            pulses: self
                .pulses
                .iter()
                .map(|pulse| {
                    Ok(FireFieldPulse {
                        tick: pulse.tick,
                        projectile: EffectId {
                            bank,
                            id: pulse.projectile,
                        },
                        rule: bundle.phase_rule(0, pulse.rule)?,
                    })
                })
                .collect::<Result<_>>()?,
        };
        recipe.validate()?;
        Ok(recipe)
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((module, _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    if module != "US_r_Top2Btl.rel" {
        return Ok(None);
    }
    crate::battle::embedded::write(
        file,
        output,
        "battle-stored-spell-parameters",
        &Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0x1238},"parameters_section":4}),
    )
    .map(Some)
}
