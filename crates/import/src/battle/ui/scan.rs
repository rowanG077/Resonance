use super::*;
use resonance_content::battle::ui::ScanLayout;

pub(super) fn cook(
    style: &super::super::ui_tables::ScanStyle,
    sprite: &mut impl FnMut(usize, [u32; 4]) -> Result<UiRegion>,
) -> Result<ScanLayout> {
    let affinities = (0..9)
        .map(|index| sprite(48 + index, [index as u32 * 24, 136, 24, 24]))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid scan affinity count"))?;
    Ok(ScanLayout {
        affinities,
        weakness: sprite(57, [214, 136, 24, 24])?,
        resistance: sprite(57, [238, 136, 24, 24])?,
        colors: style.colors,
        spacing_reduction: style.spacing[..5].try_into()?,
    })
}
