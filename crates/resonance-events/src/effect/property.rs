//! Script changes become visible after the next particle update.
use super::{BillboardEffect, Blend, Fade, SpriteOrientation};
use resonance_content::effect::VerticalAnchor;

#[derive(Debug, Clone)]
pub(crate) enum Change {
    Sprite(Property),
    Model(crate::model_particle::Property),
}

#[derive(Debug, Clone)]
pub(crate) enum Property {
    Position(usize, f32),
    Size(usize, f32),
    Color(usize, u8),
    Spin(usize, f32),
    Growth(f32),
    Rotation(usize, f32),
    Orientation(SpriteOrientation),
    Blend(Option<Blend>),
    ProportionalFade,
    Anchor(VerticalAnchor),
    Fog(bool),
}

impl Property {
    fn decode(property: i32, value: i32) -> Result<Self, String> {
        let scaled = value as f32 / 100.;
        Ok(match property {
            120..=122 => Self::Position((property - 120) as usize, value as f32),
            123..=124 => Self::Size((property - 123) as usize, value as f32),
            125..=128 => Self::Color((property - 125) as usize, value as u8),
            132..=134 => Self::Spin((property - 132) as usize, scaled),
            135 => Self::Growth(scaled),
            141..=143 => Self::Rotation((property - 141) as usize, scaled),
            144 => Self::Orientation(if value & 1 == 0 {
                SpriteOrientation::World
            } else {
                SpriteOrientation::Camera
            }),
            145 => Self::Blend(if value & 3 == 3 {
                Some(Blend::Previous)
            } else {
                Some(value.try_into()?)
            }),
            146 if value == 8 => Self::ProportionalFade,
            147 => Self::Anchor(match value {
                0 => VerticalAnchor::Center,
                4 => VerticalAnchor::Bottom,
                8 => VerticalAnchor::Top,
                _ => return Err("unsupported particle quad layout".into()),
            }),
            148 => Self::Fog(value & 1 != 0),
            _ => return Err(format!("unsupported effect property {property}: {value}")),
        })
    }

    fn apply(self, effect: &mut BillboardEffect, tick: u32) {
        match self {
            Self::Position(axis, value) => effect.position[axis] = value,
            Self::Size(axis, value) => effect.size[axis] = value,
            Self::Color(3, value) if matches!(effect.fade, Fade::Linear(delta) if delta != 0.) => {
                effect.alpha_override = Some((effect.born.max(tick + 1), value));
            }
            Self::Color(3, value) => {
                if let Fade::Tail { after } = &mut effect.fade {
                    *after = (*after).max((tick + 1).saturating_sub(effect.born));
                }
                effect.rgba[3] = value;
            }
            Self::Color(channel, value) => effect.rgba[channel] = value,
            Self::Spin(axis, value) => effect.angular_velocity[axis] = value,
            Self::Growth(value) => effect.size_delta = value,
            Self::Rotation(axis, value) => effect.rotation[axis] = value,
            Self::Orientation(value) => effect.orientation = value,
            Self::Blend(value) => effect.blend = value,
            Self::ProportionalFade => {
                effect.rgba[3] = effect.alpha(tick + 1) as u8;
                effect.fade = Fade::Proportional;
            }
            Self::Anchor(value) => effect.anchor = value,
            Self::Fog(value) => effect.field_fog = value,
        }
    }
}

impl crate::GameWorld {
    pub(crate) fn set_effect_property(
        &mut self,
        handle: i32,
        property: i32,
        value: i32,
    ) -> Result<(), String> {
        let change = Property::decode(property, value)?;
        let born = if let Some(effect) = self.refractions.get(&handle) {
            if !matches!(change, Property::Growth(_)) {
                return Err("refraction property is not implemented".into());
            }
            effect.born
        } else if let Some(effect) = self.billboards.get(&handle) {
            effect.born
        } else {
            return Ok(());
        };
        self.queue_effect_change(handle, born, Change::Sprite(change));
        Ok(())
    }

    pub(crate) fn queue_effect_change(&mut self, handle: i32, born: u32, change: Change) {
        // Constructor settings apply before presentation; changes to an already
        // presented particle apply after its following motion update.
        if self.tick < born || handle > self.particles_before_update {
            self.apply_effect_change(handle, self.tick, change);
        } else {
            self.effect_changes.push((handle, self.tick, change));
        }
    }

    fn apply_effect_change(&mut self, handle: i32, tick: u32, change: Change) {
        match change {
            Change::Sprite(change) => {
                if let Some(effect) = self.billboards.get_mut(&handle) {
                    change.apply(effect, tick);
                } else if let Property::Growth(growth) = change
                    && let Some(effect) = self.refractions.get_mut(&handle)
                {
                    effect.growth = growth;
                }
            }
            Change::Model(change) => {
                if let Some(particle) = self.model_particles.get_mut(&handle) {
                    change.apply(particle);
                }
            }
        }
    }

    pub(crate) fn apply_effect_changes(&mut self) {
        for (handle, tick, change) in std::mem::take(&mut self.effect_changes) {
            self.apply_effect_change(handle, tick, change);
        }
    }
}
