//! Battle-event presentation state shared by the ordinary scenario host.
pub struct State {
    pub camera: crate::camera::stage::Camera,
    pub fade: Fade,
    /// The rescue admits the fourth, already initialized battle participant.
    pub activate_reserve: bool,
}

impl State {
    pub fn new(vertical_fov_degrees: f32) -> Result<Self, String> {
        Ok(Self {
            camera: crate::camera::stage::Camera::new(vertical_fov_degrees)?,
            fade: Fade::default(),
            activate_reserve: false,
        })
    }

    pub(crate) fn step(&mut self) {
        self.camera.step();
        self.fade.step();
    }
}

/// An independent fullscreen cover; the ordinary field fade retains its owner.
#[derive(Default)]
pub struct Fade {
    pub color: [u8; 3],
    pub opacity: u8,
    step: i16,
}

impl Fade {
    pub(crate) fn command(&mut self, arguments: [i32; 7]) -> Result<(), String> {
        let [mode @ (1 | 2), shade @ (0 | 1), rate @ 1..=255, 0, 0, 0, 0] = arguments else {
            return Err(format!("unsupported battle event fade {arguments:?}"));
        };
        let color = if shade == 0 { [255; 3] } else { [0; 3] };
        if self.step != 0
            || (mode == 1 && (self.opacity != 255 || self.color != color))
            || (mode == 2 && self.opacity != 0)
        {
            return Err("battle event fade interruption is not prepared".into());
        }
        self.color = color;
        self.step = if mode == 1 {
            -(rate as i16)
        } else {
            rate as i16
        };
        Ok(())
    }

    pub(crate) fn step(&mut self) {
        self.opacity = (i16::from(self.opacity) + self.step).clamp(0, 255) as u8;
        if matches!(self.opacity, 0 | 255) {
            self.step = 0;
        }
    }
}
