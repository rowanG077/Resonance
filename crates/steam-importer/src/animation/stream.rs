use super::*;

pub(super) fn read(bytes: &[u8]) -> Result<Curve> {
    let mut source = Cursor::new(bytes);
    let format = source.word()?;
    let mode = format & 15;
    let scalar = (format >> 4) & 15;
    let clock = (format >> 8) & 15;
    let dimensions = (format >> 12) as usize;
    ensure!(
        (1..=4).contains(&dimensions) && scalar <= 1,
        "unsupported Steam animation vector format {format:#x}"
    );
    ensure!(
        matches!((mode, clock), (3, 0) | (0 | 2 | 8 | 9, 1..=4)),
        "unsupported Steam curve encoding {format:#x}"
    );
    let normalized = scalar == 1;
    if mode == 3 {
        let value = source.vector(dimensions, normalized)?;
        ensure!(source.at == bytes.len(), "unexpected constant curve extent");
        return Ok(Curve {
            dimensions,
            interpolation: Interpolation::Step,
            times: vec![0.],
            values: vec![value],
        });
    }
    let count = source.word()? as usize;
    ensure!((1..=MAX_KEYS).contains(&count), "invalid Steam key count");
    // All installed streams use native frame units. Reject a new clock profile
    // until its interpretation is verified, rather than dropping its multiplier.
    ensure!(source.float()? == 1., "unsupported Steam curve time factor");
    let times = (0..count)
        .map(|i| match clock {
            1 => source.float(),
            2 => Ok(f32::from(source.half()?)),
            3 => Ok(f32::from(source.byte()?)),
            4 => Ok(i as f32),
            _ => unreachable!(),
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        times[0] == 0. && times.windows(2).all(|t| t[0] < t[1]),
        "invalid Steam key timeline"
    );
    source.align()?;
    let values = if mode < 3 {
        (0..count)
            .map(|_| source.vector(dimensions, normalized))
            .collect::<Result<Vec<_>>>()?
    } else {
        dct::read(&mut source, mode == 9, count, dimensions, normalized)?
    };
    ensure!(
        source.at == bytes.len(),
        "unexpected Steam curve extent: {} != {}",
        source.at,
        bytes.len()
    );
    ensure!(
        values.iter().flatten().all(|v| v.is_finite()),
        "invalid decoded Steam curve value"
    );
    Ok(Curve {
        dimensions,
        interpolation: if mode == 2 {
            Interpolation::Step
        } else {
            Interpolation::Linear
        },
        times,
        values,
    })
}
