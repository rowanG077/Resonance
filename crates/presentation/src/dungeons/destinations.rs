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
    ForestArrival,
    Story(i32),
    AfterSalvation(i32),
    Pilgrimage(Mission, i32),
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

pub(super) const MARTEL_START: Destination = Destination::new(
    "TEMPLE OF MARTEL - START",
    307,
    [1., 194., 0.],
    180.,
    Progress::MartelEntrance,
);

pub(super) const TRIET_START: Destination = Destination::new(
    "TRIET RUINS - START",
    219,
    [-265., -4., 15.],
    272.,
    Progress::Story(1_302_000),
);

pub(super) const THODA_START: Destination = Destination::new(
    "THODA GEYSER - START",
    7,
    [21., 67., 0.],
    188.,
    Progress::Pilgrimage(Mission::Thoda, 12_000),
);

pub(super) const BALACRUF_START: Destination = Destination::new(
    "BALACRUF MAUSOLEUM - START",
    508,
    [8., 222., 0.],
    180.,
    Progress::Pilgrimage(Mission::Balacruf, 11_000),
);

pub(super) const MANA_START: Destination = Destination::new(
    "TOWER OF MANA - START",
    362,
    [-9., -46., -3.],
    180.,
    Progress::Pilgrimage(Mission::Mana, 1000),
);

pub(super) const ISELIA_RANCH: Destination = Destination::new(
    "ISELIA HUMAN RANCH - START",
    194,
    [679., -3369., 0.],
    180.,
    Progress::IseliaInfiltration(20_303_000),
);

pub(super) const PALMACOSTA_RANCH: Destination = Destination::new(
    "PALMACOSTA HUMAN RANCH - START",
    201,
    [10., -498., 0.],
    180.,
    Progress::Pilgrimage(Mission::Palmacosta, 1200),
);

pub(super) const ASGARD_RANCH: Destination = Destination::new(
    "ASGARD HUMAN RANCH - START",
    213,
    [285., -218., 49.],
    270.,
    Progress::Pilgrimage(Mission::Asgard, 3010),
);

pub(super) const BASE_ENTRANCE: Destination = Destination::new(
    "SYLVARANT BASE - START",
    267,
    [-744., 441., -49.],
    0.,
    Progress::Story(1_101_000),
);

pub(super) const BASE_GENERATOR: Destination = Destination::new(
    "SYLVARANT BASE - GENERATOR WING",
    279,
    [-1084., 1472., 0.],
    90.,
    Progress::AfterSalvation(2_403_000),
);

pub(super) const FIRST_WINGS: Destination = Destination::new(
    "COLETTE'S FIRST WINGS",
    221,
    [0., 0., 0.],
    0.,
    Progress::Story(1_302_000),
);

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

pub(super) const MARTEL_END: Destination = MARTEL_START
    .at(
        "TEMPLE OF MARTEL - BEFORE ALTAR",
        307,
        [0., 2525., 0.],
        180.,
    )
    .progress(Progress::Story(108_000))
    .solved(MARTEL_BLOCKS);

pub(super) const TRIET_END: Destination = TRIET_START
    .at(
        "TRIET RUINS - BEFORE SEAL",
        220,
        [-2131., -354., 305.],
        180.,
    )
    .solved(TRIET_PLATFORMS);

pub(super) const OSSA_END: Destination = Destination::new(
    "OSSA TRAIL - END",
    350,
    [150., 400., -158.],
    0.,
    Progress::Story(1_402_000),
);

pub(super) const BASE_END: Destination = BASE_ENTRANCE
    .at("SYLVARANT BASE - END", 275, [-1000., 475., 0.], 180.)
    .progress(Progress::Story(1_107_000))
    .ring(SorcerersRing::ElectricOrb(ElectricOrbKind::Sylvarant));

pub(super) const THODA_END: Destination = THODA_START
    .at("THODA GEYSER - BEFORE SEAL", 9, [-2555., -811., 0.], 180.)
    .progress(Progress::Pilgrimage(Mission::Thoda, 13_000))
    .solved(THODA_WATERWAYS)
    .ring(SorcerersRing::Water);

pub(super) const PALMACOSTA_END: Destination = PALMACOSTA_RANCH
    .at("PALMACOSTA HUMAN RANCH - END", 206, [0., 500., 0.], 180.)
    .progress(Progress::Pilgrimage(Mission::Palmacosta, 4100))
    .ring(SorcerersRing::Radar);

pub(super) const BALACRUF_END: Destination = BALACRUF_START
    .at(
        "BALACRUF MAUSOLEUM - BEFORE SEAL",
        509,
        [11., 1530., 60.],
        180.,
    )
    .progress(Progress::Pilgrimage(Mission::Balacruf, 12_000))
    .solved(BALACRUF_WIND);

pub(super) const ASGARD_END: Destination = ASGARD_RANCH
    .at("ASGARD HUMAN RANCH - END", 214, [0., 700., 0.], 180.)
    .progress(Progress::Pilgrimage(Mission::Asgard, 5000))
    .solved(ASGARD_BLOCKS);

pub(super) const MANA_END: Destination = MANA_START
    .at(
        "TOWER OF MANA - BEFORE SEAL",
        366,
        [-652., 2321., 913.],
        270.,
    )
    .progress(Progress::Pilgrimage(Mission::Mana, 13_600))
    .solved(MANA_BRIDGES)
    .variables(MANA_MIRRORS);

pub(super) const SALVATION_END: Destination = Destination::new(
    "TOWER OF SALVATION - END",
    149,
    [0., 3900., 0.],
    180.,
    Progress::Story(2_302_000),
);

pub(super) const ISELIA_END: Destination = ISELIA_RANCH
    .at("ISELIA HUMAN RANCH - END", 196, [2065., 5651., -149.], 180.)
    .progress(Progress::IseliaInfiltration(20_305_000));

pub(super) const REMOTE_ISLAND_END: Destination = Destination::new(
    "REMOTE ISLAND RANCH - END",
    227,
    [250., -2., 23360.],
    90.,
    Progress::Reunited(13_401_000),
);

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
        Progress::ForestArrival,
    ),
    Destination::new(
        "ISELIA FOREST - END",
        233,
        [3711., 3861., 700.],
        180.,
        Progress::Story(104_000),
    ),
    MARTEL_START,
    MARTEL_END,
    TRIET_START,
    TRIET_END,
    Destination::new(
        "OSSA TRAIL - START",
        347,
        [-750., -381., 15.],
        167.,
        Progress::Story(1_305_000),
    ),
    OSSA_END,
    BASE_ENTRANCE,
    BASE_END,
    THODA_START,
    THODA_END,
    PALMACOSTA_RANCH,
    PALMACOSTA_END,
    BALACRUF_START,
    BALACRUF_END,
    ASGARD_RANCH
        .at("ASGARD HUMAN RANCH - START", 211, [-14., -344., 0.], 180.)
        .progress(Progress::Pilgrimage(Mission::Asgard, 3000)),
    ASGARD_END,
    MANA_START,
    MANA_END,
    Destination::new(
        "TOWER OF SALVATION - START",
        147,
        [6., 36., 0.],
        180.,
        Progress::Story(2_206_000),
    ),
    SALVATION_END,
    ISELIA_RANCH,
    ISELIA_END,
    Destination::new(
        "REMOTE ISLAND RANCH - START",
        223,
        [1., 1453., -389.],
        180.,
        Progress::Reunited(13_401_000),
    ),
    REMOTE_ISLAND_END,
    Destination::town("THODA DOCK", 82, [-194., -26., 0.], 100.),
    BASE_GENERATOR,
    FIRST_WINGS,
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
        party.formation = match self.progress {
            Progress::MartelEntrance => &[1, 2, 3, 9][..],
            Progress::ForestArrival => &[1, 3],
            Progress::Reunited(_) => &[1, 2, 3, 4, 5, 6, 7, 8],
            Progress::Pilgrimage(Mission::Asgard, _) => &[1, 2, 9, 4, 3, 5],
            Progress::IseliaInfiltration(_) => &[1, 2, 3, 9, 4, 6, 7, 8],
            _ => &PARTY,
        }
        .to_vec();
        party.field_leader = 1;
        party.travel.saved_formation = party.formation.clone();
        if !matches!(self.progress, Progress::MartelEntrance) {
            party.travel.sorcerers_ring = self.ring;
            party.items.insert(resonance_events::ring::ITEM, 1);
        }
        let mut persistent = PersistentState {
            party: Some(party),
            ..Default::default()
        };
        let story = match self.progress {
            Progress::MartelEntrance => 104_000,
            Progress::ForestArrival => 202_500,
            Progress::Story(story) => story,
            Progress::Reunited(story) | Progress::AfterSalvation(story) => {
                persistent.memory.write(ANGEL_PROGRESS, Width::S32, 1000)?;
                story
            }
            Progress::Pilgrimage(mission, value) => {
                const FIRE_SEAL: u16 = 200;
                const WATER_SEAL: u16 = 201;
                const WIND_SEAL: u16 = 202;
                let (angel_progress, seals) = match mission {
                    Mission::Balacruf => (101, &[FIRE_SEAL, WATER_SEAL][..]),
                    Mission::Mana => (201, &[FIRE_SEAL, WATER_SEAL, WIND_SEAL][..]),
                    _ => (1, &[FIRE_SEAL][..]),
                };
                persistent.event_flags.extend(seals);
                persistent
                    .memory
                    .write(ANGEL_PROGRESS, Width::S32, angel_progress)?;
                persistent.memory.write(mission as u16, Width::S32, value)?;
                if matches!(mission, Mission::Asgard) {
                    const ASGARD_RANCH_INFILTRATED: u16 = 330;
                    persistent.event_flags.insert(ASGARD_RANCH_INFILTRATED);
                    // Asgard's scenes rebuild both three-person groups from
                    // these per-member bits.
                    split_party(&mut persistent, 3)?;
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
                split_party(&mut persistent, 4)?;
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
fn split_party(state: &mut PersistentState, group_size: usize) -> Result<()> {
    const GROUP_FLAGS: u16 = 150;
    let party = state.party.as_ref().unwrap();
    state
        .memory
        .write(SAVED_LEADER, Width::S8, i32::from(party.field_leader))?;
    for (index, &id) in party.formation.iter().enumerate() {
        state
            .memory
            .write(SAVED_PARTY + index as u16, Width::S8, i32::from(id))?;
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
    Ok(())
}
