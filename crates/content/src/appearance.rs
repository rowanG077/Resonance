//! Character texture selections tied to story content.
use std::collections::BTreeSet;

const COLETTE: u32 = 2;
const GENIS: u32 = 3;
const RAINE: u32 = 4;
const PRESEA: u32 = 7;
const CRUXIS_CRYSTAL_RECEIVED: u16 = 24;
pub const ANGEL_PROGRESS: u16 = 0x4c;

pub fn costume_frame(resource: u32, flags: &BTreeSet<u16>) -> u8 {
    match resource {
        COLETTE if flags.contains(&CRUXIS_CRYSTAL_RECEIVED) => 0,
        COLETTE | GENIS | RAINE => 3,
        PRESEA => 1,
        _ => 0,
    }
}

pub fn forced_face(resource: u32, angel_progress: i32) -> Option<u8> {
    const RED_EYES_FRAME: u8 = 15;
    (resource == COLETTE && (1000..2000).contains(&angel_progress)).then_some(RED_EYES_FRAME)
}

pub fn costume_resource(resource: u32, costume: u8) -> u32 {
    const COSTUME_RESOURCE_BASE: u32 = 0x2300_0000;
    if !(1..=9).contains(&resource) || costume == 0 {
        resource
    } else {
        COSTUME_RESOURCE_BASE | u32::from(costume) << 8 | resource
    }
}
