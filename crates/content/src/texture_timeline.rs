//! Human-authored UV timelines. Compact operations expand once at preparation;
//! ordinary rendering keeps the same sampled offsets and intro/loop clock.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_TICKS: usize = 36_000;
pub(crate) fn no_target<T>(_: &T) -> bool {
    std::mem::size_of::<T>() == 0
}
fn one() -> u32 {
    1
}
fn is_one(value: &u32) -> bool {
    *value == 1
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}
fn zero_offset(value: &[f32; 2]) -> bool {
    value.map(f32::to_bits) == [0; 2]
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Timeline {
    One(Segment),
    Many(Vec<Segment>),
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Segment {
    Offset([f32; 2]),
    Motion(Motion),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Motion {
    Hold {
        ticks: u32,
        #[serde(default, skip_serializing_if = "zero_offset")]
        offset: [f32; 2],
    },
    Scroll {
        ticks: u32,
        step: [f32; 2],
        #[serde(default, skip_serializing_if = "zero_offset")]
        origin: [f32; 2],
    },
    /// Whole texture turns over a cycle, with exact rational phase positions.
    Cycle { ticks: u32, turns: [i32; 2] },
    /// Row-major cells. Row offsets can extend past the grid for wrapping art.
    Atlas {
        grid: [u32; 2],
        #[serde(default = "one", skip_serializing_if = "is_one")]
        interval: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frames: Option<u32>,
        #[serde(default, skip_serializing_if = "is_zero")]
        first: u32,
    },
}

impl Motion {
    fn ticks(&self) -> Result<usize> {
        let ticks = match *self {
            Self::Hold { ticks, offset } => {
                ensure!(
                    offset.into_iter().all(f32::is_finite),
                    "nonfinite texture offset"
                );
                u64::from(ticks)
            }
            Self::Scroll {
                ticks,
                origin,
                step,
            } => {
                ensure!(
                    origin.into_iter().chain(step).all(f32::is_finite),
                    "nonfinite texture scroll"
                );
                u64::from(ticks)
            }
            Self::Cycle { ticks, .. } => u64::from(ticks),
            Self::Atlas {
                grid,
                interval,
                frames,
                ..
            } => {
                ensure!(
                    grid.into_iter().all(|n| n > 0) && interval > 0,
                    "empty texture atlas"
                );
                u64::from(interval)
                    .checked_mul(frames.map_or(u64::from(grid[0]) * u64::from(grid[1]), u64::from))
                    .context("texture atlas duration overflow")?
            }
        };
        ensure!(
            (1..=MAX_TICKS as u64).contains(&ticks),
            "texture sequence duration must be 1..=36000 ticks"
        );
        Ok(ticks as usize)
    }

    fn offset(&self, tick: usize) -> [f32; 2] {
        match *self {
            Self::Hold { offset, .. } => offset,
            Self::Scroll { step, origin, .. } => {
                std::array::from_fn(|axis| step[axis].mul_add(tick as f32, origin[axis]))
            }
            Self::Cycle { ticks, turns } => turns.map(|turns| {
                (tick as i64 * i64::from(turns)).rem_euclid(i64::from(ticks)) as f32 / ticks as f32
            }),
            Self::Atlas {
                grid,
                interval,
                first,
                ..
            } => {
                let frame = u64::from(first) + tick as u64 / u64::from(interval);
                [
                    (frame % u64::from(grid[0])) as f32 / grid[0] as f32,
                    (frame / u64::from(grid[0])) as f32 / grid[1] as f32,
                ]
            }
        }
    }
}

fn equal(a: [f32; 2], b: [f32; 2]) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}
fn matches(motion: &Motion, offsets: &[[f32; 2]]) -> bool {
    motion.ticks().ok() == Some(offsets.len())
        && offsets
            .iter()
            .enumerate()
            .all(|(i, &value)| equal(motion.offset(i), value))
}

/// Identify a complete ordinary motion before considering individual keyframes.
fn whole(offsets: &[[f32; 2]]) -> Option<Motion> {
    let ticks = offsets.len() as u32;
    let origin = *offsets.first()?;
    if offsets.iter().all(|&value| equal(value, origin)) {
        return Some(Motion::Hold {
            ticks,
            offset: origin,
        });
    }
    if offsets.len() < 4 {
        return None;
    }
    let step = std::array::from_fn(|axis| offsets[1][axis] - origin[axis]);
    let scroll = Motion::Scroll {
        ticks,
        step,
        origin,
    };
    if matches(&scroll, offsets) {
        return Some(scroll);
    }
    if equal(origin, [0.; 2])
        && offsets[1]
            .into_iter()
            .all(|value| (0. ..1.).contains(&value))
    {
        let turns = offsets[1].map(|value| {
            let mut turns = (f64::from(value) * f64::from(ticks)).round() as i32;
            if turns > ticks as i32 / 2 {
                turns -= ticks as i32;
            }
            turns
        });
        let cycle = Motion::Cycle { ticks, turns };
        if matches(&cycle, offsets) {
            return Some(cycle);
        }
    }
    let interval = offsets
        .iter()
        .take_while(|&&value| equal(value, origin))
        .count();
    if offsets.len() % interval != 0 || offsets.iter().flatten().any(|&value| value < 0.) {
        return None;
    }
    let mut grid = [1; 2];
    for axis in 0..2 {
        let mut values: Vec<_> = offsets.iter().map(|v| v[axis]).chain([0.]).collect();
        values.sort_by(f32::total_cmp);
        let spacing = values
            .windows(2)
            .map(|v| v[1] - v[0])
            .filter(|&v| v > 0.)
            .min_by(f32::total_cmp);
        if let Some(spacing) = spacing {
            let cells = (1. / spacing).round();
            if !(1. ..=MAX_TICKS as f32).contains(&cells) {
                return None;
            }
            grid[axis] = cells as u32;
        }
    }
    let first = (origin[1] * grid[1] as f32).round() as u32;
    let first = first
        .checked_mul(grid[0])?
        .checked_add((origin[0] * grid[0] as f32).round() as u32)?;
    let frames = offsets.len() as u32 / interval as u32;
    let atlas = Motion::Atlas {
        grid,
        interval: interval as u32,
        frames: (u64::from(frames) != u64::from(grid[0]) * u64::from(grid[1])).then_some(frames),
        first,
    };
    matches(&atlas, offsets).then_some(atlas)
}

fn compact(offsets: &[[f32; 2]]) -> Timeline {
    if offsets.len() == 1 {
        return Timeline::One(Segment::Offset(offsets[0]));
    }
    if let Some(motion) = whole(offsets) {
        return Timeline::One(Segment::Motion(motion));
    }
    let mut segments = Vec::new();
    let mut index = 0;
    while index < offsets.len() {
        let origin = offsets[index];
        let held = offsets[index..]
            .iter()
            .take_while(|&&v| equal(v, origin))
            .count();
        if held > 1 {
            segments.push(Segment::Motion(Motion::Hold {
                ticks: held as u32,
                offset: origin,
            }));
            index += held;
            continue;
        }
        if let Some(&next) = offsets.get(index + 1) {
            let step = std::array::from_fn(|axis| next[axis] - origin[axis]);
            let mut scroll = Motion::Scroll {
                ticks: 1,
                origin,
                step,
            };
            let length = offsets[index..]
                .iter()
                .enumerate()
                .take_while(|&(i, &v)| equal(scroll.offset(i), v))
                .count();
            if length >= 4 {
                let Motion::Scroll { ticks, .. } = &mut scroll else {
                    unreachable!()
                };
                *ticks = length as u32;
                segments.push(Segment::Motion(scroll));
                index += length;
                continue;
            }
        }
        segments.push(Segment::Offset(origin));
        index += 1;
    }
    Timeline::Many(segments)
}

pub(crate) fn serialize<S: Serializer>(
    offsets: &[[f32; 2]],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if offsets.is_empty()
        || offsets.len() > MAX_TICKS
        || !offsets.iter().flatten().all(|v| v.is_finite())
    {
        return Err(serde::ser::Error::custom("invalid texture sequence"));
    }
    compact(offsets).serialize(serializer)
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<[f32; 2]>, D::Error> {
    let sequence = Timeline::deserialize(deserializer)?;
    let segments = match sequence {
        Timeline::One(segment) => vec![segment],
        Timeline::Many(segments) => segments,
    };
    let result = (|| -> Result<_> {
        let mut offsets = Vec::new();
        for segment in segments {
            match segment {
                Segment::Offset(value) => {
                    ensure!(offsets.len() < MAX_TICKS, "texture sequence is too long");
                    offsets.push(value);
                }
                Segment::Motion(motion) => {
                    let ticks = motion.ticks()?;
                    ensure!(
                        offsets.len() + ticks <= MAX_TICKS,
                        "texture sequence is too long"
                    );
                    offsets.extend((0..ticks).map(|tick| motion.offset(tick)));
                }
            }
        }
        ensure!(
            !offsets.is_empty() && offsets.iter().flatten().all(|v| v.is_finite()),
            "invalid texture sequence"
        );
        Ok(offsets)
    })();
    result.map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests;
