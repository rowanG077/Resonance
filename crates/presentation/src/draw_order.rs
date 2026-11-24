//! Authored ordering for ordinary transparent scene meshes.
use bevy::prelude::*;

/// Native drawing phases. Asset material order is local to its scene or instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Layer {
    Background,
    Scene(u32),
    FieldOverlay(u32),
    Shadows,
    Actor(usize, ActorLayer),
    Foreground(u8),
    Effects,
    ModelEffects(u32),
    EffectOverlay,
    Overlay,
}

impl Layer {
    pub(super) fn field_actor(pass: i8) -> Self {
        use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
        match pass {
            -4 => Self::Scene(DrawStage::TranslucentScenery.offset() + 2 * MODEL_DRAW_SPAN),
            -3 => Self::ModelEffects(MODEL_DRAW_SPAN),
            -2 => Self::ModelEffects(2 * MODEL_DRAW_SPAN),
            -1 => Self::Scene(DrawStage::LateActors.offset()),
            0..=2 => Self::Scene(DrawStage::Actors.offset() + (2 - pass) as u32),
            _ => Self::Scene(DrawStage::Actors.offset() + 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ActorLayer {
    Behind,
    BackWeapon(u8, bool),
    Body,
    FrontWeapon(u8, bool),
    Front,
}

/// Layer, instance order, then material order within that instance.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DrawOrder(pub Layer, pub usize, pub u32);

impl DrawOrder {
    pub(super) fn field_model(layer: Layer, instance: usize, material: u32) -> Self {
        use resonance_content::field::MODEL_DRAW_SPAN;
        // Scenery and save points encode a scene-wide pass in the material order.
        let pass = material / MODEL_DRAW_SPAN * MODEL_DRAW_SPAN;
        let layer = match layer {
            Layer::Scene(0) => Layer::Scene(pass),
            Layer::FieldOverlay(0) => Layer::FieldOverlay(pass),
            // Actors finish their complete model in slot order within each pass.
            _ => return Self(layer, instance, material),
        };
        Self(layer, instance, material % MODEL_DRAW_SPAN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_actors_draw_in_pass_then_slot_order() {
        use resonance_content::field::{DrawStage, MODEL_DRAW_SPAN};
        let mut previous = None;
        for pass in [2, 1, 0, -1, -4, -3, -2] {
            let layer = Layer::field_actor(pass);
            let first_body = DrawOrder::field_model(layer, 0, 15);
            let first_outline = DrawOrder::field_model(layer, 0, MODEL_DRAW_SPAN);
            let second_body = DrawOrder::field_model(layer, 1, 0);
            assert!(previous.is_none_or(|last| last < first_body));
            assert!(first_body < first_outline && first_outline < second_body);
            previous = Some(second_body);
        }
        // Static scenery keeps its separate foreground pass.
        assert!(
            DrawOrder::field_model(Layer::Scene(0), 3, DrawStage::Foreground.offset() + 7)
                == DrawOrder(Layer::Scene(DrawStage::Foreground.offset()), 3, 7)
        );
    }
}
