//! Battle opening enemy names, quick orders, and target selection.
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
        opening_fraction: f32,
        bounds: [f32; 4],
    ) -> Result<[Batch; 3]> {
        let [mut panels, mut labels, mut arrows] = std::array::from_fn(|_| Batch::default());
        if frame.recognized_result.is_some() {
            *self = Self::default();
            return Ok([panels, labels, arrows]);
        }
        opening_roster(
            &mut panels,
            &mut labels,
            font,
            enemies,
            frame,
            opening_fraction,
            bounds,
        )?;
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

/// Introduce each group in the starting formation, including its initial head count.
fn opening_roster(
    panels: &mut Batch,
    labels: &mut Batch,
    font: &BitmapFont,
    enemies: &[EnemyHud],
    frame: &BattleFrame,
    fraction: f32,
    [left, _, right, _]: [f32; 4],
) -> Result<()> {
    const HOLD: u64 = 120;
    const STAGGER: u64 = 4;
    const FADE: u64 = 40;
    // Skip ordinary combat frames without traversing the formation.
    if frame.update >= HOLD + FADE + STAGGER * enemies.len() as u64
        || !enemies.iter().any(|e| {
            let actor = &frame.actors[e.actor.index()];
            actor.available() && actor.hp > 0
        })
    {
        return Ok(());
    }

    for group in 0..enemy_group_count(enemies) {
        let mut members = enemies.iter().filter(|e| usize::from(e.group) == group);
        let Some(first) = members.next() else {
            continue;
        };
        let exit = (frame.update as f32 + fraction - (HOLD + group as u64 * STAGGER) as f32)
            .clamp(0., FADE as f32);
        let alpha = (160. - 4. * exit) as u8;
        if alpha == 0 {
            continue;
        }
        let y = 28. + group as f32 * 28.;
        let colors = ENEMY_BACKDROP.map(|[r, g, b, _]| [r, g, b, alpha]);
        backdrop(panels, [left, y, right - left, 12.], colors, 4., 0.);

        let x = 320. + 28. * exit;
        if x < right {
            let spacing = bitmap_text::Spacing::Font;
            let mut draw = |value: &str, x: f32| {
                bitmap_text::draw(
                    font,
                    value,
                    [x, y - 16.],
                    [20., 26.],
                    spacing,
                    |[left, top, _, bottom], uv| {
                        party::slanted_quad(
                            labels,
                            [left, top, left + 22., bottom],
                            uv,
                            8.,
                            [WHITE; 4],
                        );
                    },
                )
            };
            draw(&first.name, x)?;
            let count = 1 + members.count();
            if count > 1 {
                let width = bitmap_text::width(font, &first.name, 20., spacing)?;
                draw(&count.to_string(), x + width + 56.)?;
            }
        }
    }
    Ok(())
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
    #[ignore = "requires the dialogue font"]
    fn opening_names_group_the_starting_enemies_and_leave_in_order() -> Result<()> {
        let font = fixtures::dialogue_font()?;
        let actor = fixtures::actor(Default::default());
        let mut enemies = Vec::new();
        for (slot, (name, group)) in [
            ("Black Bat", 0),
            ("Cardinal Knight", 1),
            ("Bear", 2),
            ("Black Bat", 0),
            ("Cardinal Knight", 1),
        ]
        .into_iter()
        .enumerate()
        {
            enemies.push(EnemyHud {
                actor: ActorId::from_index(slot)?,
                group,
                name: name.into(),
                icon: None,
            });
        }
        let mut frame = BattleFrame {
            actors: vec![actor; enemies.len()],
            ..Default::default()
        };
        let draw = |frame: &BattleFrame| -> Result<_> {
            let mut panels = Batch::default();
            let mut labels = Batch::default();
            opening_roster(
                &mut panels,
                &mut labels,
                &font,
                &enemies,
                frame,
                0.,
                [0., 0., 640., 480.],
            )?;
            Ok((panels, labels))
        };
        let (_, initial) = draw(&frame)?;
        let glyphs = "Black Bat2Cardinal Knight2Bear".chars().map(|c| {
            let [u, v, _, _] = font.glyphs[&c].rect;
            [u as f32, v as f32]
        });
        assert!(initial.uv.iter().step_by(4).copied().eq(glyphs));
        let glyph_x = |index: usize| initial.positions[index * 4 + 3][0] + 320.;
        let second_row = "Black Bat2".len();
        assert_eq!(glyph_x(second_row - 1), 497.);
        assert_eq!(glyph_x(second_row + "Cardinal Knight".len()), 571.);
        frame.actors[0].hp = 0;
        assert!(
            initial == draw(&frame)?.1,
            "the opening count describes the starting formation"
        );
        frame.update = 121;
        let (_, leaving) = draw(&frame)?;
        assert_eq!(leaving.positions[0][0], initial.positions[0][0] + 28.);
        assert_eq!(
            leaving.positions[4 * second_row],
            initial.positions[4 * second_row]
        );
        frame.update = 168;
        assert!(draw(&frame)?.0.indices.is_empty());
        frame.update = 10;
        frame.actors.iter_mut().for_each(|actor| actor.hp = 0);
        let (panels, labels) = draw(&frame)?;
        assert!(panels.indices.is_empty() && labels.indices.is_empty());
        Ok(())
    }

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
