//! Sliding quick orders and the enemy-name list shown by battle selection.
use super::*;
use resonance_battle::ActorId;

const SLIDE_SPEED: f32 = 16.;
const ORDER_ENTRY: f32 = -160.;
const ORDER_NOTICE_TICKS: u32 = 60;
const ENEMY_SHOWN: f32 = 408.;
const ENEMY_HIDDEN: f32 = 656.;
const WHITE: [u8; 4] = [128, 128, 128, 255];
const SHADOW: [u8; 4] = [0, 0, 0, 128];
const CANCEL_BACKDROP: [u8; 4] = [96, 32, 32, 160];
const PRESET_BACKDROP: [u8; 4] = [32, 96, 32, 160];
const ENEMY_BACKDROP: [[u8; 4]; 4] = [
    [96, 32, 32, 160],
    [96, 64, 32, 160],
    [96, 32, 32, 160],
    [96, 64, 32, 160],
];

#[derive(Clone, Copy)]
pub(crate) struct Input<'a> {
    pub tick: u32,
    pub menu_open: bool,
    /// A selected enemy, rather than the actor owning the target selector.
    pub target: Option<ActorId>,
    /// Cancel, left preset, upper preset, right preset.
    pub order: u8,
    pub order_revision: u32,
    pub order_label: &'a str,
}

#[derive(Default)]
pub(super) struct State {
    tick: Option<u32>,
    order_revision: u32,
    notice_since: Option<u32>,
    orders_x: f32,
    enemies_x: f32,
    presented: [f32; 2],
    phase: u32,
}

impl State {
    fn advance(&mut self, input: Input<'_>, order_characters: usize) {
        if self.tick == Some(input.tick) {
            return;
        }
        let hidden = -56. - order_characters as f32 * 24.;
        let elapsed = self.tick.map_or(1, |tick| input.tick.wrapping_sub(tick));
        if self.tick.is_none() {
            self.orders_x = hidden;
            self.enemies_x = ENEMY_HIDDEN;
        }
        if input.order_revision != self.order_revision {
            self.order_revision = input.order_revision;
            self.notice_since = Some(input.tick);
            self.orders_x = ORDER_ENTRY;
        }
        // Present the completed placement, then advance the next one. Repeated
        // readbacks retain both its position and animation phase.
        self.presented = [self.orders_x, self.enemies_x];
        self.phase = self.tick.unwrap_or(input.tick);
        let notice = self
            .notice_since
            .is_some_and(|since| input.tick.wrapping_sub(since) < ORDER_NOTICE_TICKS);
        let distance = SLIDE_SPEED * elapsed as f32;
        self.orders_x = if input.menu_open || notice {
            (self.orders_x + distance).min(0.)
        } else {
            (self.orders_x - distance).max(hidden)
        };
        self.enemies_x = if input.menu_open || input.target.is_some() {
            (self.enemies_x - distance).max(ENEMY_SHOWN)
        } else {
            (self.enemies_x + distance).min(ENEMY_HIDDEN)
        };
        self.tick = Some(input.tick);
    }

    /// Batches use the solid surface, dialogue font, and existing battle font atlas.
    pub(super) fn draw(
        &mut self,
        input: Input<'_>,
        font: &BitmapFont,
        enemies: &[EnemyHud],
        frame: &BattleFrame,
    ) -> Result<[Batch; 3]> {
        ensure!(input.order <= 3, "invalid quick order selection");
        let [mut panels, mut labels, mut arrows] = std::array::from_fn(|_| Batch::default());
        if frame.recognized_result.is_some() {
            *self = Self::default();
            return Ok([panels, labels, arrows]);
        }
        let width = input.order_label.chars().try_fold(0., |width, character| {
            let glyph = font
                .glyphs
                .get(&character)
                .context("missing quick order glyph")?;
            Ok::<_, anyhow::Error>(width + glyph.advance as f32)
        })?;
        self.advance(input, input.order_label.chars().count());
        let [orders_x, enemies_x] = self.presented;
        if orders_x + width + 24. > 0. {
            let x = orders_x;
            let color = if input.order == 0 {
                CANCEL_BACKDROP
            } else {
                PRESET_BACKDROP
            };
            backdrop(&mut panels, [x, 348., width + 20., 14.], [color; 4], 4., 0.);
            label(
                &mut labels,
                font,
                input.order_label,
                [x + 22., 334.],
                [20., 26.],
                WHITE,
            )?;
            if (self.phase / 8).is_multiple_of(2) {
                for (omitted, rect, uv) in [
                    (
                        0,
                        [x + (width / 2.).trunc(), 360., 32., 20.],
                        [224., 64., 248., 48.],
                    ),
                    (
                        2,
                        [x + (width / 2.).trunc(), 316., 32., 20.],
                        [224., 48., 248., 64.],
                    ),
                    (1, [x, 334., 20., 28.], [208., 48., 192., 72.]),
                    (3, [x + width, 334., 20., 28.], [192., 48., 208., 72.]),
                ] {
                    if input.order != omitted {
                        let [x, y, w, h] = rect;
                        arrows.quad([x, y, x + w, y + h], uv, hud_color(WHITE));
                    }
                }
            }
        }
        if enemies_x < ENEMY_HIDDEN {
            let selected_group = input.target.and_then(|target| {
                enemies
                    .iter()
                    .find(|enemy| enemy.actor == target.index())
                    .map(|enemy| enemy.group)
            });
            let mut y = 354.;
            let groups: Vec<_> = (0..enemy_group_count(enemies))
                .filter(|&group| {
                    enemies.iter().any(|e| {
                        usize::from(e.group) == group
                            && frame.actors[e.actor].available()
                            && frame.actors[e.actor].hp > 0
                    })
                })
                .collect();
            let page = selected_group
                .and_then(|selected| groups.iter().position(|&g| g == usize::from(selected)))
                .unwrap_or(0)
                / 4;
            for &group in groups.iter().skip(page * 4).take(4) {
                let Some(enemy) = enemies.iter().find(|enemy| {
                    usize::from(enemy.group) == group
                        && frame
                            .actors
                            .get(enemy.actor)
                            .is_some_and(|actor| actor.available() && actor.hp > 0)
                }) else {
                    continue;
                };
                backdrop(
                    &mut panels,
                    [enemies_x - 8., y, 256., 12.],
                    ENEMY_BACKDROP,
                    3.,
                    4.,
                );
                let color = if selected_group.map(usize::from) == Some(group) {
                    let wave = ((self.phase.wrapping_mul(8) % 360) as f32)
                        .to_radians()
                        .sin();
                    let shade = (160. + 32. * wave) as u8;
                    [shade, shade, (112. - 16. * wave) as u8, 255]
                } else {
                    WHITE
                };
                label(
                    &mut labels,
                    font,
                    &enemy.name,
                    [enemies_x + 8., y - 10.],
                    [18., 22.],
                    color,
                )?;
                y -= 24.;
            }
        }
        Ok([panels, labels, arrows])
    }
}

fn label(
    batch: &mut Batch,
    font: &BitmapFont,
    value: &str,
    [x, y]: [f32; 2],
    size: [f32; 2],
    color: [u8; 4],
) -> Result<()> {
    for (offset, color) in [(1., SHADOW), (0., color)] {
        command_strip::label(
            batch,
            font,
            value,
            [x + offset, y + offset],
            size,
            size[0],
            0.,
            color,
        )?;
    }
    Ok(())
}

/// A colored strip with separately feathered edges and optional slant.
fn backdrop(batch: &mut Batch, [x, y, w, h]: [f32; 4], colors: [[u8; 4]; 4], edge: f32, skew: f32) {
    let [tl, tr, bl, br] = colors;
    let clear = |mut color: [u8; 4]| {
        color[3] = 0;
        color
    };
    for (rect, colors) in [
        ([x, y, x + w, y + h], colors),
        (
            [x + skew, y - edge, x + w + skew, y],
            [clear(tl), clear(tr), tl, tr],
        ),
        (
            [x - skew, y + h, x + w - skew, y + h + edge],
            [bl, br, clear(bl), clear(br)],
        ),
        ([x - edge, y, x, y + h], [clear(tl), tl, clear(bl), bl]),
        (
            [x + w, y, x + w + edge, y + h],
            [tr, clear(tr), br, clear(br)],
        ),
    ] {
        party::slanted_quad(batch, rect, [0.5; 4], skew, colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_notice_can_be_reselected_and_menu_holds_the_ribbons_open() {
        let mut state = State::default();
        let mut input = Input {
            tick: 1,
            menu_open: false,
            target: None,
            order: 0,
            order_revision: 0,
            order_label: "Cancel orders",
        };
        state.advance(input, 13);
        assert_eq!(state.presented, [-368., ENEMY_HIDDEN]);
        input.menu_open = true;
        for tick in 2..=11 {
            input.tick = tick;
            state.advance(input, 13);
        }
        assert_eq!(state.presented, [-224., 512.]);
        for tick in 12..=25 {
            input.tick = tick;
            state.advance(input, 13);
        }
        assert_eq!(state.presented, [0., ENEMY_SHOWN]);
        let placement = (state.presented, state.phase);
        state.advance(input, 13);
        assert_eq!((state.presented, state.phase), placement);
        input.menu_open = false;
        input.order = 1;
        input.order_revision = 1;
        for tick in 26..=86 {
            input.tick = tick;
            state.advance(input, 7);
        }
        assert_eq!(state.presented[0], 0.);
        input.tick = 87;
        state.advance(input, 7);
        assert_eq!(state.presented[0], -16.);
        for tick in 88..=187 {
            input.tick = tick;
            state.advance(input, 7);
        }
        assert_eq!(state.presented[0], -224.);
        input.tick = 188;
        input.order_revision = 2;
        state.advance(input, 7);
        assert_eq!(state.presented[0], ORDER_ENTRY);
        for tick in 189..=198 {
            input.tick = tick;
            state.advance(input, 7);
        }
        assert_eq!(state.presented[0], 0.);
    }
}
