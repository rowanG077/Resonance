//! Billboard effects expressed as ordinary position, size, rotation and lifetime.
/// Per-character light colors and transitions.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterLight {
    pub position: LightPosition,
    pub strength: u8,
    pub shade: [u8; 3],
    pub bright: [u8; 3],
    pub color_step: u16,
    pub mode: u8,
}
#[derive(Debug, Clone, PartialEq)]
pub enum LightPosition {
    Relative([f32; 3]),
    World([f32; 3]),
    Actor { id: i32, height: f32 },
}
impl Default for CharacterLight {
    fn default() -> Self {
        Self {
            position: LightPosition::Relative([60000., -60000., 140.]),
            strength: 192,
            shade: [48; 3],
            bright: [64; 3],
            color_step: 4,
            mode: 0,
        }
    }
}
impl CharacterLight {
    pub(crate) fn set(&mut self, operation: i32, value: [i32; 3]) -> Result<(), String> {
        let color = || value.map(|v| (v / 4) as u8);
        match operation {
            0 => {
                self.bright = color();
                self.shade = self.bright.map(|v| (f32::from(v) * 0.75) as u8);
            }
            1 => self.mode = value[0] as u8,
            2 => self.strength = value[0] as u8,
            3 => self.position = LightPosition::Relative(value.map(|v| v as f32)),
            4 => self.position = LightPosition::World(value.map(|v| v as f32)),
            6 => {
                self.position = LightPosition::Actor {
                    id: value[0],
                    height: value[1] as f32,
                }
            }
            7 => self.bright = color(),
            8 => self.shade = color(),
            9 => self.color_step = value[0] as u16,
            _ => return Err("character light operation is not implemented".into()),
        }
        Ok(())
    }
    /// Approach each color channel by the configured step.
    pub fn approach(&mut self, target: &Self) {
        let step = i32::from(target.color_step);
        let approach = |current: [u8; 3], next: [u8; 3]| {
            std::array::from_fn(|i| {
                let delta = i32::from(next[i]) - i32::from(current[i]);
                (i32::from(current[i]) + delta.clamp(-step, step)) as u8
            })
        };
        let shade = approach(self.shade, target.shade);
        let bright = approach(self.bright, target.bright);
        *self = target.clone();
        self.shade = shade;
        self.bright = bright;
    }
}

#[derive(Debug, Clone)]
pub struct BillboardEffect {
    pub recipe: u16,
    pub born: u32,
    pub lifetime: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub rotation: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub size: [f32; 2],
    pub size_delta: f32,
    pub rgba: [u8; 4],
    pub alpha_delta: f32,
}
impl BillboardEffect {
    pub fn step(&mut self) {
        for i in 0..3 {
            self.position[i] += self.velocity[i];
            self.rotation[i] += self.angular_velocity[i];
        }
        for size in &mut self.size {
            *size += self.size_delta;
        }
    }
    pub fn alive(&self, tick: u32) -> bool {
        tick.saturating_sub(self.born) < self.lifetime
            && self.size.iter().all(|s| *s >= 0.)
            && self.alpha(tick) >= 0.
    }
    pub fn alpha(&self, tick: u32) -> f32 {
        f32::from(self.rgba[3]) + self.alpha_delta * tick.saturating_sub(self.born) as f32
    }
}
