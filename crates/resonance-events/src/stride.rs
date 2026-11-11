pub const WALK_SPEED: f32 = 100. / 60.;
pub const RUN_SPEED: f32 = 7.75;
pub const MAXIMUM_WALK_SPEED: f32 = 4.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    Walk,
    Run,
}
impl Gait {
    pub fn at(speed: f32, scale: f32) -> Self {
        if speed.abs() > MAXIMUM_WALK_SPEED * scale {
            Self::Run
        } else {
            Self::Walk
        }
    }
    pub fn rate(self, speed: f32, scale: f32) -> f32 {
        let stride = match self {
            Self::Walk => WALK_SPEED,
            Self::Run => RUN_SPEED,
        };
        speed.abs() / (stride * scale)
    }
}
