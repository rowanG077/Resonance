use anyhow::Result;
use resonance_content::session::SessionData;
use resonance_events::{PersistentState, party::Party};
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
}

#[derive(Clone, Copy)]
enum Progress {
    MartelEntrance,
    Story(i32),
    AfterSalvation(i32),
    AfterFireSeal(Mission, i32),
    IseliaInfiltration,
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

const PARTY: [u8; 5] = [1, 2, 3, 4, 9];

impl Fixture {
    pub(super) const fn destination(self) -> Destination {
        match self {
            Self::Martel => Destination {
                name: "TEMPLE OF MARTEL",
                map: 307,
                position: [1., 194., 0.],
                heading: 180.,
                progress: Progress::MartelEntrance,
            },
            Self::FireSeal => Destination {
                name: "TRIET RUINS - FIRE SEAL",
                map: 219,
                position: [-265., -4., 15.],
                heading: 272.,
                progress: Progress::Story(1_302_000),
            },
            Self::WaterSeal => Destination {
                name: "THODA GEYSER - WATER SEAL",
                map: 7,
                position: [21., 67., 0.],
                heading: 188.,
                progress: Progress::AfterFireSeal(Mission::Thoda, 12_000),
            },
            Self::AirSeal => Destination {
                name: "BALACRUF MAUSOLEUM - AIR SEAL",
                map: 508,
                position: [8., 222., 0.],
                heading: 180.,
                progress: Progress::AfterFireSeal(Mission::Balacruf, 11_000),
            },
            Self::Mana => Destination {
                name: "TOWER OF MANA",
                map: 362,
                position: [-9., -46., -3.],
                heading: 180.,
                progress: Progress::AfterFireSeal(Mission::Mana, 1000),
            },
            Self::Iselia => Destination {
                name: "ISELIA HUMAN RANCH",
                map: 194,
                position: [679., -3369., 0.],
                heading: 180.,
                progress: Progress::IseliaInfiltration,
            },
            Self::Palmacosta => Destination {
                name: "PALMACOSTA HUMAN RANCH",
                map: 201,
                position: [10., -498., 0.],
                heading: 180.,
                progress: Progress::AfterFireSeal(Mission::Palmacosta, 1200),
            },
            Self::Asgard => Destination {
                name: "ASGARD HUMAN RANCH",
                map: 213,
                position: [285., -218., 49.],
                heading: 270.,
                progress: Progress::AfterFireSeal(Mission::Asgard, 3010),
            },
            Self::GuardEntrance => Destination {
                name: "SYLVARANT BASE - GUARD ENTRANCE",
                map: 267,
                position: [-744., 441., -49.],
                heading: 0.,
                progress: Progress::Story(1_101_000),
            },
            Self::Generator => Destination {
                name: "SYLVARANT BASE - GENERATOR WING",
                map: 279,
                position: [-1084., 1472., 0.],
                heading: 90.,
                progress: Progress::AfterSalvation(2_403_000),
            },
            Self::Wings => Destination {
                name: "COLETTE'S FIRST WINGS",
                map: 221,
                position: [0., 0., 0.],
                heading: 0.,
                progress: Progress::Story(1_302_000),
            },
        }
    }
}

pub(super) const DESTINATIONS: [Destination; 11] = [
    Fixture::Martel.destination(),
    Fixture::FireSeal.destination(),
    Fixture::WaterSeal.destination(),
    Fixture::AirSeal.destination(),
    Fixture::Mana.destination(),
    Fixture::Iselia.destination(),
    Fixture::Palmacosta.destination(),
    Fixture::Asgard.destination(),
    Fixture::GuardEntrance.destination(),
    Fixture::Generator.destination(),
    Fixture::Wings.destination(),
];

impl Destination {
    pub(crate) fn entry(
        self,
        data: Arc<SessionData>,
        available_fields: BTreeSet<u32>,
    ) -> Result<FieldEntry> {
        let mut party = Party::new(&data, Default::default())?;
        party.formation = PARTY.to_vec();
        party.field_leader = 1;
        party.travel.saved_formation = party.formation.clone();
        party.travel.sorcerers_ring = resonance_events::ring::SorcerersRing::Fire;
        party.items.insert(resonance_events::ring::ITEM, 1);
        let mut persistent = PersistentState {
            party: Some(party),
            ..Default::default()
        };
        let story = match self.progress {
            Progress::MartelEntrance => {
                // HOL_D02 introduces the golem at 104000 and removes the
                // altar ring at 107000. Let its original events grant the ring.
                let party = persistent.party.as_mut().unwrap();
                party.formation = vec![1, 2, 3, 9];
                party.travel.saved_formation = party.formation.clone();
                party.travel.sorcerers_ring = resonance_events::ring::SorcerersRing::Disabled;
                party.items.remove(&resonance_events::ring::ITEM);
                104_000
            }
            Progress::Story(story) => story,
            Progress::AfterSalvation(story) => {
                persistent.memory.write(0x4c, Width::S32, 1000)?;
                story
            }
            Progress::AfterFireSeal(mission, value) => {
                // Later seals require Colette's first angel progression branch.
                persistent.memory.write(0x4c, Width::S32, 1)?;
                persistent.memory.write(mission as u16, Width::S32, value)?;
                if matches!(mission, Mission::Asgard) {
                    // Stage 3010 follows the party split. Field 214 rebuilds
                    // both three-person groups from these per-member bits.
                    let party = persistent.party.as_mut().unwrap();
                    party.formation = vec![1, 2, 9, 4, 3, 5];
                    party.travel.saved_formation = party.formation.clone();
                    for (index, &id) in party.formation.iter().enumerate() {
                        let slot = index % 3;
                        let base = 150 + u16::from(id) * 3;
                        for (offset, set) in [index >= 3, slot & 2 != 0, slot & 1 != 0]
                            .into_iter()
                            .enumerate()
                        {
                            if set {
                                persistent.event_flags.insert(base + offset as u16);
                            }
                        }
                    }
                }
                if matches!(mission, Mission::Palmacosta) {
                    // Field 198's post-Magnius evacuation and destruction
                    // dispatch requires this story stage as well as mission B8.
                    2_002_000
                } else {
                    4_000_000
                }
            }
            Progress::IseliaInfiltration => {
                let party = persistent.party.as_mut().unwrap();
                // Sheena handles the escape route; FAA_D05 excludes her when
                // rebuilding the four-person party that confronts Forcystus.
                party.formation = vec![1, 2, 3, 4, 5, 6, 7, 8];
                party.travel.saved_formation = party.formation.clone();
                // FAA_D02 L_2D42 backs up the party and field leader before
                // the split. FAA_D01 restores both after the Forcystus battle.
                for (slot, id) in party.formation.iter().copied().enumerate() {
                    persistent
                        .memory
                        .write(0x150 + slot as u16, Width::S8, i32::from(id))?;
                }
                persistent.memory.write(0x158, Width::S8, 1)?;
                // FAA_D03 reconstructs groups from three bits per character:
                // reserve group followed by the two slot bits.
                for (slot, id) in party.formation.iter().copied().enumerate() {
                    let base = 150 + u16::from(id) * 3;
                    for (offset, set) in [slot >= 4, slot & 2 != 0, slot & 1 != 0]
                        .into_iter()
                        .enumerate()
                    {
                        if set {
                            persistent.event_flags.insert(base + offset as u16);
                        }
                    }
                }
                20_303_000
            }
        };
        persistent.memory.write(0x40, Width::S32, story)?;
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
