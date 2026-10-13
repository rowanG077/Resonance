//! Battle command emblems and selectors on one packed sprite atlas.
use super::*;
use resonance_content::battle_ui::CommandArt;
use resonance_game::battle::command::{Command, Frame, View};

const FIRST_BUTTON: f32 = 152.;
const BUTTON_PITCH: f32 = 56.;
const BUTTON_TOP: f32 = 128.;
const BUTTON_SIZE: f32 = 48.;
const SELECTED_INSET: f32 = 8.;
const WHITE: [u8; 4] = [128, 128, 128, 255];

#[derive(Default)]
pub(super) struct Motion {
    selected: Option<Command>,
    since: u32,
    trail: crate::choice_cursor::Trail,
}

pub(super) fn validate_font(art: &CommandArt, font: &BitmapFont) -> Result<()> {
    for text in art.names.iter().chain([&art.cancel_escape]) {
        font.validate_text(text)?;
    }
    Ok(())
}

impl Artwork {
    pub fn hide_commands(&mut self, commands: &mut Commands) {
        for layer in &mut self.ui.commands {
            layer.show(false, commands);
        }
        self.command_motion = Default::default();
    }

    pub fn render_commands(
        &mut self,
        state: Option<&Frame>,
        tick: u32,
        pending: Option<resonance_battle::item::Release>,
        characters: &[u8],
        _user_name: Option<&str>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let pending = pending
            .filter(|_| {
                state.is_some_and(|state| {
                    state.selected == Command::Items && matches!(state.view, View::Strip)
                })
            })
            .map(|release| -> Result<_> {
                let character = characters
                    .get(release.user.index())
                    .and_then(|id| id.checked_sub(1))
                    .context("pending item user has no prepared character")?;
                Ok((
                    usize::from(character),
                    self.menu_data.item_text(release.item)?.name.as_str(),
                ))
            })
            .transpose()?;
        let batches = draw(
            state,
            tick,
            pending,
            &self.art,
            &self.font,
            &mut self.command_motion,
        )?;
        for (layer, batch) in self.ui.commands.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

fn sine(degrees: u32) -> f32 {
    ((degrees % 360) as f32).to_radians().sin()
}
fn half_sine(degrees: u32) -> f32 {
    sine(degrees % 180)
}
fn button_center(command: usize) -> f32 {
    FIRST_BUTTON + BUTTON_PITCH * command as f32 + BUTTON_SIZE / 2.
}

fn sprite(batch: &mut Batch, [x, y, w, h]: [f32; 4], [u, v, uw, vh]: [u16; 4], color: [u8; 4]) {
    batch.quad(
        [x, y, x + w, y + h],
        [u as f32, v as f32, (u + uw) as f32, (v + vh) as f32],
        hud_color(color),
    );
}

fn emblem(
    batch: &mut Batch,
    art: &CommandArt,
    command: usize,
    selected: bool,
    enabled: bool,
    phase: u32,
    tick: u32,
) {
    let inset = if selected { SELECTED_INSET } else { 0. };
    let x = FIRST_BUTTON + BUTTON_PITCH * command as f32 - inset;
    let y = BUTTON_TOP - inset;
    let size = BUTTON_SIZE + 2. * inset;
    sprite(
        batch,
        [x, y, size + 4., size + 4.],
        art.shadow,
        art.shadow_color,
    );
    sprite(batch, [x, y, size, size], art.background, WHITE);
    let motion = &art.motion;
    let phase = if selected && enabled { phase } else { 0 };
    let shade = if selected {
        motion.selected_shade[0] + motion.selected_shade[1] * sine(tick.wrapping_mul(16))
    } else {
        128.
    } as u8;
    let count = if command == 1 {
        3
    } else if command == 5 {
        2
    } else {
        art.icons[command].len()
    };
    for part in 0..count {
        let mut at = [x, y];
        let mut rotation = 0.;
        let mut pivot = [0., 0.];
        let mut tilted = false;
        let mut frame = if command == 5 { 0 } else { part };
        match command {
            0 if selected => {
                if part == 0 {
                    rotation = phase as f32 * 8.;
                    tilted = true;
                } else {
                    at[1] +=
                        (motion.bob * sine(phase * 6 + if part == 1 { 180 } else { 0 })).trunc();
                }
            }
            1 if selected => {
                if part < 2 {
                    at[1] -= (motion.bob * half_sine(phase * 6)).trunc();
                } else {
                    frame += (tick / 6 % 3) as usize;
                }
            }
            2 => {
                if part == 3 && (!selected || (phase + 30) / 18 & 1 != 0) {
                    continue;
                }
                if selected && part == 0 {
                    at[0] -= 5.;
                    rotation = motion.strategy_rotation * sine(phase * 10);
                    pivot = [2., 2.];
                    tilted = true;
                }
                if selected && part == 2 {
                    at[1] += (motion.small_bob * sine(phase * 8)).trunc();
                }
            }
            3 if selected => {
                at[1] +=
                    (motion.lift * half_sine(phase * 6 + if part == 1 { 90 } else { 0 })).trunc();
            }
            4 if selected => match part {
                0 => at[1] += (motion.lift * half_sine(phase * 6)).trunc(),
                1 => {
                    at[0] += 12.;
                    at[1] += 24.;
                    rotation = motion.label_bob * sine(phase * 5);
                    pivot = [-14., -24.];
                    tilted = true;
                }
                _ => {
                    at[0] -= 10.;
                    at[1] += 16. + (motion.small_bob * sine(phase * 3)).trunc();
                    rotation = motion.item_sway * sine(phase * 6);
                    pivot = [8., -16.];
                    tilted = true;
                }
            },
            5 => {
                at[1] -= 8.;
                rotation = if selected {
                    motion.escape_rotation * sine(phase * 8 + part as u32 * 180)
                } else {
                    0.
                };
                pivot = [0., 8.];
                tilted = true;
            }
            _ => {}
        }
        sprite(
            batch,
            [at[0], at[1], size, size],
            art.icons[command][frame],
            [shade, shade, shade, 255],
        );
        if tilted {
            let center = [at[0] + size / 2. - 320., 240. - at[1] - size / 2.];
            let (sin, cos) = rotation.to_radians().sin_cos();
            let tilt = motion.tilt.to_radians().cos();
            for point in batch.positions.iter_mut().rev().take(4) {
                let x = point[0] - center[0] + pivot[0];
                let y = center[1] - point[1] + pivot[1];
                point[0] = center[0] + tilt * (cos * x - sin * y);
                point[1] = center[1] - sin * x - cos * y;
            }
        }
    }
    if !enabled {
        sprite(batch, [x, y, size, size], art.disabled, WHITE);
    }
}

fn nameplate(
    batch: &mut Batch,
    art: &CommandArt,
    center: f32,
    width: f32,
    offset: f32,
    color: [u8; 4],
) {
    // Caps enclose 24-pixel cells; the middle pair carries the downward pointer.
    let cells = (width / 24.).floor() as usize + 1;
    let left =
        center - 40. - offset - cells.saturating_sub(2) as f32 * 12. - (24. - width % 24.) / 2.;
    let y = 80. - offset;
    let [u, v, _, _] = art.plate;
    sprite(batch, [left, y, 16., 40.], [u, v, 16, 40], color);
    let mut x = left + 16.;
    for cell in 0..cells * 2 {
        let middle = cells;
        let source = if cell + 1 == middle {
            28
        } else if cell == middle {
            40
        } else {
            16
        };
        sprite(batch, [x, y, 12., 40.], [u + source, v, 12, 40], color);
        x += 12.;
    }
    sprite(batch, [x, y, 16., 40.], [u + 64, v, 16, 40], color);
}

fn label(
    batch: &mut Batch,
    font: &BitmapFont,
    text: &str,
    [mut x, y]: [f32; 2],
    [width, height]: [f32; 2],
    spacing: f32,
    skew: f32,
    color: [u8; 4],
) -> Result<()> {
    for character in text.chars() {
        if character == ' ' {
            x += (spacing / 2.).trunc();
            continue;
        }
        let glyph = font
            .glyphs
            .get(&character)
            .context("missing battle command glyph")?;
        let [u, v, w, h] = glyph.rect.map(|v| v as f32);
        party::slanted_quad(
            batch,
            [x, y, x + width, y + height],
            [u, v, u + w, v + h],
            skew,
            [color; 4],
        );
        x += if resonance_content::font::is_single_byte(character) {
            (glyph.advance as f32 * spacing / font.line_height as f32).trunc()
        } else {
            spacing
        };
    }
    Ok(())
}

fn draw(
    state: Option<&Frame>,
    tick: u32,
    pending: Option<(usize, &str)>,
    art: &Art,
    font: &BitmapFont,
    motion: &mut Motion,
) -> Result<[Batch; 4]> {
    let mut panels = Batch::default();
    let mut sprites = Batch::default();
    let mut labels = Batch::default();
    let mut numbers = Batch::default();
    let Some(state) =
        state.filter(|s| matches!(s.view, View::Strip | View::User(_) | View::Ally(_)))
    else {
        *motion = Default::default();
        return Ok([panels, sprites, labels, numbers]);
    };
    ensure!(state.controller < 4, "invalid command controller");
    let selected = Command::ALL
        .iter()
        .position(|&command| command == state.selected)
        .unwrap();
    if motion.selected != Some(state.selected) {
        motion.selected = Some(state.selected);
        motion.since = tick;
    }
    let phase = tick.wrapping_sub(motion.since) % 360;
    let a = &art.commands;
    // Selected artwork overlaps its neighbors and is therefore submitted last.
    for command in (0..6).filter(|&i| i != selected).chain([selected]) {
        emblem(
            &mut sprites,
            a,
            command,
            command == selected,
            state.enabled & Command::ALL[command].mask() != 0,
            phase,
            tick,
        );
    }
    let name = if state.selected == Command::Escape && state.escape_requested {
        &a.cancel_escape
    } else {
        &a.names[selected]
    };
    let width = name
        .chars()
        .map(|c| font.glyphs[&c].advance as f32 * 24. / font.line_height as f32)
        .sum::<f32>();
    let center = button_center(selected);
    nameplate(&mut sprites, a, center, width, -4., a.shadow_color);
    nameplate(&mut sprites, a, center, width, 0., WHITE);
    let cells = (width / 24.).floor() + 1.;
    label(
        &mut labels,
        font,
        name,
        [
            center - cells * 12.,
            76. - (a.motion.label_bob * half_sine(tick.wrapping_mul(6))).trunc(),
        ],
        [22., 32.],
        24.,
        0.,
        a.text_color,
    )?;
    let mut cursor = [center - 44., 172.];
    if let View::User(selection) | View::Ally(selection) = &state.view {
        ensure!(selection.slot < 4, "invalid item selector slot");
        sprite(
            &mut sprites,
            [cursor[0], cursor[1], 32., 32.],
            a.cursor,
            [128, 128, 128, 128],
        );
        let [left, top, _, _] = party::card_rect(usize::from(selection.slot));
        cursor = [left - 20., top + 44.];
        if !selection.eligible {
            sprite(&mut sprites, [left, top, 64., 64.], a.disabled, WHITE);
        }
        party::slanted_quad(
            &mut panels,
            [left + 8., 388., left + 104., 406.],
            [0.5; 4],
            0.,
            a.selector_plate_colors,
        );
        let units = selection
            .name
            .chars()
            .map(|c| {
                if resonance_content::font::is_single_byte(c) {
                    1
                } else {
                    2
                }
            })
            .sum::<usize>()
            / 2;
        let position = [
            left + 52. - (14. * units as f32 + 4.) / 2.,
            380. - (a.motion.selector_bob * half_sine(tick.wrapping_mul(8))).trunc(),
        ];
        for (offset, color) in [(2., a.shadow_color), (0., a.text_color)] {
            label(
                &mut labels,
                font,
                &selection.name,
                [position[0] + offset, position[1] + offset],
                [16., 24.],
                16.,
                2.,
                color,
            )?;
        }
        let role = &a.selector_labels[usize::from(matches!(state.view, View::Ally(_)))];
        for (offset, colors) in [
            (2., [a.shadow_color; 2]),
            (0., a.selector_colors[usize::from(!selection.eligible)]),
        ] {
            text::italic(
                &mut numbers,
                &art.font,
                role,
                [left + 20. + offset, 454. + offset],
                [16., 16.],
                13.,
                0.,
                colors,
            )?;
        }
    }
    let bob = (a.motion.cursor_bob * half_sine(tick.wrapping_mul(8))).trunc();
    cursor[0] += bob;
    cursor[1] -= bob;
    for ([x, y], alpha) in motion.trail.sample(tick, cursor.map(|v| v as i32)) {
        sprite(
            &mut sprites,
            [x as f32, y as f32, 32., 32.],
            a.cursor,
            [128, 128, 128, alpha],
        );
    }
    sprite(
        &mut sprites,
        [cursor[0] + 4., cursor[1] + 4., 32., 32.],
        a.cursor_shadow,
        a.shadow_color,
    );
    sprite(
        &mut sprites,
        [cursor[0], cursor[1], 32., 32.],
        a.cursor,
        WHITE,
    );
    let player = a
        .player_format
        .replacen("%d", &(state.controller + 1).to_string(), 1);
    for (offset, colors) in [(2., [a.shadow_color; 2]), (0., a.player_colors)] {
        text::italic(
            &mut numbers,
            &art.font,
            &player,
            [center - 14. + offset, 184. + offset],
            [20., 20.],
            18.,
            2.,
            colors,
        )?;
    }
    if let Some((character, name)) = pending {
        let face = *a
            .faces
            .get(character)
            .context("pending item character is missing")?;
        let width = name.chars().count() as f32 * 9.;
        let x = 405. - width / 2.;
        panels.quad(
            [x - 34., 210., x + width + 2., 230.],
            [0.5; 4],
            hud_color(a.pending_item_color),
        );
        sprite(&mut sprites, [x - 32., 204., 32., 32.], face, WHITE);
        for (offset, color) in [(2., a.shadow_color), (0., a.text_color)] {
            label(
                &mut labels,
                font,
                name,
                [x + offset, 208. + offset],
                [18., 24.],
                18.,
                0.,
                color,
            )?;
        }
    }
    Ok([panels, sprites, labels, numbers])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires current cooked battle artwork; CPU command drawing only"]
    fn command_emblems_selection_and_disabled_overlay_use_prepared_art() -> Result<()> {
        let art: Art = serde_json::from_slice(&std::fs::read(
            fixtures::asset_root().join(resonance_content::battle_ui::PATH),
        )?)?;
        let font = fixtures::dialogue_font()?;
        let mut state = Frame {
            actor: resonance_battle::ActorId::from_index(0)?,
            selected: Command::Tech,
            enabled: u8::MAX,
            controller: 0,
            connected: [true, false, false, false],
            view: View::Strip,
            escape_requested: false,
        };
        let mut motion = Motion::default();
        let visible = draw(Some(&state), 0, None, &art, &font, &mut motion)?;
        assert!(!visible[1].indices.is_empty());
        state.enabled &= !Command::Tech.mask();
        let disabled = draw(Some(&state), 0, None, &art, &font, &mut motion)?;
        assert_eq!(disabled[1].positions.len(), visible[1].positions.len() + 4);
        state.selected = Command::Items;
        let items = draw(
            Some(&state),
            1,
            Some((0, "Apple Gel")),
            &art,
            &font,
            &mut motion,
        )?;
        assert!(items[2] != visible[2]);
        assert!(
            draw(None, 2, None, &art, &font, &mut motion)?
                .iter()
                .all(|b| b.indices.is_empty())
        );
        Ok(())
    }
}
