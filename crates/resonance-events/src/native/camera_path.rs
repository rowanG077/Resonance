use super::{NativeHost, require};
use crate::camera::CameraRig;
impl NativeHost<'_> {
    pub(super) fn camera_path(&mut self, a: &[i32]) -> Result<(), String> {
        let rig = self
            .world
            .field_camera
            .get_or_insert_with(CameraRig::default);
        let path = rig
            .motion
            .as_mut()
            .ok_or("script camera path is not active")?;
        let xyz = [a[1] as f32, a[2] as f32, a[3] as f32];
        match a[0] {
            0 => {
                require((0..=1).contains(&a[1]), "invalid camera path mode")?;
                path.mode = a[1] as u8;
            }
            1 => self.registers[0] = i32::from(path.mode),
            3 => path.position.request(xyz, a[4])?,
            6 => self.registers[..3].copy_from_slice(&path.position.value.map(|v| v as i32)),
            7 => {
                require((100..17900).contains(&a[1]), "invalid path FOV")?;
                path.fov.request(a[1] as f32 / 100., a[2])?;
            }
            8 => self.registers[0] = (path.fov.value * 100.) as i32,
            9 => {
                path.actor = if a[1] == 0xF423F {
                    self.world.controlled_actor
                } else {
                    a[1]
                }
            }
            10 => self.registers[0] = path.actor,
            11 => path.offset.request(xyz, a[4])?,
            12 => self.registers[..3].copy_from_slice(&path.offset.value.map(|v| v as i32)),
            14 => path.angles.request(xyz.map(|v| v.rem_euclid(360.)), a[4])?,
            18 => self.registers[..3].copy_from_slice(&path.angles.value.map(|v| v as i32)),
            _ => return Err(format!("camera path command {} is not implemented", a[0])),
        }
        Ok(())
    }
}
