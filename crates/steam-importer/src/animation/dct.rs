//! Quantized DCT-IV residuals, relative to explicitly stored segment anchors.
use super::*;
use std::{collections::BTreeMap, sync::LazyLock};

static BASIS: LazyLock<Vec<Vec<f64>>> = LazyLock::new(|| {
    (1..=8)
        .map(|groups| {
            let n = groups * 4;
            (0..n * n)
                .map(|index| {
                    let row = index / n;
                    let column = index % n;
                    (2. / n as f64).sqrt()
                        * (std::f64::consts::PI * (row as f64 + 0.5) * (column as f64 + 0.5)
                            / n as f64)
                            .cos()
                })
                .collect()
        })
        .collect()
});

struct Component {
    width: usize,
    coefficients: Vec<f64>,
}

impl Component {
    fn read(source: &mut Cursor<'_>, scale: f64) -> Result<Self> {
        let first = f64::from(source.half()?) / 65535. * scale;
        let second = f64::from(source.half()?) / 65535. * first;
        let third = f64::from(source.byte()?) / 255. * second;
        let counts = source.byte()?;
        let tail = source.byte()?;
        let ranges = source.byte()?;
        let shorts = usize::from(counts >> 4);
        let bytes = usize::from(counts & 15);
        let nibbles = usize::from(tail >> 4);
        let zeros = usize::from(tail & 15);
        let groups = shorts + bytes + nibbles + zeros;
        ensure!(
            (1..=8).contains(&groups) && nibbles % 2 == 0,
            "invalid compressed animation coefficient counts"
        );
        let mut coefficients = Vec::new();
        for _ in 0..shorts * 4 {
            coefficients.push(f64::from(source.half()? as i16) / 32767.);
        }
        for _ in 0..bytes * 4 {
            coefficients.push(f64::from(source.byte()? as i8) / 127.);
        }
        for _ in 0..nibbles / 2 {
            let packed = source.take(4)?;
            coefficients.extend(packed.iter().map(|&b| (f64::from(b >> 4) - 8.) / 7.));
            coefficients.extend(packed.iter().map(|&b| (f64::from(b & 15) - 8.) / 7.));
        }
        for (i, value) in coefficients.iter_mut().enumerate() {
            let group = i / 4;
            *value *= if group < usize::from(ranges >> 4) {
                first
            } else if group < usize::from(ranges >> 4) + usize::from(ranges & 15) {
                second
            } else {
                third
            };
        }
        Ok(Self {
            width: groups * 4,
            coefficients,
        })
    }

    fn residual(&self, row: usize) -> Result<f32> {
        if self.coefficients.is_empty() {
            return Ok(0.);
        }
        ensure!(
            row < self.width,
            "compressed animation block has too few samples"
        );
        let basis = &BASIS[self.width / 4 - 1][row * self.width..];
        Ok(self
            .coefficients
            .iter()
            .zip(basis)
            .map(|(value, weight)| value * weight)
            .sum::<f64>() as f32)
    }
}

pub(super) fn read(
    source: &mut Cursor<'_>,
    segmented: bool,
    count: usize,
    dimensions: usize,
    normalized: bool,
) -> Result<Vec<[f32; 4]>> {
    ensure!(count >= 2, "compressed animation needs endpoints");
    let scale = f64::from(source.float()?);
    let expected = (count - 1).div_ceil(33);
    let segments = if segmented {
        usize::from(source.half()?)
    } else {
        1
    };
    ensure!(
        segments == expected,
        "compressed animation segment count mismatch"
    );
    let mut offsets = vec![0usize];
    if segmented {
        for _ in 1..segments {
            offsets.push(usize::from(source.half()?) * 4);
        }
    }
    if !normalized {
        source.align()?;
    }
    let anchors = (0..=segments)
        .map(|_| source.vector(dimensions, normalized))
        .collect::<Result<Vec<_>>>()?;
    let base = source.at;
    let mut blocks = BTreeMap::new();
    for &offset in &offsets {
        blocks.insert(offset, Vec::new());
    }
    let positions = blocks.keys().copied().collect::<Vec<_>>();
    for (i, &offset) in positions.iter().enumerate() {
        let start = base
            .checked_add(offset)
            .context("compressed animation offset overflow")?;
        let end = positions
            .get(i + 1)
            .map_or(source.bytes.len(), |next| base + next);
        let data = source
            .bytes
            .get(start..end)
            .context("compressed animation segment exceeds stream")?;
        let mut segment = Cursor::new(data);
        let components = (0..dimensions)
            .map(|_| Component::read(&mut segment, scale))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            segment.at == data.len(),
            "unexpected compressed animation segment extent"
        );
        blocks.insert(offset, components);
    }
    source.at = source.bytes.len();
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        if i == count - 1 {
            values.push(anchors[segments]);
            continue;
        }
        let segment = i / 33;
        let sample = i % 33;
        let length = 33.min(count - 1 - segment * 33);
        let t = sample as f32 / length as f32;
        let mut value = std::array::from_fn(|axis| {
            anchors[segment][axis] * (1. - t) + anchors[segment + 1][axis] * t
        });
        // The first block starts at zero implicitly. In the directory for later
        // segments, zero means no residual, not a reference to that first block.
        if sample != 0 && (segment == 0 || offsets[segment] != 0) {
            for (axis, component) in blocks[&offsets[segment]].iter().enumerate() {
                value[axis] += component.residual(sample - 1)?;
            }
        }
        values.push(value);
    }
    Ok(values)
}
