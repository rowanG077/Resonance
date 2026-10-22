//! Bind automatic field atmosphere to shared artwork and live rendering settings.
use anyhow::{Context, Result};
use resonance_content::{effect::SpriteRecipe, field::*};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub(crate) const TEXTURES_PATH: &str = "embedded/field-environment-textures.json";
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Textures {
    pub dust: String,
    pub particles: String,
    pub haze: String,
}
impl Textures {
    pub(crate) fn cook(
        extracted: &Path,
        output: &Path,
        particles: String,
        haze: String,
    ) -> Result<Self> {
        let files = extracted.join("files");
        let path = crate::all_assets::roles::declared_path(&files, "sand.tpl")?;
        let dust = crate::texture::decode_source(&fs::read(files.join(path))?)?
            .write(output)?
            .textures
            .into_iter()
            .next()
            .flatten()
            .context("missing dust texture")?;
        let textures = Self {
            dust: dust.image(0)?.path,
            particles,
            haze,
        };
        crate::write_atomic(&output.join(TEXTURES_PATH), &serde_json::to_vec(&textures)?)?;
        Ok(textures)
    }
    pub(crate) fn files(&self) -> impl Iterator<Item = &str> {
        [TEXTURES_PATH, &self.dust, &self.particles, &self.haze].into_iter()
    }
}

pub(crate) fn profile(map: u32, textures: &Textures) -> FieldEnvironment {
    let mut environment = FieldEnvironment::default();
    let dust = |gate,
                interval,
                lifetime,
                position,
                position_step,
                position_steps,
                velocity,
                half_size,
                alpha| AmbientParticles {
        gate,
        sprite: sprite(&textures.dust, [0., 0., 255. / 256., 255. / 256.], false),
        emission: AmbientEmission::Drifting {
            interval,
            lifetime,
            position,
            position_step,
            position_steps,
            velocity,
            half_size,
            color: [63, 51, 31, alpha],
        },
    };
    let particle = match map {
        218 => Some(dust(
            EnvironmentGate::Always,
            32,
            120,
            [-500., -800., 264.],
            [-42., 0., 0.],
            [32, 1, 1],
            [1., 16., 0.],
            [1056., 256.],
            32,
        )),
        281 => Some(dust(
            EnvironmentGate::SettingNonzero(7),
            8,
            120,
            [1024., -2184., 253.],
            [-124., 0., 32.],
            [8, 1, 8],
            [0., 32., 0.],
            [1024., 448.],
            128,
        )),
        485 => Some(dust(
            EnvironmentGate::Always,
            16,
            240,
            [639., -552., 264.],
            [-124., 0., 0.],
            [32, 1, 1],
            [0., 8., 0.],
            [1024., 256.],
            64,
        )),
        486 => Some(dust(
            EnvironmentGate::RenderingEnabled,
            16,
            360,
            [0., -361., 264.],
            [-124., 0., 0.],
            [32, 1, 1],
            [2., 8., 0.],
            [1024., 256.],
            64,
        )),
        _ => None,
    };
    environment.particles.extend(particle);
    if map == 322 {
        for (x, size) in [(-183.884, 40.), (-193.884, 30.), (-173.884, 20.)] {
            environment.particles.push(AmbientParticles {
                gate: EnvironmentGate::Always,
                sprite: sprite(&textures.particles, [0., 0.25, 0.25, 0.5], true),
                emission: AmbientEmission::Fire {
                    position: [x, 309.894, 38.991],
                    size,
                },
            });
        }
    }
    if (310..=313).contains(&map) {
        environment.particles.push(AmbientParticles {
            gate: EnvironmentGate::Always,
            sprite: sprite(
                &textures.particles,
                [0., 192. / 256., 15. / 256., 207. / 256.],
                false,
            ),
            emission: AmbientEmission::Snow {
                count: 4096,
                spread: 1023,
                height: 800.,
                fall_speed: [3, 18],
                half_size: 6.,
                color: [240, 240, 255, 112],
                fog_color: [0, 0, 31, 16],
                fog_range: [100., 10_000.],
            },
        });
    }
    if matches!(map, 218 | 220 | 281 | 485 | 486) {
        environment.heat_haze = Some(HeatHaze {
            gate: if map == 281 {
                EnvironmentGate::SettingNonzero(7)
            } else {
                EnvironmentGate::Always
            },
            sprite: sprite(&textures.haze, [0., 0., 1., 1.], false),
            scroll_per_tick: -1. / 1600.,
            rotation_degrees: 95.,
            scale: 0.8,
            alpha: 192,
        });
    }
    environment.wind = match map {
        29..=31 => Some(FieldWind {
            gate: EnvironmentGate::Always,
            base: [-0.8, 0., -0.8],
            step: [-1. / 30., 0., -1. / 30.],
            steps: 32,
        }),
        281 => Some(FieldWind {
            gate: EnvironmentGate::SettingNonzero(7),
            base: [0., 1.8, -0.8],
            step: [0., 1. / 15., -1. / 30.],
            steps: 32,
        }),
        _ => None,
    };
    if matches!(map, 414 | 478) {
        environment.lights = Some(SceneryLighting {
            gate: EnvironmentGate::ActorExists(RenderValue::Setting(7)),
            target: RenderValue::Fixed(scenery_actor(0)),
            ambient: [50, 60, 70],
            color: [255, 159, 159],
            attenuation: [1., 0.002, 0.],
            lights: if map == 414 {
                vec![SceneryLight {
                    actor: RenderValue::Setting(7),
                    height: 200.,
                    intensity: 3.,
                }]
            } else {
                vec![
                    SceneryLight {
                        actor: RenderValue::Setting(6),
                        height: 200.,
                        intensity: 1.,
                    },
                    SceneryLight {
                        actor: RenderValue::Setting(7),
                        height: 200.,
                        intensity: 2.,
                    },
                ]
            },
        });
    }
    environment
}
fn sprite(texture: &str, uv: [f32; 4], additive: bool) -> SpriteRecipe {
    SpriteRecipe {
        texture: texture.into(),
        uv,
        additive,
        frames: Vec::new(),
        repeat: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_profiles_bind_only_live_field_controls_and_shared_images() {
        let textures = Textures {
            dust: "textures/dust.png".into(),
            particles: "textures/effects.png".into(),
            haze: "textures/haze.png".into(),
        };
        let populated: Vec<_> = (0..547)
            .filter(|&id| !profile(id, &textures).is_empty())
            .collect();
        assert_eq!(
            populated,
            [
                29, 30, 31, 218, 220, 281, 310, 311, 312, 313, 322, 414, 478, 485, 486
            ]
        );
        for id in populated {
            profile(id, &textures).validate().unwrap();
        }
        let mut settings = std::collections::BTreeMap::new();
        let base = profile(281, &textures);
        assert!(
            !base
                .wind
                .as_ref()
                .unwrap()
                .gate
                .active(true, &settings, |_| false)
        );
        settings.insert(7, 1);
        assert!(
            base.wind
                .as_ref()
                .unwrap()
                .gate
                .active(false, &settings, |_| false)
        );
        let mine = profile(414, &textures);
        assert!(
            !mine
                .lights
                .as_ref()
                .unwrap()
                .gate
                .active(true, &settings, |_| false)
        );
        assert!(
            mine.lights
                .as_ref()
                .unwrap()
                .gate
                .active(false, &settings, |id| id == 1)
        );
        assert_eq!(mine.lights.as_ref().unwrap().lights[0].intensity, 3.);
    }
}
