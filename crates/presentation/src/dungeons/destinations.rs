use anyhow::Result;
use resonance_content::session::SessionData;
use resonance_events::{
    PersistentState,
    party::Party,
    ring::{ElectricOrbKind, SorcerersRing},
};
use resonance_game::field::FieldEntry;
use std::{collections::BTreeSet, sync::Arc};
use symphonia_script::Width;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fixture {
    Martel,
    FireSeal,
    WaterSeal,
    AirSeal,
    Mana,
    Iselia,
    Palmacosta,
    Asgard,
    GuardEntrance,
    Generator,
    Wings,
}

#[derive(Clone, Copy)]
pub(crate) struct Destination {
    pub name: &'static str,
    pub map: u32,
    position: [f32; 3],
    heading: f32,
    progress: Progress,
    flags: &'static [u16],
    variables: &'static [(u16, i32)],
    ring: SorcerersRing,
}

#[derive(Clone, Copy)]
enum Progress {
    MartelEntrance,
    Story(i32),
    AfterSalvation(i32),
    AfterFireSeal(Mission, i32),
    IseliaInfiltration(i32),
    Reunited(i32),
}

#[derive(Clone, Copy)]
#[repr(u16)]
enum Mission {
    Palmacosta = 0xB8,
    Thoda = 0xC4,
    Balacruf = 0xC8,
    Mana = 0xCC,
    Asgard = 0xE0,
}

const STORY: u16 = 0x40;
const ANGEL_PROGRESS: u16 = 0x4c;
const SAVED_PARTY: u16 = 0x150;
const SAVED_LEADER: u16 = 0x158;
const PARTY: [u8; 5] = [1, 2, 3, 4, 9];

impl Fixture {
    pub(super) const fn destination(self) -> Destination {
        use Progress::*;
        let (name, map, position, heading, progress) = match self {
            Self::Martel => (
                "TEMPLE OF MARTEL - START",
                307,
                [1., 194., 0.],
                180.,
                MartelEntrance,
            ),
            Self::FireSeal => (
                "TRIET RUINS - START",
                219,
                [-265., -4., 15.],
                272.,
                Story(1_302_000),
            ),
            Self::WaterSeal => (
                "THODA GEYSER - START",
                7,
                [21., 67., 0.],
                188.,
                AfterFireSeal(Mission::Thoda, 12_000),
            ),
            Self::AirSeal => (
                "BALACRUF MAUSOLEUM - START",
                508,
                [8., 222., 0.],
                180.,
                AfterFireSeal(Mission::Balacruf, 11_000),
            ),
            Self::Mana => (
                "TOWER OF MANA - START",
                362,
                [-9., -46., -3.],
                180.,
                AfterFireSeal(Mission::Mana, 1000),
            ),
            Self::Iselia => (
                "ISELIA HUMAN RANCH - START",
                194,
                [679., -3369., 0.],
                180.,
                IseliaInfiltration(20_303_000),
            ),
            Self::Palmacosta => (
                "PALMACOSTA HUMAN RANCH - START",
                201,
                [10., -498., 0.],
                180.,
                AfterFireSeal(Mission::Palmacosta, 1200),
            ),
            Self::Asgard => (
                "ASGARD HUMAN RANCH - START",
                213,
                [285., -218., 49.],
                270.,
                AfterFireSeal(Mission::Asgard, 3010),
            ),
            Self::GuardEntrance => (
                "SYLVARANT BASE - START",
                267,
                [-744., 441., -49.],
                0.,
                Story(1_101_000),
            ),
            Self::Generator => (
                "SYLVARANT BASE - GENERATOR WING",
                279,
                [-1084., 1472., 0.],
                90.,
                AfterSalvation(2_403_000),
            ),
            Self::Wings => (
                "COLETTE'S FIRST WINGS",
                221,
                [0., 0., 0.],
                0.,
                Story(1_302_000),
            ),
        };
        Destination::new(name, map, position, heading, progress)
    }
}

// Positions and progression values are field-content data. Each end checkpoint
// leaves the final encounter pending; these flags complete only its puzzle route.
const MARTEL_BLOCKS: &[u16] = &[206, 208, 209, 210, 211, 212];
const TRIET_PLATFORMS: &[u16] = &[230, 231, 232, 233, 234, 235, 236, 588, 589, 590];
const THODA_WATERWAYS: &[u16] = &[150, 151, 152, 153, 154, 155];
const BALACRUF_WIND: &[u16] = &[150, 151, 152, 153, 154, 155, 158, 333, 568, 569];
const MANA_BRIDGES: &[u16] = &[187];
const MANA_MIRRORS: &[(u16, i32)] = &[
    (0x104, -225),
    (0x108, 225),
    (0x10c, 225),
    (0x110, 2175),
    (0x114, -225),
    (0x118, 2175),
    (0x11c, 225),
    (0x120, 1425),
];
const ASGARD_BLOCKS: &[u16] = &[186, 190, 191, 192, 193, 194, 195];

pub(super) const DESTINATIONS: &[Destination] = &[
    Destination::new(
        "ISELIA",
        330,
        [25., -275., -46.],
        180.,
        Progress::Story(104_000),
    ),
    Destination::town("TRIET", 485, [-20., -814., 2.], 181.),
    Destination::town("IZOOLD", 353, [-653., -603., 1.], 166.),
    Destination::town("PALMACOSTA", 54, [-82., 18., 10.], 117.),
    Destination::town("ASGARD", 29, [-360., -424., 11.], 180.),
    Destination::town("LUIN", 447, [-31., -130., 10.], 360.),
    Destination::town("HIMA", 300, [-292., 48., 13.], 87.),
    Destination::town("KATZ VILLAGE", 419, [-22., -137., 0.], 180.),
    Destination::town("DIRK'S HOUSE", 372, [-540., -333., 0.], 126.),
    Destination::town("HAKONESIA PEAK", 123, [-227., 177., 27.], 180.),
    Destination::new(
        "ISELIA FOREST - START",
        192,
        [-1905., -4960., -2000.],
        187.,
        Progress::Story(104_000),
    ),
    Destination::new(
        "ISELIA FOREST - END",
        233,
        [3711., 3861., 700.],
        180.,
        Progress::Story(104_000),
    ),
    Fixture::Martel.destination(),
    Fixture::Martel
        .destination()
        .at(
            "TEMPLE OF MARTEL - BEFORE ALTAR",
            307,
            [0., 2525., 0.],
            180.,
        )
        .progress(Progress::Story(108_000))
        .solved(MARTEL_BLOCKS),
    Fixture::FireSeal.destination(),
    Fixture::FireSeal
        .destination()
        .at(
            "TRIET RUINS - BEFORE SEAL",
            220,
            [-2131., -354., 305.],
            180.,
        )
        .solved(TRIET_PLATFORMS),
    Destination::new(
        "OSSA TRAIL - START",
        347,
        [-750., -381., 15.],
        167.,
        Progress::Story(1_305_000),
    ),
    Destination::new(
        "OSSA TRAIL - END",
        350,
        [150., 400., -158.],
        0.,
        Progress::Story(1_402_000),
    ),
    Fixture::GuardEntrance.destination(),
    Fixture::GuardEntrance
        .destination()
        .at("SYLVARANT BASE - END", 275, [-1000., 475., 0.], 180.)
        .progress(Progress::Story(1_107_000))
        .ring(SorcerersRing::ElectricOrb(ElectricOrbKind::Sylvarant)),
    Fixture::WaterSeal.destination(),
    Fixture::WaterSeal
        .destination()
        .at("THODA GEYSER - BEFORE SEAL", 9, [-2555., -811., 0.], 180.)
        .progress(Progress::AfterFireSeal(Mission::Thoda, 13_000))
        .solved(THODA_WATERWAYS)
        .ring(SorcerersRing::Water),
    Fixture::Palmacosta.destination(),
    Fixture::Palmacosta
        .destination()
        .at("PALMACOSTA HUMAN RANCH - END", 206, [0., 500., 0.], 180.)
        .progress(Progress::AfterFireSeal(Mission::Palmacosta, 4100))
        .ring(SorcerersRing::Radar),
    Fixture::AirSeal.destination(),
    Fixture::AirSeal
        .destination()
        .at(
            "BALACRUF MAUSOLEUM - BEFORE SEAL",
            509,
            [11., 1530., 60.],
            180.,
        )
        .progress(Progress::AfterFireSeal(Mission::Balacruf, 12_000))
        .solved(BALACRUF_WIND),
    Fixture::Asgard
        .destination()
        .at("ASGARD HUMAN RANCH - START", 211, [-14., -344., 0.], 180.)
        .progress(Progress::AfterFireSeal(Mission::Asgard, 3000)),
    Fixture::Asgard
        .destination()
        .at("ASGARD HUMAN RANCH - END", 214, [0., 700., 0.], 180.)
        .progress(Progress::AfterFireSeal(Mission::Asgard, 5000))
        .solved(ASGARD_BLOCKS),
    Fixture::Mana.destination(),
    Fixture::Mana
        .destination()
        .at(
            "TOWER OF MANA - BEFORE SEAL",
            366,
            [-652., 2321., 913.],
            270.,
        )
        .progress(Progress::AfterFireSeal(Mission::Mana, 13_600))
        .solved(MANA_BRIDGES)
        .variables(MANA_MIRRORS),
    Destination::new(
        "TOWER OF SALVATION - START",
        147,
        [6., 36., 0.],
        180.,
        Progress::Story(2_206_000),
    ),
    Destination::new(
        "TOWER OF SALVATION - END",
        149,
        [0., 3900., 0.],
        180.,
        Progress::Story(2_302_000),
    ),
    Fixture::Iselia.destination(),
    Fixture::Iselia
        .destination()
        .at("ISELIA HUMAN RANCH - END", 196, [2065., 5651., -149.], 180.)
        .progress(Progress::IseliaInfiltration(20_305_000)),
    Destination::new(
        "REMOTE ISLAND RANCH - START",
        223,
        [1., 1453., -389.],
        180.,
        Progress::Reunited(13_401_000),
    ),
    Destination::new(
        "REMOTE ISLAND RANCH - END",
        227,
        [250., -2., 23360.],
        90.,
        Progress::Reunited(13_401_000),
    ),
    Destination::town("THODA DOCK", 82, [-194., -26., 0.], 100.),
    Fixture::Generator.destination(),
    Fixture::Wings.destination(),
];

impl Destination {
    const fn new(
        name: &'static str,
        map: u32,
        position: [f32; 3],
        heading: f32,
        progress: Progress,
    ) -> Self {
        Self {
            name,
            map,
            position,
            heading,
            progress,
            flags: &[],
            variables: &[],
            ring: SorcerersRing::Fire,
        }
    }

    const fn town(name: &'static str, map: u32, position: [f32; 3], heading: f32) -> Self {
        Self::new(name, map, position, heading, Progress::Story(4_000_000))
    }

    const fn at(self, name: &'static str, map: u32, position: [f32; 3], heading: f32) -> Self {
        Self {
            name,
            map,
            position,
            heading,
            ..self
        }
    }

    const fn progress(self, progress: Progress) -> Self {
        Self { progress, ..self }
    }

    const fn solved(self, flags: &'static [u16]) -> Self {
        Self { flags, ..self }
    }

    const fn variables(self, variables: &'static [(u16, i32)]) -> Self {
        Self { variables, ..self }
    }

    const fn ring(self, ring: SorcerersRing) -> Self {
        Self { ring, ..self }
    }

    pub(crate) fn entry(
        self,
        data: Arc<SessionData>,
        available_fields: BTreeSet<u32>,
    ) -> Result<FieldEntry> {
        let mut party = Party::new(&data, Default::default())?;
        party.formation = PARTY.to_vec();
        party.field_leader = 1;
        party.travel.saved_formation = party.formation.clone();
        party.travel.sorcerers_ring = self.ring;
        party.items.insert(resonance_events::ring::ITEM, 1);
        let mut persistent = PersistentState {
            party: Some(party),
            ..Default::default()
        };
        let story = match self.progress {
            Progress::MartelEntrance => {
                let party = persistent.party.as_mut().unwrap();
                party.formation = vec![1, 2, 3, 9];
                party.travel.saved_formation = party.formation.clone();
                party.travel.sorcerers_ring = resonance_events::ring::SorcerersRing::Disabled;
                party.items.remove(&resonance_events::ring::ITEM);
                104_000
            }
            Progress::Story(story) => story,
            Progress::Reunited(story) => {
                let party = persistent.party.as_mut().unwrap();
                party.formation = vec![1, 2, 3, 4, 5, 6, 7, 8];
                party.travel.saved_formation = party.formation.clone();
                persistent.memory.write(ANGEL_PROGRESS, Width::S32, 1000)?;
                story
            }
            Progress::AfterSalvation(story) => {
                persistent.memory.write(ANGEL_PROGRESS, Width::S32, 1000)?;
                story
            }
            Progress::AfterFireSeal(mission, value) => {
                // Later seals require Colette's first angel progression branch.
                persistent.memory.write(ANGEL_PROGRESS, Width::S32, 1)?;
                persistent.memory.write(mission as u16, Width::S32, value)?;
                if matches!(mission, Mission::Asgard) {
                    // Asgard's scenes rebuild both three-person groups from
                    // these per-member bits.
                    let party = persistent.party.as_mut().unwrap();
                    party.formation = vec![1, 2, 9, 4, 3, 5];
                    party.travel.saved_formation = party.formation.clone();
                    split_party(&mut persistent, 3);
                }
                if matches!(mission, Mission::Palmacosta) {
                    // Field 198's post-Magnius evacuation and destruction
                    // dispatch requires this story stage as well as mission B8.
                    2_002_000
                } else {
                    4_000_000
                }
            }
            Progress::IseliaInfiltration(story) => {
                let party = persistent.party.as_mut().unwrap();
                party.formation = vec![1, 2, 3, 9, 4, 6, 7, 8];
                party.travel.saved_formation = party.formation.clone();
                for (slot, id) in party.formation.iter().copied().enumerate() {
                    persistent
                        .memory
                        .write(SAVED_PARTY + slot as u16, Width::S8, i32::from(id))?;
                }
                persistent.memory.write(SAVED_LEADER, Width::S8, 1)?;
                split_party(&mut persistent, 4);
                story
            }
        };
        persistent.memory.write(STORY, Width::S32, story)?;
        persistent.event_flags.extend(self.flags);
        for &(address, value) in self.variables {
            persistent.memory.write(address, Width::S32, value)?;
        }
        Ok(FieldEntry {
            persistent,
            data: Some(data),
            available_fields,
            position: self.position,
            heading: self.heading,
            ..Default::default()
        })
    }
}

/// Each character stores a group bit and a two-bit position within that group.
fn split_party(state: &mut PersistentState, group_size: usize) {
    const GROUP_FLAGS: u16 = 150;
    for (index, &id) in state.party.as_ref().unwrap().formation.iter().enumerate() {
        let slot = index % group_size;
        let base = GROUP_FLAGS + u16::from(id) * 3;
        for (offset, set) in [index >= group_size, slot & 2 != 0, slot & 1 != 0]
            .into_iter()
            .enumerate()
        {
            if set {
                state.event_flags.insert(base + offset as u16);
            }
        }
    }
}
