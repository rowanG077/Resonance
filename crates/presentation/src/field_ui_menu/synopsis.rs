use super::*;
use resonance_content::menu_data::{SYNOPSIS_LIST_ROWS, SYNOPSIS_TEXT_ROWS};

impl Drawing<'_> {
    pub(super) fn synopsis(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.synopsis;
        let (entry, record) = menu.synopsis_entry();
        let catalog = &menu.resources.as_ref().unwrap().data.synopsis;
        let fade = i32::from(state.transition.page_fade);
        self.offset = [0., -(fade * 76 / 256) as f32];
        self.heading(&self.spec.labels["synopsis"])?;
        self.offset = [0., -(fade * 132 / 256) as f32];
        self.frame([304., 60., 316., 60.]);
        self.text(&entry.heading, [312., 64.], 16., WHITE)?;
        self.text("Lv", [312., 92.], 16., GOLD)?;
        let level = record
            .level
            .map_or_else(|| " --".into(), |v| format!("{v:3}"));
        self.text(
            &level,
            [312. + self.text_width("Lv", 16.)?, 92.],
            16.,
            WHITE,
        )?;
        let date = record
            .recorded_at
            .and_then(|v| time::OffsetDateTime::from_unix_timestamp(v).ok())
            .map_or_else(
                || "--".into(),
                |date| {
                    format!(
                        "{} {:2}, {}",
                        catalog.months[date.month() as usize - 1],
                        date.day(),
                        date.year()
                    )
                },
            );
        self.text(&date, [408., 92.], 16., WHITE)?;
        self.offset = [(fade * 344 / 256) as f32, 0.];
        if let Some(location) = &entry.location {
            self.opacity = 255;
            self.quad(FONT, [328., 148., 600., 356.], [0.5; 4], [0., 0., 0., 1.]);
            self.opacity = 255 - state.transition.page_fade;
            let index = WORLD_MAPS + usize::from(location.world);
            let texture = &self.spec.textures[index - 1];
            self.quad(
                index,
                [336., 156., 592., 348.],
                [0., 0., texture.width as f32, texture.height as f32],
                [1.; 4],
            );
        }
        let list_offset = -(fade * 280 / 256) as f32;
        self.offset = [list_offset, 0.];
        self.frame([16., 60., 276., 368.]);
        let records = menu.synopsis_records();
        self.text_size(
            &format!("{}/{}", state.row + 1, records.len()),
            [32., 60.],
            [16.; 2],
            WHITE,
        )?;
        let y = 84. + (state.row - state.first) as f32 * 28.;
        self.highlight([32., y, 240., 24.], if state.reading { 127 } else { 255 });
        let starts = self.vertex_counts();
        let first = state.first - usize::from(state.list_scroll > 0);
        let offset = scroll_offset(state.list_scroll, 28);
        for (row, &id) in records
            .iter()
            .skip(first)
            .take(SYNOPSIS_LIST_ROWS + usize::from(state.list_scroll != 0))
            .enumerate()
        {
            let record = &menu.checkpoint.as_ref().unwrap().progress.event_records[&id];
            self.text(
                &catalog.entries[usize::from(id)].title,
                [32., 84. + row as f32 * 28. - offset as f32],
                20.,
                if record.value == 3 { WHITE } else { 6 },
            )?;
        }
        self.clip_rows(starts, [84., 420.]);
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [142., 68.]);
        }
        if state.first + SYNOPSIS_LIST_ROWS < records.len() {
            self.scroll_arrow(SCROLL_DOWN, [142., 416.]);
        }
        self.plane = 2;
        self.offset = [(fade * 344 / 256) as f32, 0.];
        self.opacity = 255;
        if let Some(point) = entry.location.as_ref().and_then(|p| p.point) {
            let phase = (self.tick % 60) * 512 / 60;
            let alpha = if phase >= 256 { 511 - phase } else { phase } as f32 / 255.;
            let [x, y] = [336. + f32::from(point[0]), 156. + f32::from(point[1])];
            self.quad(
                FONT,
                [x - 1., 156., x + 1., 348.],
                [0.5; 4],
                [1., 1., 1., alpha],
            );
            self.quad(
                FONT,
                [336., y - 1., 592., y + 1.],
                [0.5; 4],
                [1., 1., 1., alpha],
            );
        }
        self.offset = [0.; 2];
        self.opacity = 255 - state.transition.page_fade;
        Ok([32. + list_offset, y + 12.])
    }

    pub(super) fn synopsis_text(&mut self, menu: &Menu) -> Result<()> {
        if !menu.synopsis.reading {
            return Ok(());
        }
        let (entry, record) = menu.synopsis_entry();
        let lines = entry.lines(record.value);
        let height = lines.len().min(SYNOPSIS_TEXT_ROWS) as f32 * 27. + 17.;
        let top = 428. - height;
        let state = &menu.synopsis;
        let top = top + ((456. - top) * f32::from(255 - state.text_opacity) / 256.).trunc();
        self.opacity = state.text_opacity / 2;
        self.plane = 2;
        self.quad(FONT, [0., 0., 640., 448.], [0.5; 4], [0., 0., 0., 1.]);
        self.opacity = state.text_opacity;
        self.shade([16., top, 620., top + height]);
        self.plane = 3;
        self.frame([16., top, 604., height]);
        let starts = self.vertex_counts();
        let first = state.line - usize::from(state.text_scroll > 0);
        let offset = scroll_offset(state.text_scroll, 27);
        let visible = SYNOPSIS_TEXT_ROWS + usize::from(state.text_scroll != 0);
        for (row, line) in lines.iter().skip(first).take(visible).enumerate() {
            let y = top + 4. + row as f32 * 27. - offset as f32;
            let mut x = 24.;
            for span in &line.0 {
                self.text(&span.text, [x, y], 24., usize::from(span.color))?;
                x += self.text_width(&span.text, 24.)?;
            }
            if first + row + 1 < lines.len() {
                self.opacity = state.text_opacity / 2;
                self.quad(
                    FONT,
                    [24., y + 25., 600., y + 27.],
                    [0.5; 4],
                    [32. / 255., 32. / 255., 32. / 255., 1.],
                );
                self.opacity = state.text_opacity;
            }
        }
        self.clip_rows(starts, [top + 4., top + 355.]);
        let arrow_line = if (1..4).contains(&state.text_scroll) {
            first
        } else {
            state.line
        };
        if arrow_line > 0 {
            self.scroll_arrow(SCROLL_UP, [306., top - 12.]);
        }
        if first + visible < lines.len() {
            self.scroll_arrow(SCROLL_DOWN, [306., top + height - 8.]);
        }
        self.opacity = 255;
        Ok(())
    }
}
