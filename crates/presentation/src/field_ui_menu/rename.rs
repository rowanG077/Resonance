use super::*;
use resonance_game::menu::rename::Focus;

impl Drawing<'_> {
    pub(super) fn rename(
        &mut self,
        menu: &Menu,
        cursor: &resonance_content::font::UiTexture,
    ) -> Result<[f32; 2]> {
        let state = &menu.rename;
        if state.pending {
            return Ok([0.; 2]);
        }
        let data = &menu
            .resources
            .as_ref()
            .context("name editor data")?
            .data
            .rename;
        let fade = u32::from(state.fade);
        self.opacity = 255 - state.fade;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&data.heading)?;
        self.offset = [0.; 2];
        if state.focus == Focus::Name {
            let x = 416. + (fade * 224 / 256) as f32;
            for (label, button, y) in [(&data.delete, 10, 58.), (&data.default, 12, 84.)] {
                self.sprite(self.spec.sprites.buttons[button], [x, y]);
                self.text(label, [x + 24., y], 24., WHITE)?;
            }
        }
        let y = 148. + (fade * 308 / 256) as f32;
        self.framed([140., y - 4., 344., 232.], false);
        let key_position = |col: usize, row: usize| {
            [
                156. + col as f32 * 24. + (col / 5) as f32 * 4. + (col / 10) as f32 * 8.,
                y + row as f32 * 28.,
            ]
        };
        let [kx, ky] = key_position(state.column, state.row);
        if state.focus == Focus::Keyboard {
            self.highlight([kx, ky, 20., 24.], 255);
        }
        for (i, c) in data.keyboard.chars().enumerate() {
            self.text(&c.to_string(), key_position(i % 13, i / 13), 20., WHITE)?;
        }
        let x = 64. - (fade * 304 / 256) as f32;
        self.framed([x - 16., 64., 272., 38.], false);
        let alpha = if state.focus == Focus::Name { 255 } else { 127 };
        let nx = x + state.position as f32 * 40.;
        self.highlight([nx, 68., 32., 32.], alpha);
        for (i, c) in state.value.chars().take(6).enumerate() {
            self.text_size(&c.to_string(), [x + i as f32 * 40., 68.], [32.; 2], WHITE)?;
        }
        let mut anchor = if state.focus == Focus::Keyboard {
            [kx, ky + 8.]
        } else {
            [nx, 84.]
        };
        if state.focus != Focus::Name {
            self.cursor([nx, 84.], cursor, alpha);
        }
        let mut x = 320. + (fade * 320 / 256) as f32;
        self.framed([x - 8., 20., 324., 32.], false);
        for (i, command) in data.commands.iter().enumerate() {
            let width = self.text_width(command, 24.)?;
            if state.focus == Focus::Commands && state.command == i {
                self.highlight([x, 24., width, 24.], 255);
                anchor = [x, 32.];
            }
            self.text(command, [x, 24.], 24., WHITE)?;
            x += width + 12.;
        }
        self.opacity = 255;
        Ok(anchor)
    }
}
