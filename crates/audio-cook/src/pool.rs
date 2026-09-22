//! Physical instrument mappings and table views, independent of active voices.
use crate::{
    bank::{Bank, ObjectKind},
    parameters, read,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Serialize)]
pub struct Key {
    pub object: u16,
    pub transpose: i8,
    /// Values 0..127 offset the incoming pan by -64..63; bit 7 selects surround.
    pub pan: u8,
    pub priority_delta: i16,
    pub reserved: u16,
}

pub fn key(bytes: &[u8], index: u8) -> Result<Key> {
    ensure!(bytes.len() == 128 * 8, "invalid keymap length");
    let entry = read::slice(bytes, usize::from(index) * 8, 8)?;
    Ok(Key {
        object: read::u16(entry, 0)?,
        transpose: entry[2] as i8,
        pan: entry[3],
        priority_delta: read::u16(entry, 4)? as i16,
        reserved: read::u16(entry, 6)?,
    })
}

#[derive(Serialize)]
pub struct Keymap {
    pub id: u16,
    /// Indexed by the original MIDI key, including disabled (65535) entries.
    pub keys: Vec<Key>,
}

pub fn keymap(bank: &Bank<'_>, id: u16) -> Result<Keymap> {
    let bytes = bank.object(ObjectKind::Keymap, id)?;
    Ok(Keymap {
        id,
        keys: (0..128)
            .map(|index| key(bytes, index))
            .collect::<Result<_>>()?,
    })
}

#[derive(Clone, Copy, Serialize)]
pub struct LayerEntry {
    pub object: u16,
    pub minimum_key: u8,
    pub maximum_key: u8,
    pub transpose: i8,
    /// Multiplies incoming velocity and divides by 127.
    pub velocity_scale: u8,
    /// Accumulates across matching layers, in their authored order.
    pub priority_delta: i16,
    pub pan: u8,
    pub reserved: [u8; 3],
}

pub fn layer_entries(bytes: &[u8]) -> Result<impl Iterator<Item = Result<LayerEntry>> + '_> {
    let count = usize::from(read::u16(bytes, 2)?);
    ensure!(bytes.len() == 4 + count * 12, "invalid layer length");
    Ok(bytes[4..].chunks_exact(12).map(|entry| {
        Ok(LayerEntry {
            object: read::u16(entry, 0)?,
            minimum_key: entry[2],
            maximum_key: entry[3],
            transpose: entry[4] as i8,
            velocity_scale: entry[5],
            priority_delta: read::u16(entry, 6)? as i16,
            pan: entry[8],
            reserved: entry[9..12].try_into()?,
        })
    }))
}

#[derive(Serialize)]
pub struct Layer {
    pub id: u16,
    pub reserved: u16,
    pub entries: Vec<LayerEntry>,
}

pub fn layer(bank: &Bank<'_>, id: u16) -> Result<Layer> {
    let bytes = bank.object(ObjectKind::Layer, id)?;
    Ok(Layer {
        id,
        reserved: read::u16(bytes, 0)?,
        entries: layer_entries(bytes)?.collect::<Result<_>>()?,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    Envelope,
    DlsEnvelope,
    PitchEnvelope,
    VolumeCurve,
}

impl TableKind {
    fn length(self) -> usize {
        match self {
            Self::Envelope => 8,
            Self::DlsEnvelope | Self::PitchEnvelope => 20,
            Self::VolumeCurve => 128,
        }
    }
}

#[derive(Serialize)]
pub struct Consumer {
    pub program: u16,
    pub instruction: usize,
}

pub type TableUses = BTreeMap<u16, BTreeMap<TableKind, Vec<Consumer>>>;

/// Table rows have no type tag. Native macro consumers select their layout;
/// a single physical row may legitimately have more than one semantic view.
pub fn table_uses(bank: &Bank<'_>) -> Result<TableUses> {
    let mut uses = TableUses::new();
    for program in bank.object_ids(ObjectKind::Macro) {
        let bytes = bank.object(ObjectKind::Macro, program)?;
        ensure!(
            bytes.len().is_multiple_of(8),
            "invalid instrument {program} length"
        );
        for (instruction, bytes) in bytes.chunks_exact(8).enumerate() {
            let a = read::u32(bytes, 0)? & !0x80;
            let b = read::u32(bytes, 4)?;
            let (id, kind) = match a as u8 {
                0x0c => (
                    (a >> 8) as u16,
                    if a >> 24 == 0 {
                        TableKind::Envelope
                    } else {
                        TableKind::DlsEnvelope
                    },
                ),
                0x20 => ((a >> 8) as u16, TableKind::PitchEnvelope),
                0x0d | 0x0f | 0x14 => {
                    let id = ((a >> 24) | ((b & 255) << 8)) as u16;
                    if id == u16::MAX {
                        continue;
                    }
                    (id, TableKind::VolumeCurve)
                }
                _ => continue,
            };
            uses.entry(id)
                .or_default()
                .entry(kind)
                .or_default()
                .push(Consumer {
                    program,
                    instruction,
                });
        }
    }
    Ok(uses)
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableView<'a> {
    Envelope {
        consumers: &'a [Consumer],
        parameters: resonance_audio::envelope::Parameters,
        sustain_source: u16,
    },
    DlsEnvelope {
        consumers: &'a [Consumer],
        parameters: resonance_audio::dls::Definition,
        sustain_source: u16,
    },
    PitchEnvelope {
        consumers: &'a [Consumer],
        parameters: resonance_audio::dls::Definition,
        sustain_source: u16,
    },
    VolumeCurve {
        consumers: &'a [Consumer],
        /// Output volume at each integer input level 0..127; fractional levels
        /// linearly interpolate adjacent points.
        levels: Vec<u8>,
    },
}

#[derive(Serialize)]
pub struct Table<'a> {
    pub id: u16,
    pub source_bytes: usize,
    pub views: Vec<TableView<'a>>,
}

pub fn table<'a>(bank: &Bank<'_>, id: u16, uses: &'a TableUses) -> Result<Table<'a>> {
    let bytes = bank.object(ObjectKind::Table, id)?;
    let consumers = uses.get(&id);
    ensure!(
        consumers.is_some(),
        "table {id}: unknown semantics for {} bytes (no identified macro consumer)",
        bytes.len()
    );
    let consumers = consumers.unwrap();
    let maximum = consumers.keys().map(|kind| kind.length()).max().unwrap();
    ensure!(
        bytes.len() <= maximum,
        "table {id}: {} bytes exceed its known {maximum}-byte views",
        bytes.len()
    );
    let mut views = Vec::new();
    for (&kind, consumers) in consumers {
        let view = match kind {
            TableKind::Envelope => TableView::Envelope {
                consumers,
                parameters: parameters::ordinary(bytes)?,
                sustain_source: u16::from_le_bytes(read::slice(bytes, 4, 2)?.try_into()?),
            },
            TableKind::DlsEnvelope => TableView::DlsEnvelope {
                consumers,
                parameters: parameters::dls(bytes)?,
                sustain_source: u16::from_le_bytes(read::slice(bytes, 8, 2)?.try_into()?),
            },
            TableKind::PitchEnvelope => {
                let bytes = bank.pitch_envelope(id)?;
                TableView::PitchEnvelope {
                    consumers,
                    parameters: parameters::dls(bytes)?,
                    sustain_source: u16::from_le_bytes(bytes[8..10].try_into()?),
                }
            }
            TableKind::VolumeCurve => TableView::VolumeCurve {
                consumers,
                levels: bank.volume_curve(id)?.to_vec(),
            },
        };
        views.push(view);
    }
    Ok(Table {
        id,
        source_bytes: bytes.len(),
        views,
    })
}
