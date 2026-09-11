use super::*;
use resonance_content::menu_data::MenuSpan;

impl Drawing<'_> {
    pub(super) fn manual(
        &mut self,
        menu: &Menu,
        cursor: &resonance_content::font::UiTexture,
    ) -> Result<[f32; 2]> {
        let state = &menu.manual;
        let fade = u32::from(state.page_fade);
        let left = -((fade * 240 / 256) as f32);
        let right = (fade * 620 / 256) as f32;
        let bottom = (fade * 144 / 256) as f32;
        self.opacity = 255 - state.page_fade;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&menu.resources.as_ref().unwrap().data.manual.title)?;
        self.offset = [0., bottom];
        self.framed([16., 312., 604., 116.], true);
        self.offset = [left, 0.];
        self.frame([16., 60., 256., 242.]);
        self.offset = [right, 0.];
        self.frame([282., 60., 338., 242.]);
        self.offset = [left, 0.];
        let chapters = menu.manual_chapters();
        let chapter_y = 66. + state.chapter as f32 * 26.;
        let dim = if state.reading { 127 } else { 255 };
        self.highlight([32., chapter_y, 216., 24.], dim);
        for (row, (chapter, _)) in chapters.iter().enumerate() {
            self.text(&chapter.name, [32., 66. + row as f32 * 26.], 20., WHITE)?;
        }
        let chapter_cursor = [32., chapter_y + 8.];
        let Some((_, topics)) = chapters.get(state.chapter) else {
            self.offset = [0.; 2];
            self.opacity = 255;
            return Ok([chapter_cursor[0] + left, chapter_cursor[1]]);
        };
        let topic_y = 66. + state.topic as f32 * 26.;
        if state.reading {
            self.offset = [right, 0.];
            self.highlight([298., topic_y, 308., 24.], 255);
            self.offset = [left, 0.];
            self.cursor(chapter_cursor, cursor, 127);
            self.offset = [0., bottom];
            let topic = &topics[state.topic];
            for (row, line) in topic.paragraphs[state.paragraph].lines.iter().enumerate() {
                let mut x = 32.;
                let y = 320. + row as f32 * 26.;
                for span in line {
                    match span {
                        MenuSpan::Text { text, color } => {
                            self.text(text, [x, y], 20., usize::from(*color))?;
                            x += self.text_width(text, 20.)?;
                        }
                        MenuSpan::Button { sprite } => {
                            let rect = self.spec.sprites.buttons[usize::from(*sprite)];
                            self.sprite_rect(rect, [x, y, x + 24., y + 24.], [1.; 4]);
                            x += 24.;
                        }
                    }
                }
            }
            if state.paragraph > 0 {
                self.scroll_arrow(SCROLL_UP, [306., 304.]);
            }
            if state.paragraph + 1 < topic.paragraphs.len() {
                self.scroll_arrow(SCROLL_DOWN, [306., 420.]);
            }
        }
        self.offset = [right, 0.];
        for (row, topic) in topics.iter().enumerate() {
            self.text(&topic.name, [298., 66. + row as f32 * 26.], 20., WHITE)?;
        }
        self.offset = [0.; 2];
        self.opacity = 255;
        Ok(if state.reading {
            [298. + right, topic_y + 6.]
        } else {
            [chapter_cursor[0] + left, chapter_cursor[1]]
        })
    }
}
