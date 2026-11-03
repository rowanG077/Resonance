//! Compile source UV states into an ordinary image, checking shared texels.
//! The resulting declaration still uses the shared UV frames, birth ranges,
//! palette sets and GameCube geometry. There is no runtime atlas remapper.
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    BirthField, EffectOperation, ParticleBirth, ParticleTemplate, UvAnimation, UvChange, UvFrame,
    ValueRange,
    document::Definition,
    visual::{ParticleShape, UvLayout},
};
use std::collections::{BTreeMap, BTreeSet};

mod pixels;
pub(crate) type Rect = [i16; 4];

pub(crate) struct Plan {
    pub template: ParticleTemplate,
    pub rectangles: BTreeSet<Rect>,
    pub size: [u32; 2],
    shape: ParticleShape,
    counts: BTreeSet<u8>,
    origins: [BTreeMap<i16, i16>; 2],
}

impl Plan {
    pub(crate) fn read(bank: &Definition, member: usize) -> Result<Self> {
        let actor = bank
            .templates
            .get(member)
            .context("missing sprite declaration")?;
        let mut template = actor.template()?.clone();
        let mut shape = actor.particle_visual()?.geometry.clone();
        // The shared reader explicitly defers this native quad's owner-bone
        // drawing controller. Its rectangular artwork can still be cooked;
        // keep the original unsupported visual on the declaration.
        if matches!(&shape, ParticleShape::Unsupported { reason }
            if reason == "particle requires an unsupported bone or point-history drawing binding")
        {
            shape = ParticleShape::Quad;
        }
        let scroll_expanded = expand_scroll(&mut template)?;
        let mut rectangles = BTreeSet::from([template.state.uv]);
        if let Some(UvAnimation::Frames { frames, .. }) = &template.uv_animation {
            for frame in frames {
                if let UvChange::Rectangle { rect } = frame.change {
                    rectangles.insert(rect);
                }
            }
        }
        let mut counts = BTreeSet::from([template.state.geometry_count]);
        let births = bank
            .programs
            .iter()
            .flatten()
            .filter_map(|event| {
                if let EffectOperation::Spawn {
                    particle, birth, ..
                } = &event.operation
                {
                    (usize::from(*particle) == member).then_some(birth.as_ref())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for birth in &births {
            ensure!(
                !scroll_expanded
                    || (birth.uv.iter().all(Option::is_none)
                        && !birth
                            .sums
                            .iter()
                            .any(|sum| matches!(sum.field, BirthField::Uv(_)))),
                "scroll origin depends on sprite birth"
            );
            let values = (0..4)
                .map(|axis| birth_choices(birth, axis, template.state.uv[axis]))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                values
                    .iter()
                    .try_fold(1usize, |count, values| count.checked_mul(values.len()))
                    .is_some_and(|count| count <= 4096),
                "too many sprite birth UV states"
            );
            for &x in &values[0] {
                for &y in &values[1] {
                    for &width in &values[2] {
                        for &height in &values[3] {
                            rectangles.insert([x, y, width, height]);
                        }
                    }
                }
            }
            if let Some(range) = birth.geometry_count {
                for count in choices(range)? {
                    counts.insert(count.try_into()?);
                }
            }
        }
        ensure!(rectangles.len() <= 4096, "too many sprite frames");
        let mut extent = [[0i32, 0i32]; 2];
        for &rect in &rectangles {
            for &count in &counts {
                for quad in panels(&shape, rect, count)? {
                    for axis in 0..2 {
                        for endpoint in [quad[axis], quad[axis] + quad[axis + 2]] {
                            let offset = endpoint - i32::from(rect[axis]);
                            extent[axis][0] = extent[axis][0].min(offset);
                            extent[axis][1] = extent[axis][1].max(offset);
                        }
                    }
                }
            }
        }
        let mut origins: [BTreeMap<i16, i16>; 2] = Default::default();
        let mut size = [0u32; 2];
        for axis in 0..2 {
            let values = rectangles.iter().map(|r| r[axis]).collect::<BTreeSet<_>>();
            // Keep a texel on each side where possible. Large source rings
            // can fill the entire 4096-pixel axis with their authored panels;
            // in that case adjacent frame cells share a seam instead.
            let unpadded = extent[axis][1] - extent[axis][0];
            let padding = if (unpadded + 2) * values.len() as i32 <= 4096 {
                1
            } else {
                0
            };
            let pitch = unpadded + padding * 2;
            let separate = i64::from(pitch) * values.len() as i64 <= 4096;
            let first = i32::from(*values.first().context("sprite has no origins")?);
            let last = i32::from(*values.last().unwrap());
            // Nearby random origins can sample overlapping source pixels.
            // Retain that spacing if isolated cells would exceed the image
            // limit. image() still rejects conflicting native remaps where
            // overlapping interiors would require different source texels.
            size[axis] = u32::try_from(if separate {
                pitch * values.len() as i32
            } else {
                last - first + unpadded + 2
            })?;
            ensure!(
                (1..=4096).contains(&size[axis]),
                "sprite frames exceed image limits"
            );
            for (index, value) in values.into_iter().enumerate() {
                origins[axis].insert(
                    value,
                    (if separate {
                        index as i32 * pitch + padding - extent[axis][0]
                    } else {
                        i32::from(value) - first + 1 - extent[axis][0]
                    })
                    .try_into()?,
                );
            }
        }
        let mut plan = Self {
            template,
            rectangles,
            shape,
            counts,
            origins,
            size,
        };
        plan.template.state.uv = plan.rectangle(plan.template.state.uv)?;
        if let Some(UvAnimation::Frames { frames, .. }) = &mut plan.template.uv_animation {
            for frame in frames {
                if let UvChange::Rectangle { rect } = &mut frame.change {
                    for axis in 0..2 {
                        rect[axis] = plan.origins[axis][&rect[axis]];
                    }
                }
            }
        }
        // Reject a non-affine random choice map before publishing any images.
        for birth in births {
            plan.rewrite_birth(&mut birth.clone())?;
        }
        plan.template.validate()?;
        Ok(plan)
    }

    pub(crate) fn rectangle(&self, mut rect: Rect) -> Result<Rect> {
        for axis in 0..2 {
            rect[axis] = *self.origins[axis]
                .get(&rect[axis])
                .context("sprite origin was not enumerated")?;
        }
        Ok(rect)
    }

    pub(crate) fn rewrite_birth(&self, birth: &mut ParticleBirth) -> Result<()> {
        for axis in 0..2 {
            if let Some(sum) = birth
                .sums
                .iter_mut()
                .find(|sum| sum.field == BirthField::Uv(axis as u8))
            {
                let source = sum_choices(&sum.terms)?;
                let mapped = source
                    .iter()
                    .map(|value| {
                        self.origins[axis]
                            .get(value)
                            .copied()
                            .context("sprite UV sum origin was not enumerated")
                    })
                    .collect::<Result<Vec<_>>>()?;
                let scale = if source.len() > 1 {
                    (f64::from(*mapped.last().unwrap()) - f64::from(mapped[0]))
                        / (f64::from(*source.last().unwrap()) - f64::from(source[0]))
                } else {
                    1.
                };
                let offset = f64::from(mapped[0]) - f64::from(source[0]) * scale;
                ensure!(
                    source
                        .iter()
                        .zip(&mapped)
                        .all(
                            |(&a, &b)| (f64::from(a) * scale + offset - f64::from(b)).abs()
                                < 0.000001
                        ),
                    "sprite UV sum choices cannot use one affine image layout"
                );
                for (index, term) in sum.terms.iter_mut().enumerate() {
                    let translation = if index == 0 { offset } else { 0. };
                    term.min = (f64::from(term.min) * scale + translation) as f32;
                    term.max = (f64::from(term.max) * scale + translation) as f32;
                    term.step = (f64::from(term.step) * scale) as f32;
                    term.validate()?;
                }
                continue;
            }
            let Some(range) = &mut birth.uv[axis] else {
                continue;
            };
            let source = choices(*range)?;
            let mapped = source
                .iter()
                .map(|v| {
                    self.origins[axis]
                        .get(v)
                        .copied()
                        .context("sprite birth origin was not enumerated")
                })
                .collect::<Result<Vec<_>>>()?;
            let step = if mapped.len() > 1 {
                i32::from(mapped[1]) - i32::from(mapped[0])
            } else {
                0
            };
            ensure!(
                mapped
                    .iter()
                    .enumerate()
                    .all(|(i, &v)| i32::from(v) == i32::from(mapped[0]) + i as i32 * step),
                "sprite birth choices cannot use one shared range"
            );
            range.min = f32::from(mapped[0]);
            range.max = f32::from(*mapped.last().unwrap());
            if mapped.len() > 1 {
                range.step = step as f32;
            }
            // Draw identity, direction and consumption are intentionally kept.
            range.validate()?;
        }
        Ok(())
    }

    pub(crate) fn quads(&self, rect: Rect) -> Result<BTreeSet<[i32; 4]>> {
        self.counts
            .iter()
            .flat_map(|&count| match panels(&self.shape, rect, count) {
                Ok(v) => v.into_iter().map(Ok).collect(),
                Err(e) => vec![Err(e)],
            })
            .collect()
    }
}

fn sum_choices(terms: &[ValueRange]) -> Result<Vec<i16>> {
    ensure!(!terms.is_empty(), "empty sprite UV sum");
    let mut values = BTreeSet::from([0i32]);
    for &term in terms {
        let choices = choices(term)?;
        let mut next = BTreeSet::new();
        for &value in &values {
            for &choice in &choices {
                next.insert(value + i32::from(choice));
                ensure!(next.len() <= 4096, "too many sprite UV sum choices");
            }
        }
        values = next;
    }
    values
        .into_iter()
        .map(|value| Ok(value.try_into()?))
        .collect()
}

fn birth_choices(birth: &ParticleBirth, axis: usize, default: i16) -> Result<Vec<i16>> {
    if let Some(sum) = birth
        .sums
        .iter()
        .find(|sum| sum.field == BirthField::Uv(axis as u8))
    {
        sum_choices(&sum.terms)
    } else {
        birth.uv[axis]
            .map(choices)
            .transpose()
            .map(|values| values.unwrap_or_else(|| vec![default]))
    }
}

fn choices(range: ValueRange) -> Result<Vec<i16>> {
    range.validate()?;
    ensure!(
        range.min.fract() == 0.
            && range.max.fract() == 0.
            && range.min >= f32::from(i16::MIN)
            && range.max <= f32::from(i16::MAX),
        "noninteger sprite choice"
    );
    if range.min == range.max {
        return Ok(vec![range.min as i16]);
    }
    ensure!(
        range.step >= 1. && range.step.fract() == 0.,
        "continuous sprite choice"
    );
    let count = range.choice_count() as usize;
    ensure!(count <= 4096, "too many sprite choices");
    ensure!(
        range.min + (count - 1) as f32 * range.step == range.max,
        "sprite choice endpoints do not share one integer lattice"
    );
    Ok((0..count)
        .map(|i| (range.min + i as f32 * range.step) as i16)
        .collect())
}

fn expand_scroll(template: &mut ParticleTemplate) -> Result<bool> {
    let Some(UvAnimation::Scroll {
        origin,
        step,
        interval,
    }) = template.uv_animation.clone()
    else {
        return Ok(false);
    };
    let base = template.state.uv;
    ensure!(
        base[2] > 0 && base[3] > 0 && interval > 0,
        "invalid sprite scroll"
    );
    let mut frames = vec![UvFrame {
        duration: interval,
        change: UvChange::Rectangle { rect: base },
    }];
    let mut phase = [0i32; 2];
    loop {
        ensure!(
            frames.len() <= 4096,
            "sprite scroll period exceeds frame limits"
        );
        let mut rect = base;
        for axis in 0..2 {
            phase[axis] =
                (phase[axis] + i32::from(step[axis])).rem_euclid(i32::from(base[axis + 2]));
            rect[axis] = (i32::from(origin[axis]) + phase[axis]).try_into()?;
        }
        frames.push(UvFrame {
            duration: interval,
            change: UvChange::Rectangle { rect },
        });
        if phase == [0; 2] {
            break;
        }
    }
    template.uv_animation = Some(UvAnimation::Frames {
        frames,
        loop_start: Some(1),
    });
    Ok(true)
}

/// Pixel-space panels of the shared GameCube geometry. Spatial transforms,
/// vertex colors and native port geometry adjustments are not involved.
fn panels(shape: &ParticleShape, rect: Rect, count: u8) -> Result<Vec<[i32; 4]>> {
    let rect = rect.map(i32::from);
    let panel = |layout: UvLayout, index: u16| -> Result<[i32; 4]> {
        let mut q = rect;
        let column = match layout {
            UvLayout::Repeat => 0,
            UvLayout::Advance => index,
            UvLayout::Cycle if count == 0 => index,
            UvLayout::Cycle => index % u16::from(count),
            UvLayout::HalfWidthCycle => {
                q[2] >>= 1;
                index % (u16::from(count.max(1)) * 2)
            }
            UvLayout::AlternatingHalves { panels_per_half } => {
                ensure!(panels_per_half > 0, "zero UV panels per half");
                q[2] >>= 1;
                (index / u16::from(panels_per_half)) % 2
            }
        };
        q[0] += i32::from(column) * q[2];
        Ok(q)
    };
    match *shape {
        ParticleShape::Ring {
            segments,
            uv_layout,
            ..
        }
        | ParticleShape::Shell {
            segments,
            uv_layout,
            ..
        } => (0..segments)
            .map(|i| panel(uv_layout, u16::from(i)))
            .collect(),
        ParticleShape::Spiral { uv_layout, .. } => {
            (0..count).map(|i| panel(uv_layout, u16::from(i))).collect()
        }
        ParticleShape::RadialQuads { advance_u, .. } => (0..count)
            .map(|i| {
                panel(
                    if advance_u {
                        UvLayout::Advance
                    } else {
                        UvLayout::Repeat
                    },
                    u16::from(i),
                )
            })
            .collect(),
        ParticleShape::Ribbon { .. } => Ok((0..count)
            .map(|i| {
                let mut q = rect;
                q[1] += q[3] * (i32::from(i > 0) + i32::from(i == count - 1));
                q
            })
            .collect()),
        ParticleShape::Quad
        | ParticleShape::Disc { .. }
        | ParticleShape::VertexQuad { .. }
        | ParticleShape::Orbit
        | ParticleShape::BillboardTrail
        | ParticleShape::Sphere { .. }
        | ParticleShape::Hemisphere { .. }
        | ParticleShape::TriangleBand
        | ParticleShape::SquareBand => Ok(vec![rect]),
        _ => anyhow::bail!("sprite geometry needs frame normalization"),
    }
}

#[cfg(test)]
mod tests;
