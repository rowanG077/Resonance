//! A finite, vertically staggered burst of grey smoke.
use crate::effect::{BILLBOARD_LIMIT, BillboardEffect, Fade};

parameters! { count = 113, batch_size = 114, spread_x = 115, spread_y = 116, spacing = 117, size = 118, size_variation = 119, alpha = 120, alpha_variation = 121, fade = 122 }

#[derive(Debug, Clone)]
pub(crate) struct Smoke {
    parameters: Parameters,
    emitted: f32,
}
impl Smoke {
    pub(super) fn from_native(a: &[i32]) -> Result<Self, String> {
        let result = Self {
            parameters: Parameters::read(a),
            emitted: 0.,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<(), String> {
        let p = self.parameters;
        if p.batch_size > BILLBOARD_LIMIT as i32
            || [p.spread_x, p.spread_y, p.size_variation, p.alpha_variation]
                .iter()
                .any(|v| *v <= 0)
        {
            return Err("invalid smoke burst count or random range".into());
        }
        Ok(())
    }
    pub(super) fn property(&mut self, property: i32, value: Option<i32>) -> Result<i32, String> {
        let previous = self.parameters.property(property, value)?;
        if value.is_some() {
            self.validate()?;
        }
        Ok(previous)
    }
    pub(super) fn particles(
        &mut self,
        center: [f32; 3],
        speed: f32,
        born: u32,
        random: &mut u32,
        out: &mut Vec<BillboardEffect>,
    ) {
        let p = self.parameters;
        let mut draw = |range| (crate::world::random(random) % range as u32) as i32;
        for _ in 0..p.batch_size {
            if self.emitted >= p.count as f32 {
                break;
            }
            let shade = ((175 - draw(100)) / 3) as u8;
            let mut smoke = super::particle(center, born, 33, 300);
            smoke.position[0] += (p.spread_x - draw(2 * p.spread_x)) as f32;
            smoke.position[1] += (p.spread_y - draw(2 * p.spread_y)) as f32;
            smoke.position[2] += self.emitted * p.spacing as f32;
            smoke.size_delta = (draw(2) + 1) as f32;
            smoke.rgba = [
                shade,
                shade,
                shade,
                (p.alpha + draw(p.alpha_variation)) as u8,
            ];
            smoke.fade = Fade::Linear(p.fade as f32);
            smoke.size = [(p.size + draw(p.size_variation)) as f32; 2];
            let direction = [
                (50 - draw(100)) as f32,
                (50 - draw(100)) as f32,
                draw(100) as f32,
            ];
            let length = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
            smoke.velocity = direction.map(|v| {
                if length == 0. {
                    0.
                } else {
                    v / length * speed / 100.
                }
            });
            // The native particle update runs once before its first draw.
            smoke.step();
            out.push(smoke);
            self.emitted += 1.;
        }
    }
}
