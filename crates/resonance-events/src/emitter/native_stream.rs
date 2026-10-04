//! Continuous preset decoding and mutable scenario properties.
use super::{particle, stream::Settings};
use crate::effect::{Fade, SpriteOrientation};

impl Settings {
    pub(super) fn decode(recipe: i32, v: [i32; 10]) -> Result<Self, String> {
        let mut s = Settings::default();
        use crate::effect::{CAMERA_DISC_SPRITE, GLOW_SPRITE};
        use resonance_content::effect::{STREAK_SPRITE, VerticalAnchor};
        s.sprite.field_lighting = true;
        let alpha = |value: i32| value.clamp(0, 255) as u8;
        let count =
            |value: i32| u32::try_from(value).map_err(|_| "negative emitter count".to_owned());
        let duration = |value: i32| count(value).map(|v| v.max(1));
        match recipe {
            0..=3 => {
                let flame = recipe == 0;
                s.palette = None;
                s.images = &[GLOW_SPRITE];
                s.interval = if flame { 4 } else { 8 };
                s.count = if flame { 2 } else { 1 };
                s.size_variation = 15.;
                s.sprite.velocity[2] = 2.;
                s.speed_variation = 1.;
                s.sprite.size_delta = if flame { -1. } else { 1. };
                s.sprite.rgba = if flame {
                    [255, 150, 10, 255]
                } else {
                    [255, 255, 255, 127]
                };
                s.sprite.blend_mode = Some(if flame { 1 } else { 0 });

                s.sprite.lifetime = v[0].max(60) as u32;
                s.sprite.size = [s.sprite.lifetime as f32; 2];
                s.sprite.fade = Fade::tail(s.sprite.lifetime);
            }
            9 | 75 => {
                s.filled = recipe == 75;
                s.speed_variation = 5.;
                s.sprite.gravity = -0.98;
                s.sprite.fade = Fade::Linear(0.);
                s.images = &[GLOW_SPRITE];
                s.inherit_appearance = true;

                s.palette = Some(v[0]);
                s.radius = [v[1] as f32; 2];
                s.sprite.lifetime = duration(v[2])?;
                s.interval = count(v[3])?;
                if v[4] <= 0 {
                    return Err("splash angle must be positive".into());
                }
                s.count = (360. / v[4] as f32).ceil() as u32;
                s.sprite.size = [v[5] as f32; 2];
                s.sprite.rgba[3] = alpha(v[6]);
                s.sprite.velocity[2] = v[7] as f32;
                s.radial_speed = v[8] as f32;
                s.size_variation = v[9] as f32;
            }
            15 | 30 | 54 => {
                s.speed_scale = 0.01;
                s.sprite.angular_velocity[2] = 2.;
                s.owned = recipe != 30;
                s.inherit_appearance = recipe == 30;
                s.filled = recipe == 30;
                s.images = match recipe {
                    30 => &[10, 12, 69],
                    54 => &[10, 7, 68],
                    _ => &[10],
                };
                s.sprite.fade = Fade::Linear(-1.);

                s.palette = Some(v[0]);
                s.radius = [v[1] as f32; 2];
                s.sprite.size = [v[2] as f32; 2];
                s.size_variation = v[3] as f32;
                s.sprite.field_lighting = v[4] & 1 != 0;
                s.speed_variation = v[5] as f32 / 100.;
                s.interval = count(v[8])?;
                s.preserve_particles = recipe == 15 && v[9] != 1;
                if recipe == 54 {
                    s.sprite.rgba[3] = alpha(v[5]);
                    s.sprite.lifetime = duration(v[7])?;
                    s.sprite.fade = if v[6] == 0 {
                        Fade::Linear(0.)
                    } else {
                        Fade::tail(s.sprite.lifetime)
                    };
                }
            }
            33 => {
                s.sprite.lifetime = 60;
                s.count = 2;
                s.orbit_speed_scale = 0.01;

                s.palette = Some(v[0]);
                s.radius = [v[1] as f32; 2];
                s.sprite.size = [v[2] as f32; 2];
                s.sprite.rgba[3] = alpha(v[3] / 16);
                s.sprite.fade = Fade::Linear(v[4] as f32 / 16.);
            }
            36 => {
                s.sprite.rgba[3] = 150;
                s.sprite.angular_velocity[2] = 3.;
                s.speed_scale = 0.01;
                s.images = &[10, 12, 69];
                s.interval = 3;

                s.palette = Some(v[0]);
                s.radius = [v[1] as f32; 2];
                s.sprite.size = [v[2] as f32; 2];
                s.sprite.lifetime = duration(v[3])?;
                s.sprite.fade = Fade::tail(s.sprite.lifetime);
                s.target = Some([v[4] as f32, v[5] as f32, v[6] as f32]);
                s.size_variation = v[7] as f32;
            }
            55 => {
                s.inherit_appearance = true;

                s.palette = Some(v[0]);
                s.camera_offset = Some(v[1] as f32);
                s.sprite.lifetime = duration(v[2])?;
                s.sprite.size = [v[3] as f32; 2];
                s.sprite.rgba[3] = alpha(v[4]);
                s.sprite.fade = if v[5] == 0 {
                    Fade::Linear(0.)
                } else {
                    Fade::tail(s.sprite.lifetime)
                };
                s.sprite.size_delta = v[6] as f32;
                s.interval = count(v[7])?;
                s.sprite.orientation = match v[8] {
                    0 => SpriteOrientation::Camera,
                    1 => SpriteOrientation::World,
                    _ => return Err("invalid sprite orientation".into()),
                };
            }
            16 => {
                s.count = 72;
                s.limit = Some(s.count);
                s.images = &[STREAK_SPRITE];
                s.sprite.size = [10., 120.];
                s.sprite.rgba[3] = 100;
                s.sprite.velocity[2] = 8.;
                s.sprite.lifetime = 90;

                s.palette = Some(v[0]);
                s.radius = [v[1] as f32; 2];
                s.radial_speed = v[2] as f32 / 10.;
            }
            17 => {
                s.palette = Some(33);
                s.images = &[STREAK_SPRITE];
                s.radius = [200.; 2];
                s.sprite.position[2] = 1000.;
                s.sprite.velocity[2] = -30.;
                s.sprite.size = [15., 250.];
                s.sprite.lifetime = 60;
            }
            24 => {
                s.palette = Some(33);
                s.filled = true;
                s.sprite.rgba = [48, 48, 48, 255];
                s.sprite.size_delta = 1.;
                s.speed_scale = 0.01;
                s.radial_speed_scale = 0.005;

                s.limit = Some(count(v[0])?);
                s.count = v[1].max(1) as u32;
                s.radius = [v[2] as f32, v[3] as f32];
                s.sprite.size = [v[5] as f32; 2];
                s.size_variation = v[6] as f32;
                s.sprite.rgba[3] = alpha(v[7]);
                s.sprite.fade = Fade::Linear(v[9] as f32);
            }
            26 => {
                s.images = &[STREAK_SPRITE];
                s.sprite.orientation = SpriteOrientation::World;
                s.sprite.anchor = VerticalAnchor::Top;
                s.sprite.rotation = [90., 0., 0.];
                s.sprite.fade = Fade::RiseFall {
                    rise_ticks: 40,
                    step: 1.25,
                };
                s.sprite.lifetime = 80;

                s.palette = Some(v[0]);
                s.interval = count(v[1])?;
                s.radius = [v[2] as f32; 2];
                s.sprite.size = [v[3] as f32, v[5] as f32];
                s.size_variation = v[4] as f32;
                s.count = v[8].max(1) as u32;
            }
            27 | 28 => {
                let mut glow = particle([0.; 3], 0, 0, 180);
                glow.recipe = CAMERA_DISC_SPRITE;
                glow.size = [1.; 2];
                glow.size_delta = 3.;
                glow.rgba[3] = 100;
                glow.field_lighting = true;
                glow.fade = Fade::Proportional {
                    after: 0,
                    lifetime: glow.lifetime,
                };
                s.sprite.size = [15.; 2];
                s.sprite.lifetime = 60;
                s.sprite.fade = Fade::Proportional {
                    after: 0,
                    lifetime: 60,
                };
                s.palette = Some(v[0]);
                if recipe == 27 {
                    s.count = 100;
                    s.limit = Some(s.count);
                    glow.size = [v[1] as f32; 2];
                    glow.size_delta = v[2] as f32;
                    s.radius = [v[3] as f32; 2];
                    s.radial_speed = -s.radius[0] / s.sprite.lifetime as f32;
                } else {
                    s.radius = [100.; 2];
                    s.radial_speed = 2.;
                    s.interval = 4;
                    s.camera_offset = Some(v[1] as f32);
                }
                s.flash = Some(glow);
            }
            _ => return Err("unsupported particle preset".into()),
        }
        if s.palette.is_some_and(|color| !(0..=108).contains(&color))
            || s.interval == 0
            || s.radius.iter().any(|r| *r < 0.)
            || s.size_variation < 0.
            || s.speed_variation < 0.
            || s.count > crate::effect::BILLBOARD_LIMIT as u32
        {
            return Err("invalid emitter palette, interval or spread".into());
        }
        Ok(s)
    }
}
