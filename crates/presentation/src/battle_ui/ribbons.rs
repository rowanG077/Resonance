//! Sliding quick orders and the enemy-name list shown by battle selection.
use super::*;
use resonance_battle::ActorId;
use resonance_game::battle::lifecycle::QuickOrder;

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
    pub fraction: f32,
    pub menu_open: bool,
    /// A selected enemy, rather than the actor owning the target selector.
    pub target: Option<ActorId>,
    pub order: QuickOrder,
    pub order_revision: u32,
    pub order_label: &'a str,
}

#[derive(Default)]
pub(super) struct State {
    tick: Option<u32>,
    order_revision: u32,
    notice_since: Option<u32>,
    next: [f32; 2],
    presented: [f32; 2],
    phase: u32,
}

impl State {
    fn advance(&mut self, input: Input<'_>, order_characters: usize) {
        if self.tick == Some(input.tick) {
            return;
        }
        let hidden = -56. - order_characters as f32 * 24.;
        let first = self.tick.map_or(input.tick, |tick| tick.wrapping_add(1));
        let skipped = input.tick.wrapping_sub(first);
        if self.tick.is_none() {
            self.next = [hidden, ENEMY_HIDDEN];
        }
        // A newly observed order starts at this visit, even after a gap in draws.
        let new_order = input.order_revision != self.order_revision;
        if new_order {
            self.order_revision = input.order_revision;
            self.notice_since = Some(input.tick);
            self.next[0] = ORDER_ENTRY;
        }
        let (order_first, order_skipped) = if new_order {
            (input.tick, 0)
        } else {
            (first, skipped)
        };
        let notice_ticks = self.notice_since.map_or(0, |since| {
            ORDER_NOTICE_TICKS.saturating_sub(order_first.wrapping_sub(since))
        });
        let slide = |start: f32, target: f32, ticks: u32| {
            let distance = SLIDE_SPEED * ticks as f32;
            start + (target - start).clamp(-distance, distance)
        };
        let order_x = |ticks| {
            let shown = if input.menu_open {
                ticks
            } else {
                notice_ticks.min(ticks)
            };
            slide(slide(self.next[0], 0., shown), hidden, ticks - shown)
        };
        let enemy_target = if input.menu_open || input.target.is_some() {
            ENEMY_SHOWN
        } else {
            ENEMY_HIDDEN
        };
        let enemy_x = |ticks| slide(self.next[1], enemy_target, ticks);
        self.presented = [order_x(order_skipped), enemy_x(skipped)];
        self.next = [order_x(order_skipped + 1), enemy_x(skipped + 1)];
        self.phase = if self.tick.is_some() {
            input.tick.wrapping_sub(1)
        } else {
            input.tick
        };
        self.tick = Some(input.tick);
    }

    fn sample(&self, fraction: f32) -> [f32; 2] {
        std::array::from_fn(|i| self.presented[i] + (self.next[i] - self.presented[i]) * fraction)
    }

    /// Batches use the solid surface, dialogue font, and existing battle font atlas.
    pub(super) fn draw(
        &mut self,
        input: Input<'_>,
        font: &BitmapFont,
        enemies: &[EnemyHud],
        frame: &BattleFrame,
    ) -> Result<[Batch; 3]> {
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
        let [orders_x, enemies_x] = self.sample(input.fraction);
        if orders_x + width + 24. > 0. {
            let x = orders_x;
            let color = if input.order == QuickOrder::Cancel {
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
                        QuickOrder::Cancel,
                        [x + (width / 2.).trunc(), 360., 32., 20.],
                        [224., 64., 248., 48.],
                    ),
                    (
                        QuickOrder::UpperPreset,
                        [x + (width / 2.).trunc(), 316., 32., 20.],
                        [224., 48., 248., 64.],
                    ),
                    (
                        QuickOrder::LeftPreset,
                        [x, 334., 20., 28.],
                        [208., 48., 192., 72.],
                    ),
                    (
                        QuickOrder::RightPreset,
                        [x + width, 334., 20., 28.],
                        [192., 48., 208., 72.],
                    ),
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
                    .find(|enemy| enemy.actor == target)
                    .map(|enemy| enemy.group)
            });
            let mut y = 354.;
            let groups: Vec<_> = (0..enemy_group_count(enemies))
                .filter(|&group| {
                    enemies.iter().any(|e| {
                        usize::from(e.group) == group
                            && frame.actors[e.actor.index()].available()
                            && frame.actors[e.actor.index()].hp > 0
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
                            .get(enemy.actor.index())
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
                    let wave = ((self.phase.wrapping_mul(8) % 360) as f32 + input.fraction * 8.)
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

fn backdrop(batch: &mut Batch, [x, y, w, h]: [f32; 4], colors: [[u8; 4]; 4], edge: f32, skew: f32) {
    party::backdrop(batch, [x, y, x + w, y + h], colors, edge, skew);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_notice_can_be_reselected_and_menu_holds_the_ribbons_open() {
        let mut state = State::default();
        let mut input = Input {
            tick: 1,
            fraction: 0.,
            menu_open: false,
            target: None,
            order: QuickOrder::Cancel,
            order_revision: 0,
            order_label: "Cancel orders",
        };
        state.advance(input, 13);
        assert_eq!(state.presented, [-368., ENEMY_HIDDEN]);
        input.menu_open = true;
        for tick in [2, 11] {
            input.tick = tick;
            state.advance(input, 13);
        }
        assert_eq!(state.presented, [-224., 512.]);
        assert_eq!(state.sample(0.5), [-216., 504.]);
        for tick in 12..=25 {
            input.tick = tick;
            state.advance(input, 13);
        }
        assert_eq!(state.presented, [0., ENEMY_SHOWN]);
        let placement = (state.presented, state.phase);
        state.advance(input, 13);
        assert_eq!((state.presented, state.phase), placement);
        input.menu_open = false;
        input.order = QuickOrder::LeftPreset;
        input.order_revision = 1;
        for tick in [26, 86] {
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
