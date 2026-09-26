//! Source69838/69728 records and byte-verified68C90 composition.
use super::{Art, Batch, BitmapFont, results};
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy)]
pub(super) enum NoticeKind {
    Defeat,
    Escape,
}

#[derive(Clone, Copy)]
pub(super) enum NoticeOwner {
    Party(u8),
    Enemy(u8),
}

pub(super) struct Notice {
    text: String,
    owner: Option<NoticeOwner>,
    side: u8,
    kind: u8,
    tick: u32,
    remaining: i16,
    alpha: u8,
    offset: i8,
    left: i32,
    columns: i32,
    remainder: i32,
}

#[derive(Default)]
pub(super) struct NoticeDraw {
    pub shadow: Batch,
    pub owner: Option<(NoticeOwner, Batch)>,
    pub background: Batch,
    pub text: Batch,
    pub generic_icon: Batch,
    pub symbol: Option<(usize, Batch)>,
}

impl Notice {
    pub fn new(
        kind: NoticeKind,
        owner: Option<u8>,
        art: &Art,
        font: &BitmapFont,
        tick: u32,
    ) -> Result<Self> {
        Self::request(
            &art.overlays.notice_texts[kind as usize],
            owner.map(NoticeOwner::Party),
            2,
            3,
            -1,
            font,
            tick,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request(
        text: &str,
        owner: Option<NoticeOwner>,
        side: u8,
        kind: u8,
        remaining: i16,
        font: &BitmapFont,
        tick: u32,
    ) -> Result<Self> {
        ensure!(
            side <= 2 && (remaining > 0 || remaining == -1),
            "invalid battle notice lifetime"
        );
        let mut width = 0i32;
        for character in text.chars() {
            width += font
                .glyphs
                .get(&character)
                .context("missing battle notice glyph")?
                .advance as i32;
        }
        let window_width = ((width / 24) + 1) * 22 + 4;
        let drawn_width = width * 22 / 24;
        Ok(Self {
            text: text.to_owned(),
            owner,
            side,
            kind,
            tick,
            remaining,
            alpha: 0,
            offset: 16,
            left: 320 - (window_width >> 1),
            columns: drawn_width / 24 + 1,
            remainder: drawn_width % 24,
        })
    }

    pub fn advance(&mut self, tick: u32, held: bool) {
        if tick == self.tick {
            return;
        }
        self.tick = tick;
        if held {
            return;
        }
        if self.remaining > 0 || self.remaining == -1 {
            self.alpha = self.alpha.saturating_add(32);
            if self.remaining != -1 {
                self.remaining -= 1;
            }
        } else {
            self.offset = self
                .offset
                .wrapping_add(if self.side == 0 { -1 } else { 1 });
            self.alpha = self.alpha.saturating_sub(16);
        }
    }

    pub fn draw(&self, art: &Art, font: &BitmapFont, color: [u8; 4]) -> Result<NoticeDraw> {
        self.draw_row(art, font, color, 36., false)
    }

    pub fn draw_row(
        &self,
        art: &Art,
        font: &BitmapFont,
        color: [u8; 4],
        y: f32,
        older: bool,
    ) -> Result<NoticeDraw> {
        let mut out = NoticeDraw::default();
        if self.alpha == 0 {
            return Ok(out);
        }
        let alpha = if older {
            (self.alpha >> 1) + (self.alpha >> 2)
        } else {
            self.alpha
        };
        let x = (self.left + i32::from(self.offset)) as f32;
        let count = self.columns;
        let shade = [
            color[0] >> 1,
            color[1] >> 1,
            color[2] >> 1,
            (alpha >> 1) + (alpha >> 2),
        ];
        let plain = [128, 128, 128, alpha];
        for column in 0..count {
            sprite(
                &mut out.shadow,
                [x - 8. + (column * 24) as f32, y + 10., 24., 32.],
                [448., 464., 24., 32.],
                shade,
            );
        }
        sprite(
            &mut out.shadow,
            [x - 8. + (count * 24) as f32, y + 10., 12., 32.],
            [448., 464., 12., 32.],
            shade,
        );
        // Original69058..69078: the left endpoint follows the signed offset.
        // The partial C incorrectly places this endpoint at the right edge.
        sprite(
            &mut out.shadow,
            [x - 48., y - 1., 40., 48.],
            [408., 464., 40., 48.],
            shade,
        );
        sprite(
            &mut out.shadow,
            [x + (count * 24) as f32, y + 4., 32., 40.],
            [480., 456., 32., 40.],
            shade,
        );
        if let Some(owner) = self.owner {
            let (rect, uv) = match owner {
                NoticeOwner::Party(character) => {
                    ensure!((1..=9).contains(&character), "invalid notice character");
                    let [u, v, w, h] = art.results.character_icons[usize::from(character - 1)]
                        .rect
                        .map(|v| v as f32);
                    ([x - 44., y + 2., w, h], [u, v, w, h])
                }
                NoticeOwner::Enemy(group) => {
                    ensure!(group < 4, "invalid notice enemy group");
                    ([x - 44., y + 2., 32., 32.], [0., 0., 32., 32.])
                }
            };
            let mut batch = Batch::default();
            sprite(&mut batch, rect, uv, plain);
            out.owner = Some((owner, batch));
        }
        for column in 0..count {
            sprite(
                &mut out.background,
                [x - 8. + (column * 24) as f32, y + 10., 24., 32.],
                [416. + ((column % 4) * 24) as f32, 376., 24., 32.],
                plain,
            );
        }
        sprite(
            &mut out.background,
            [x - 8. + (count * 24) as f32, y + 10., 12., 32.],
            [416. + ((count % 4) * 24) as f32, 376., 12., 32.],
            plain,
        );
        sprite(
            &mut out.background,
            [x - 64., y - 9., 56., 48.],
            [456., 408., 56., 48.],
            plain,
        );
        sprite(
            &mut out.background,
            [x - 9. + (count * 24) as f32, y + 4., 40., 40.],
            [416., 408., 40., 40.],
            plain,
        );
        let mut text_color = art.overlays.notice_text_color;
        text_color[3] = alpha;
        results::dol_text(
            &mut out.text,
            font,
            &self.text,
            [x + 2. - (self.remainder >> 1) as f32, y + 2.],
            [22., 28.],
            22.,
            0.,
            text_color,
        )?;
        if self.owner.is_none() {
            sprite(
                &mut out.generic_icon,
                [x - 44., y + 2., 32., 32.],
                [384., 384., 32., 32.],
                plain,
            );
        }
        if let Some(symbol) = art.overlays.notice_symbols.get(usize::from(self.kind)) {
            let mut batch = Batch::default();
            sprite(
                &mut batch,
                [x - 3. + (count * 24) as f32, y + 12., 24., 24.],
                symbol.rect.map(|v| v as f32),
                plain,
            );
            out.symbol = Some((usize::from(self.kind), batch));
        }
        Ok(out)
    }
}

fn sprite(batch: &mut Batch, [x, y, w, h]: [f32; 4], [u, v, tw, th]: [f32; 4], color: [u8; 4]) {
    super::quad(
        batch,
        [x, y, x + w, y + h],
        [u, v, u + tw, v + th],
        0.,
        [color; 4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notice_hold_preserves_fade_and_lifetime_on_selector_release_visit() {
        let mut notice = Notice {
            text: String::new(),
            owner: None,
            side: 0,
            kind: 1,
            tick: 0,
            remaining: 2,
            alpha: 0,
            offset: 16,
            left: 320,
            columns: 1,
            remainder: 0,
        };
        notice.advance(1, false);
        assert_eq!((notice.alpha, notice.remaining, notice.offset), (32, 1, 16));
        notice.advance(2, true);
        notice.advance(2, false);
        assert_eq!((notice.alpha, notice.remaining, notice.offset), (32, 1, 16));
        notice.advance(3, false);
        notice.advance(4, false);
        assert_eq!((notice.alpha, notice.remaining, notice.offset), (48, 0, 15));
    }
}
