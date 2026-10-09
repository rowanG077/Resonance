//! Script changes become visible after the next particle update.
use super::{BillboardEffect, Blend, Fade, RotationOrder, SpriteOrientation};
use resonance_content::effect::VerticalAnchor;

#[derive(Debug, Clone)]
pub(crate) enum Change {
    Sprite(Property),
    Model(crate::model_particle::Property),
}

#[derive(Debug, Clone)]
pub(crate) enum Property {
    Position(usize, f32),
    Velocity(usize, f32),
    Size(usize, f32),
    Color(usize, u8),
    Spin(usize, f32),
    Growth(f32),
    Speed(f32),
    NormalizeVelocity(bool),
    Rotation(usize, f32),
    RotationOrder(RotationOrder),
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
            129..=131 => Self::Velocity((property - 129) as usize, scaled),
            132..=134 => Self::Spin((property - 132) as usize, scaled),
            135 => Self::Growth(scaled),
            136 => Self::Speed(scaled),
            137 => Self::NormalizeVelocity(value & 1 != 0),
            138..=140 => Self::Velocity((property - 138) as usize, value as f32),
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
            149 => Self::RotationOrder(match value {
                0 => RotationOrder::Zyx,
                1 => RotationOrder::Zxy,
                2 => RotationOrder::Xyz,
                4 => RotationOrder::Xzy,
                8 => RotationOrder::Yxz,
                16 => RotationOrder::Yzx,
                _ => return Err("invalid particle rotation order".into()),
            }),
            _ => return Err(format!("unsupported effect property {property}: {value}")),
        })
    }

    fn apply(self, effect: &mut BillboardEffect, tick: u32) {
        match self {
            Self::Position(axis, value) => effect.position[axis] = value,
            Self::Velocity(axis, value) => effect.velocity[axis] = value,
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
            Self::Speed(value) => effect.speed = value,
            Self::NormalizeVelocity(value) => effect.normalize_velocity = value,
            Self::Rotation(axis, value) => effect.rotation[axis] = value,
            Self::RotationOrder(value) => effect.rotation_order = value,
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
            if !matches!(
                change,
                Property::Growth(_)
                    | Property::Rotation(_, _)
                    | Property::Spin(_, _)
                    | Property::Velocity(_, _)
                    | Property::Speed(_)
                    | Property::NormalizeVelocity(_)
                    | Property::RotationOrder(_)
            ) {
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
                } else if let Some(effect) = self.refractions.get_mut(&handle) {
                    match change {
                        Property::Growth(value) => effect.growth = value,
                        Property::Velocity(axis, value) => effect.velocity[axis] = value,
                        Property::Speed(value) => effect.speed = value,
                        Property::NormalizeVelocity(value) => effect.normalize_velocity = value,
                        Property::Rotation(axis, value) => effect.rotation[axis] = value,
                        Property::RotationOrder(value) => effect.rotation_order = value,
                        Property::Spin(axis, value) => effect.angular_velocity[axis] = value,
                        _ => unreachable!("validated refraction property"),
                    }
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
