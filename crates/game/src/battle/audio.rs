//! Native sound IDs requested by battle behavior.
pub const TARGET_CHANGED: u16 = 108;
pub const RESULT_ADVANCEMENT: u16 = 80;
pub const STORED_SPELL_RELEASE: u16 = 122;
pub const SPELL_RELEASE: u16 = 123;

/// Fixed battle/menu feedback and playable-character behavior cues. Enemy and
/// arena selections are discovered from their own encounter definitions.
pub(crate) const COMMON_CUES: &[u16] = &[
    1,
    2,
    3,
    4,
    6,
    7,
    38,
    46,
    65,
    66,
    69,
    72,
    73,
    76,
    77,
    RESULT_ADVANCEMENT,
    81,
    95,
    TARGET_CHANGED,
    109,
    STORED_SPELL_RELEASE,
    SPELL_RELEASE,
    126,
    137,
    138,
    878,
    1258,
];

/// Shared player charge feedback uses the streamed voice namespace.
pub(crate) const COMMON_STREAMS: &[u16] = &[753];
