//! Native Z/Y/X vectors assembled by the checked Unison callbacks.
use crate::pe;
use anyhow::{Result, ensure};

pub(crate) fn read(image: &pe::Image<'_>, native: u16) -> Result<Vec<[f32; 3]>> {
    if native == 321 {
        return rock_pillar(image);
    }
    let (points, count): (&[[u32; 3]], usize) = match native {
        // 0x4240aa..0x4242ee writes this ten-vector sequence twice.
        324 => (
            &[
                [0, 0, 0x883cbc],
                [0x883d48, 0x8ad124, 0],
                [0, 0x882dc8, 0x7d9220],
                [0x72f400, 0x883f14, 0],
                [0x7d9220, 0x882e10, 0x883cbc],
                [0x883d48, 0, 0x883d48],
                [0x7d9220, 0x8ad124, 0x7d9220],
                [0x883d48, 0x883f10, 0x72f400],
                [0, 0x882e10, 0x883cbc],
                [0x883d48, 0x883f14, 0],
            ],
            20,
        ),
        // 0x424d5a..0x424e96 repeats the first three vectors after this ring.
        326 => (
            &[
                [0, 0, 0x88396c],
                [0x882efc, 0, 0x882efc],
                [0x88396c, 0, 0x883f0c],
                [0x882efc, 0, 0x882ac8],
                [0, 0, 0x7a36d4],
                [0x882ac8, 0, 0x882ac8],
                [0x7a36d4, 0, 0],
                [0x882ac8, 0, 0x882efc],
                [0, 0, 0],
            ],
            12,
        ),
        _ => return Ok(Vec::new()),
    };
    points
        .iter()
        .cycle()
        .take(count)
        .map(|point| {
            let mut position = [0.; 3];
            for (axis, &address) in point.iter().enumerate() {
                if address != 0 {
                    position[axis] = f32::from_le_bytes(image.slice(address, 4)?.try_into()?);
                }
            }
            ensure!(
                position.iter().all(|value| value.is_finite()),
                "invalid Unison pattern coordinate"
            );
            Ok([position[2], position[1], position[0]])
        })
        .collect()
}

fn rock_pillar(image: &pe::Image<'_>) -> Result<Vec<[f32; 3]>> {
    let n = f32::from_le_bytes(image.slice(0x883d48, 4)?.try_into()?);
    let p = f32::from_le_bytes(image.slice(0x72f400, 4)?.try_into()?);
    ensure!(n == -150. && p == 150., "Rock Pillar spacing changed");
    Ok([
        [0., 0., n],
        [n, 0., 0.],
        [0., 0., p],
        [p, 0., 0.],
        [p, 0., n],
        [n, 0., n],
        [p, 0., p],
        [n, 0., p],
        [0., 0., n],
        [n, 0., 0.],
    ]
    .into_iter()
    .map(|[z, y, x]| [x, y, z])
    .collect())
}
