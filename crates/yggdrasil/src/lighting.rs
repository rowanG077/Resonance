use crate::drawing::BlendMode;
use crate::{
    data::{Assets, clamp, round, smooth},
    drawing::Drawing,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lighting {
    #[default]
    Auto,
    Day,
    Night,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LightState {
    pub night: f64,
    pub twilight: f64,
    pub cloud_cover: f64,
    pub orbit_phase: f64,
}
impl LightState {
    pub fn at(progress: f64, mode: Lighting) -> Self {
        const DUSK_START_GROWTH: f64 = 0.84;
        Self::dusk(
            (clamp(progress) - DUSK_START_GROWTH) / (1. - DUSK_START_GROWTH),
            mode,
        )
    }
    pub fn dusk(amount: f64, mode: Lighting) -> Self {
        let phase = 0.5 * smooth(amount);
        let elevation = (phase * std::f64::consts::TAU).cos();
        let night = match mode {
            Lighting::Night => 1.,
            Lighting::Day => 0.,
            Lighting::Auto => smooth((0.45 - elevation) / 1.15),
        };
        let twilight = if mode == Lighting::Auto {
            smooth((0.85 - elevation) / 0.55) * (1. - night)
        } else {
            0.
        };
        let cloud_cover = match mode {
            Lighting::Night => 0.,
            Lighting::Day => 1.,
            Lighting::Auto => 1. - smooth((-0.7 - elevation) / 0.14),
        };
        Self {
            night,
            twilight,
            cloud_cover,
            orbit_phase: match mode {
                Lighting::Night => 0.5,
                Lighting::Day => 0.,
                Lighting::Auto => phase,
            },
        }
    }
    pub(crate) fn tint(&self, c: &mut Drawing, strength: f64) {
        if self.twilight > 0. {
            c.fill(
                crate::SCENE_BOUNDS,
                "#d18b69",
                self.twilight * 0.2,
                BlendMode::SrcATop,
            );
        }
        if self.night > 0. {
            c.fill(
                crate::SCENE_BOUNDS,
                "#101d48",
                self.night * strength,
                BlendMode::SrcATop,
            );
        }
    }
    pub(crate) fn background(&self, c: &mut Drawing, assets: &Assets) {
        let d = &assets.data.lighting;
        if self.twilight > 0. {
            c.fill(
                crate::SCENE_BOUNDS,
                "#d18b69",
                self.twilight * 0.2,
                BlendMode::SrcOver,
            );
        }
        if self.night > 0. {
            c.image_rect(
                &assets.image(&d.backdrop),
                None,
                crate::SCENE_BOUNDS,
                self.night,
                BlendMode::SrcOver,
            );
        }
        let visible = smooth((self.night - 0.3) / 0.7);
        if visible > 0. {
            let mut stars = Drawing::new(crate::ENVIRONMENT_WIDTH, crate::ENVIRONMENT_HEIGHT);
            stars.image(&assets.image(&d.stars), 0., 0.);
            self.occlude(&mut stars, assets);
            c.image_rect(
                &stars.snapshot(),
                None,
                crate::SCENE_BOUNDS,
                visible,
                BlendMode::SrcOver,
            );
        }
        let mut moons = Drawing::new(crate::ENVIRONMENT_WIDTH, crate::ENVIRONMENT_HEIGHT);
        for moon in &d.moons {
            let o = &moon.orbit;
            let travel = (self.orbit_phase - o.rise) / o.duration;
            if !(0. ..=1.).contains(&travel) {
                continue;
            }
            let angle = travel * std::f64::consts::PI;
            let x = o.cx + o.rx * angle.cos();
            let y = o.base - o.height * angle.sin();
            let sprite = moon;
            moons.image(
                &assets.image(&sprite.image),
                round(x) - sprite.pad,
                round(y) - sprite.pad,
            );
        }
        if self.night < 1. {
            let mask = moons.snapshot();
            moons.image_rect(
                &assets.image(&d.day_air),
                None,
                crate::ENVIRONMENT_BOUNDS,
                1. - self.night,
                BlendMode::Screen,
            );
            moons.image_rect(&mask, None, crate::ENVIRONMENT_BOUNDS, 1., BlendMode::DstIn);
        }
        moons.image_rect(
            &assets.image(&d.sky),
            None,
            crate::ENVIRONMENT_BOUNDS,
            1.,
            BlendMode::DstIn,
        );
        self.occlude(&mut moons, assets);
        c.image_scaled(
            &moons.snapshot(),
            0.,
            0.,
            crate::WIDTH as f64,
            crate::HEIGHT as f64,
        );
    }
    fn occlude(&self, c: &mut Drawing, assets: &Assets) {
        if self.cloud_cover > 0. {
            c.image_rect(
                &assets.image(&assets.data.lighting.cloud_veil),
                None,
                crate::ENVIRONMENT_BOUNDS,
                self.cloud_cover,
                BlendMode::DstOut,
            );
        }
    }
}
