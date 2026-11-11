//! Shared field appearance and notices; scenes contain only local overrides.
use super::*;
use crate::{
    effect::{BlinkCycle, FieldEffects, FlutterRecipe},
    font::SystemText,
};

pub const PATH: &str = "fields/settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub effects: FieldEffects,
    pub blink: BlinkCycle,
    pub contact_shadow: ContactShadow,
    pub toon_ramp: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub particles: BTreeMap<i32, FlutterRecipe>,
    pub save_point: SavePointText,
    #[serde(default, skip_serializing_if = "EnvironmentDefaults::is_empty")]
    pub environment: EnvironmentDefaults,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavePointText {
    pub tutorial: SystemText,
    pub unlock: SystemText,
    pub no_gem: SystemText,
}

impl Settings {
    pub fn resolve_particles(
        &self,
        local: Option<BTreeMap<i32, FlutterRecipe>>,
    ) -> BTreeMap<i32, FlutterRecipe> {
        let mut particles = local.unwrap_or_else(|| self.particles.clone());
        for recipe in particles.values_mut() {
            if recipe.palette.is_empty() {
                recipe.palette.clone_from(&self.effects.palette);
            }
        }
        particles
    }

    pub fn from_field(effects: FieldEffects, field: &FieldAssets) -> Self {
        Self {
            effects,
            environment: EnvironmentDefaults::default(),
            blink: field.blink.clone(),
            contact_shadow: field.contact_shadow.clone(),
            toon_ramp: field.toon_ramp.clone(),
            particles: field.particles.clone(),
            save_point: SavePointText::from_field(field),
        }
    }
}

impl SavePointText {
    pub fn from_field(field: &FieldAssets) -> Self {
        Self {
            tutorial: SystemText(field.save_point_tutorial.clone()),
            unlock: SystemText(field.save_point_unlock.clone()),
            no_gem: SystemText(field.save_point_no_gem.clone()),
        }
    }
}

/// Minimal valid shared artwork for tests without an installed game library.
#[cfg(any(test, feature = "test-support"))]
pub fn test_fixture() -> Settings {
    use crate::effect::{self, RefractionRecipe, SpriteRecipe};
    let sprite: SpriteRecipe = SpriteRecipe {
        texture: "effects/default.ktx2".into(),
        uv: [0., 0., 1., 1.],
        frames: Vec::new(),
        repeat: false,
        additive: false,
    };
    Settings {
        effects: FieldEffects {
            emote_texture: sprite.texture.clone(),
            status_texture: sprite.texture.clone(),
            sprites: effect::sprite::all()
                .map(|id| (id, sprite.clone()))
                .collect(),
            palette: vec![[255; 4]; effect::FIELD_PALETTE_COLORS],
            refraction: RefractionRecipe {
                sprite: sprite.clone(),
                displacement: [0.; 2],
            },
            air_refraction: sprite,
            mouth_cycle: vec![0],
            rising_light_destination: [0.; 3],
        },
        blink: BlinkCycle { frames: vec![0] },
        contact_shadow: ContactShadow {
            texture: "effects/default.ktx2".into(),
            uv_size: [1.; 2],
            half_size: 1.,
            height_offset: 0.,
            alpha: 128,
            anchor_node: 0,
        },
        toon_ramp: "effects/default.ktx2".into(),
        particles: BTreeMap::new(),
        save_point: SavePointText::default(),
        environment: EnvironmentDefaults::default(),
    }
}
