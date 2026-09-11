use super::*;
use resonance_content::menu_data::RenameData;

pub(super) fn cook(executable: &[u8]) -> Result<RenameData> {
    let label = |index: u32| -> Result<String> {
        let pointer = crate::read::u32(dol::slice(executable, 0x8019d3e8 + index * 4, 4)?, 0)?;
        dol::text(executable, pointer)
    };
    let names = (0..9)
        .map(|i| dol::text(executable, 0x801f9fc8 + i * 0x118))
        .collect::<Result<Vec<_>>>()?;
    let defaults = (6..15).map(label).collect::<Result<Vec<_>>>()?;
    let keyboard = crate::read::u32(dol::slice(executable, 0x8035cf68, 4)?, 0)?;
    let data = RenameData {
        initial_names: names.try_into().unwrap(),
        defaults: defaults.try_into().unwrap(),
        keyboard: dol::text(executable, keyboard)?,
        heading: label(0)?,
        delete: label(1)?,
        default: label(2)?,
        commands: [label(3)?, label(4)?, label(5)?],
    };
    data.validate()?;
    Ok(data)
}
