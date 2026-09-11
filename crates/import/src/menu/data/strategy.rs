use super::*;
use resonance_content::menu_data::{STRATEGY_COUNTS, StrategyData, StrategyOption, StrategyPreset};

pub(super) fn cook(
    executable: &[u8],
    text: &impl Fn(&[u8], usize) -> Result<String>,
) -> Result<StrategyData> {
    let groups = [0x80208c3c, 0x80208ccc, 0x80208d5c]
        .into_iter()
        .zip(STRATEGY_COUNTS)
        .map(|(address, count)| {
            dol::slice(executable, address, count * 16)?
                .chunks_exact(16)
                .map(|row| {
                    let mask = u16::from(row[12]);
                    Ok(StrategyOption {
                        name: text(row, 0)?,
                        description: text(row, 4)?,
                        details: text(row, 8)?,
                        characters: mask | ((mask & 0x20) << 3),
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let presets = (0..3)
        .map(|i| {
            let address = 0x80208b88 + i * 60;
            let row = dol::slice(executable, address, 60)?;
            Ok(StrategyPreset {
                name: text(&address.to_be_bytes(), 0)?,
                members: std::array::from_fn(|member| {
                    row[32 + member * 3..35 + member * 3].try_into().unwrap()
                }),
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    let positions: [u8; 6] = dol::slice(executable, 0x8035c044, 6)?.try_into()?;
    let strings = |address, count: usize| {
        dol::slice(executable, address, count * 4)?
            .chunks_exact(4)
            .map(|row| text(row, 0))
            .collect::<Result<Vec<_>>>()
    };
    Ok(StrategyData {
        groups,
        presets,
        positions,
        default_positions: dol::slice(executable, 0x8018adf4, 9)?
            .iter()
            .map(|id| positions[usize::from(id - 1)])
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        keyboard: String::from_utf8(dol::slice(executable, 0x801ab150, 90)?.to_vec())?,
        labels: strings(0x801ab12c, 3)?.try_into().unwrap(),
        keys: strings(0x801ab1ac, 9)?.try_into().unwrap(),
    })
}
