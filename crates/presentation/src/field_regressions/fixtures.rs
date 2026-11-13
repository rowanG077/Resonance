//! Progression and entry fixtures compiled only with field gameplay tests.
use anyhow::Result;
use resonance_content::session::SessionData;
use resonance_events::{PersistentState, party::Party, ring::SorcerersRing};
use resonance_game::field::FieldEntry;
use std::{collections::BTreeSet, sync::Arc};
use symphonia_script::Width;

#[derive(Clone, Copy)]
pub(super) struct StoryFixture {
    position: [f32; 3],
    heading: f32,
    progress: Progress,
}

#[derive(Clone, Copy)]
enum Progress {
    MartelEntrance,
    Story(i32),
    AfterSalvation(i32),
    Pilgrimage(Mission, i32),
    IseliaInfiltration(i32),
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

pub(super) const MARTEL_START: StoryFixture =
    StoryFixture::new([1., 194., 0.], 180., Progress::MartelEntrance);

pub(super) const TRIET_START: StoryFixture =
    StoryFixture::new([-265., -4., 15.], 272., Progress::Story(1_302_000));

pub(super) const THODA_START: StoryFixture = StoryFixture::new(
    [21., 67., 0.],
    188.,
    Progress::Pilgrimage(Mission::Thoda, 12_000),
);

pub(super) const BALACRUF_START: StoryFixture = StoryFixture::new(
    [8., 222., 0.],
    180.,
    Progress::Pilgrimage(Mission::Balacruf, 11_000),
);

pub(super) const MANA_START: StoryFixture = StoryFixture::new(
    [-9., -46., -3.],
    180.,
    Progress::Pilgrimage(Mission::Mana, 1000),
);

pub(super) const ISELIA_RANCH: StoryFixture = StoryFixture::new(
    [679., -3369., 0.],
    180.,
    Progress::IseliaInfiltration(20_303_000),
);

pub(super) const PALMACOSTA_RANCH: StoryFixture = StoryFixture::new(
    [10., -498., 0.],
    180.,
    Progress::Pilgrimage(Mission::Palmacosta, 1200),
);

pub(super) const ASGARD_RANCH: StoryFixture = StoryFixture::new(
    [285., -218., 49.],
    270.,
    Progress::Pilgrimage(Mission::Asgard, 3010),
);

pub(super) const BASE_ENTRANCE: StoryFixture =
    StoryFixture::new([-744., 441., -49.], 0., Progress::Story(1_101_000));

pub(super) const BASE_GENERATOR: StoryFixture = StoryFixture::new(
    [-1084., 1472., 0.],
    90.,
    Progress::AfterSalvation(2_403_000),
);

impl StoryFixture {
    const fn new(position: [f32; 3], heading: f32, progress: Progress) -> Self {
        Self {
            position,
            heading,
            progress,
        }
    }

    pub(super) fn entry(
        self,
        data: Arc<SessionData>,
        available_fields: BTreeSet<u32>,
    ) -> Result<FieldEntry> {
        let mut party = Party::new(&data, Default::default())?;
        party.formation = match self.progress {
            Progress::MartelEntrance => &[1, 2, 3, 9][..],
            Progress::Pilgrimage(Mission::Asgard, _) => &[1, 2, 9, 4, 3, 5],
            Progress::IseliaInfiltration(_) => &[1, 2, 3, 9, 4, 6, 7, 8],
            _ => &PARTY,
        }
        .to_vec();
        party.field_leader = 1;
        party.travel.saved_formation = party.formation.clone();
        if !matches!(self.progress, Progress::MartelEntrance) {
            party.travel.sorcerers_ring = SorcerersRing::Fire;
            party.items.insert(resonance_events::ring::ITEM, 1);
        }
        let mut persistent = PersistentState {
            party: Some(party),
            ..Default::default()
        };
        let story = match self.progress {
            Progress::MartelEntrance => 104_000,
            Progress::Story(story) => story,
            Progress::AfterSalvation(story) => {
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
