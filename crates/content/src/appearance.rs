//! Character texture selections tied to story content.
use std::collections::BTreeSet;

const LLOYD: u32 = 1;
const COLETTE: u32 = 2;
const GENIS: u32 = 3;
const RAINE: u32 = 4;
const PRESEA: u32 = 7;
const CRUXIS_CRYSTAL_RECEIVED: u16 = 24;
const CRUXIS_CRYSTAL_INACTIVE: u16 = 19;
const GENIS_EXSPHERE_RECEIVED: u16 = 25;
const RAINE_EXSPHERE_RECEIVED: u16 = 18;
const EXSPHERE_UNCOVERED: u16 = 29;
const PRESEA_COSTUME_CHANGE: u16 = 28;
pub const ANGEL_PROGRESS: u16 = 0x4c;

pub fn story_costume(resource: u32, angel_progress: i32, flags: &BTreeSet<u16>) -> u8 {
    match resource {
        COLETTE if angel_progress != 0 => 3,
        PRESEA if !flags.contains(&PRESEA_COSTUME_CHANGE) => 3,
        _ => 0,
    }
}

pub fn hidden_accessory(resource: u32, flags: &BTreeSet<u16>) -> Option<&'static str> {
    (resource == LLOYD && flags.contains(&EXSPHERE_UNCOVERED)).then_some("obj_nuno01")
}

pub fn costume_frame(resource: u32, flags: &BTreeSet<u16>) -> u8 {
    match resource {
        COLETTE if flags.contains(&CRUXIS_CRYSTAL_INACTIVE) => 2,
        COLETTE if flags.contains(&CRUXIS_CRYSTAL_RECEIVED) => 0,
        GENIS if flags.contains(&GENIS_EXSPHERE_RECEIVED) => 0,
        RAINE if flags.contains(&RAINE_EXSPHERE_RECEIVED) => 0,
        COLETTE | GENIS | RAINE => 3,
        PRESEA if flags.contains(&PRESEA_COSTUME_CHANGE) => 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crystal_changes_from_absent_to_active_to_inactive_and_back() {
        let mut flags = BTreeSet::new();
        assert_eq!(costume_frame(COLETTE, &flags), 3);
        flags.insert(CRUXIS_CRYSTAL_RECEIVED);
        assert_eq!(costume_frame(COLETTE, &flags), 0);
        flags.insert(CRUXIS_CRYSTAL_INACTIVE);
        assert_eq!(costume_frame(COLETTE, &flags), 2);
        assert_eq!(forced_face(COLETTE, 1000), Some(15));
        flags.remove(&CRUXIS_CRYSTAL_INACTIVE);
        assert_eq!(costume_frame(COLETTE, &flags), 0);
        assert_eq!(forced_face(COLETTE, 2000), None);
    }

    #[test]
    fn party_accessories_follow_their_own_story_flags() {
        for (character, flag, initial) in [
            (GENIS, GENIS_EXSPHERE_RECEIVED, 3),
            (RAINE, RAINE_EXSPHERE_RECEIVED, 3),
            (PRESEA, PRESEA_COSTUME_CHANGE, 1),
        ] {
            assert_eq!(costume_frame(character, &BTreeSet::new()), initial);
            assert_eq!(costume_frame(character, &[flag].into()), 0);
        }
        assert_eq!(hidden_accessory(LLOYD, &BTreeSet::new()), None);
        assert_eq!(
            hidden_accessory(LLOYD, &[EXSPHERE_UNCOVERED].into()),
            Some("obj_nuno01")
        );
    }
}
