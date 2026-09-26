//! Actor-owned floating damage numbers: native 6F31C..6F48C, also 6696C.
//! Initialization and clocks belong to ActorHud (21B18 / 65E24).
use super::{Art, Batch, quad, results};
use anyhow::{Context, Result, ensure};
use resonance_battle::FloatingNumber;

/// Call in party/enemy roster order, and for each actor visit its four records
/// starting at `number_cursor`. `slot` is the physical record index, not its
/// position in that visit order. Numbers draw after actor labels, before gauges.
pub(super) fn draw(
    batch: &mut Batch,
    art: &Art,
    number: &FloatingNumber,
    slot: usize,
    projected: [i16; 2],
) -> Result<()> {
    if number.alpha == 0 {
        return Ok(());
    }
    ensure!(slot < 4, "invalid floating number slot");
    // 4DA2C is an unsigned decimal digit count. The ordinary battle amount
    // supplied to this display is nonnegative even when HP application heals.
    ensure!(number.value >= 0, "negative floating battle amount");
    let mut colors = *art
        .combat_number_colors
        .get(usize::from(number.palette))
        .context("invalid floating number palette")?;
    let dimensions = *art
        .combat_number_sizes
        .get(usize::from(number.style))
        .context("invalid floating number style")?;
    let layout = layout(number, slot, projected, dimensions);
    for color in &mut colors {
        color[3] = layout.alpha;
    }
    results::glyphs(
        batch,
        art,
        &number.value.to_string(),
        layout.position.map(f32::from),
        layout.size.map(f32::from),
        f32::from(layout.skew),
        f32::from(layout.advance),
        colors,
        None,
    )
}

#[derive(Debug, PartialEq)]
struct Layout {
    position: [i16; 2],
    size: [i16; 2],
    skew: i8,
    advance: i8,
    alpha: u8,
}

fn layout(number: &FloatingNumber, slot: usize, projected: [i16; 2], size: [i16; 2]) -> Layout {
    let slot = slot as i16;
    let x = projected[0].wrapping_sub((slot - 1) * 8);
    let y = projected[1].wrapping_sub(slot * 12);
    let digits = (number.value as u32).checked_ilog10().unwrap_or(0) as i16 + 1;
    let pulse = i16::from(number.pulse);
    let width = size[0] + pulse * 2;
    Layout {
        position: [
            x.wrapping_add(number.offset[0])
                .wrapping_sub((digits - 1) * 14),
            y.wrapping_add(number.offset[1]).wrapping_sub(8),
        ],
        size: [width, size[1] + pulse * 2],
        skew: (pulse + 10) as i8,
        advance: width as i8,
        // The native byte store intentionally narrows; it does not clamp.
        alpha: (i32::from(number.alpha) - i32::from(pulse) * 16) as u8,
    }
}

/// 685F0/4C9A4 emit least-significant digits first, preserving italic overlap.
fn recovery(
    batch: &mut Batch,
    art: &resonance_content::battle_ui::RecoveryNumbers,
    number: &resonance_battle::RecoveryNumber,
    party_slot: usize,
    row: usize,
) -> Result<()> {
    if number.alpha == 0 {
        return Ok(());
    }
    ensure!(
        number.value >= 0 && row < 2,
        "invalid party recovery number"
    );
    let right = i32::from(art.origin[0])
        + party_slot as i32 * i32::from(art.party_spacing)
        + i32::from(number.x_delta >> 4);
    let y = f32::from(art.origin[1]) + row as f32 * f32::from(art.row_spacing);
    let [width, height] = art.number.glyph_size.map(f32::from);
    let [u, v, w, h] = art.rect.map(f32::from);
    let mut colors = art.colors[row];
    for color in &mut colors {
        color[3] = number.alpha as u8;
    }
    let mut value = number.value as u32;
    let mut index = 0;
    loop {
        let digit = value % 10;
        let x = right as f32 - width - index as f32 * f32::from(art.number.advance);
        let u = u + digit as f32 * w;
        quad(
            batch,
            [x, y, x + width, y + height],
            [u, v, u + w, v + h],
            f32::from(art.number.skew),
            [colors[0], colors[0], colors[1], colors[1]],
        );
        value /= 10;
        if value == 0 {
            break;
        }
        index += 1;
    }
    Ok(())
}

impl super::Artwork {
    pub(super) fn render_numbers(
        &mut self,
        frame: &resonance_battle::BattleFrame,
        commands: &mut super::Commands,
        meshes: &mut super::Assets<super::Mesh>,
    ) -> Result<()> {
        use resonance_battle::Side;
        let mut floating = Batch::default();
        let mut recovered = Batch::default();
        for side in [Side::Party, Side::Enemy] {
            for (party_slot, actor) in frame
                .actors
                .iter()
                .filter(|actor| actor.side == side)
                .enumerate()
            {
                for index in 0..4 {
                    let slot = (usize::from(actor.hud.number_cursor) + index) % 4;
                    let number = &actor.hud.floating[slot];
                    if number.alpha != 0 {
                        let camera = frame
                            .camera
                            .context("floating number requires battle camera")?;
                        let projected =
                            resonance_battle::project_screen_point(camera, number.position)
                                .map(|value| value as i16);
                        draw(&mut floating, &self.art, number, slot, projected)?;
                    }
                }
                if side == Side::Party {
                    for (row, number) in actor.hud.recovery.iter().enumerate() {
                        recovery(&mut recovered, &self.art.recovery, number, party_slot, row)?;
                    }
                }
            }
        }
        for (index, batch) in [
            (super::COMBAT_NUMBERS, floating),
            (super::RECOVERY_NUMBERS, recovered),
        ] {
            let visible = !batch.indices.is_empty();
            let layer = &mut self.layers[index];
            layer.update_mesh(batch, [self.art.font.width, self.art.font.height], meshes)?;
            layer.show(visible, commands);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_digits_keep_native_right_to_left_order_and_full_24_pixel_source() {
        let art = resonance_content::battle_ui::RecoveryNumbers {
            origin: [104, 396],
            party_spacing: 110,
            row_spacing: 24,
            rect: [0, 0, 16, 24],
            number: resonance_content::battle_ui::Number {
                glyph_size: [16, 24],
                advance: 13,
                skew: 12,
            },
            colors: [[[112, 128, 112, 255], [80, 128, 80, 255]]; 2],
        };
        let mut number = resonance_battle::RecoveryNumber::default();
        number.value = 123;
        number.alpha = 247;
        number.x_delta = 24;
        let mut batch = Batch::default();
        recovery(&mut batch, &art, &number, 1, 1).unwrap();
        assert_eq!(batch.positions.len(), 12);
        assert_eq!(batch.positions[0], [-109., -180., 0.]);
        assert_eq!(batch.positions[4], [-122., -180., 0.]);
        assert_eq!(batch.positions[8], [-135., -180., 0.]);
        assert_eq!(batch.uv[0], [48., 0.]);
        assert_eq!(batch.uv[4], [32., 0.]);
        assert_eq!(batch.uv[8], [16., 0.]);
        assert_eq!(batch.uv[2][1], 24.);
        assert_eq!(batch.colors[0][3], 247. / 255.);
        assert!(batch.secondary_uv.is_empty());
    }

    #[test]
    fn birth_expansion_and_physical_slot_offsets_match_native_draw() {
        let mut number = FloatingNumber::default();
        number.value = 123;
        number.alpha = 255;
        number.pulse = 12;
        number.offset = [0, -24];
        assert_eq!(
            layout(&number, 0, [320, 224], [15, 22]),
            Layout {
                position: [300, 192],
                size: [39, 46],
                skew: 22,
                advance: 39,
                alpha: 63,
            }
        );
        assert_eq!(
            layout(&number, 3, [320, 224], [15, 22]).position,
            [276, 156]
        );
    }

    #[test]
    fn zero_retains_one_digit_and_fading_offset() {
        let mut number = FloatingNumber::default();
        number.alpha = 247;
        number.offset = [1, -24];
        assert_eq!(
            layout(&number, 1, [320, 224], [13, 19]),
            Layout {
                position: [321, 180],
                size: [13, 19],
                skew: 10,
                advance: 13,
                alpha: 247,
            }
        );
    }
}
