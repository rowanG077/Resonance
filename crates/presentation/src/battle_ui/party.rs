//! Compact party portraits, slanted vitals, and matching gauges.
use super::*;
use resonance_battle::{
    BattleResult,
    conditions::{Condition, ConditionSet, POISON},
};

pub(super) const TOP: f32 = 388.;
const CARD_SPACING: f32 = 112.;
const BAR_SIZE: [f32; 2] = [56., 8.];
const NUMBER_SIZE: [f32; 2] = [16., 24.];
const NUMBER_ADVANCE: f32 = 13.;
pub(super) const PORTRAIT_SIZE: f32 = 64.;
const HP_WARNING: [[u8; 4]; 2] = [[128, 96, 96, 255], [128, 80, 80, 255]];
const HP_CRITICAL: [[u8; 4]; 2] = [[128, 80, 80, 255], [128, 48, 48, 255]];
const HP_POISON: [[u8; 4]; 2] = [[64, 128, 64, 255], [32, 96, 32, 255]];
const VITAL_PATTERN_ORIGIN: [f32; 2] = [448., 0.];
const VITAL_PATTERN_SIZE: u64 = 32;

// Order of the party status palettes in the artwork descriptor.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(super) enum StatusIcon {
    Attack,
    Defense,
    UpArrow,
    MagicAttack,
    PhysicalBarrier,
    MagicalBarrier,
    Accuracy,
    Item,
    HolySong,
    DownArrow,
}

fn status_badges(batches: &mut [Batch], active: ConditionSet, [x, y]: [f32; 2], tick: u64) {
    use Condition::*;
    use StatusIcon as Icon;
    const SIZE: f32 = 24.;
    const SPACING: f32 = 22.;
    const ARROW_ALPHA: f32 = 208. / 255.;
    let badges = [
        (AttackUp, Some(AttackDown), Icon::Attack, [288., 136.]),
        (DefenseUp, Some(DefenseDown), Icon::Defense, [352., 56.]),
        (AccuracyUp, Some(AccuracyDown), Icon::Accuracy, [0., 72.]),
        (
            MagicAttackUp,
            Some(MagicAttackDown),
            Icon::MagicAttack,
            [328., 80.],
        ),
        (HolySong, None, Icon::HolySong, [352., 80.]),
        (Curse, None, Icon::Accuracy, [0., 72.]),
        (ReduceItemEffect, None, Icon::Item, [32., 72.]),
        (PhysicalBarrier, None, Icon::PhysicalBarrier, [256., 56.]),
        (MagicalBarrier, None, Icon::MagicalBarrier, [280., 56.]),
    ];
    let visible = badges.into_iter().filter(|(up, down, _, _)| {
        active.contains(*up) || down.is_some_and(|down| active.contains(down))
    });
    let count = visible.clone().count();
    let two_rows = count >= 6;
    let columns = if two_rows { count / 2 } else { count };
    let phase = (tick / 16 % 2) as f32;
    for (index, (up, down, icon, [u, v])) in visible.enumerate() {
        let lower_row = two_rows && index >= columns;
        let column = if lower_row { index - columns } else { index };
        let left = x + column as f32 * SPACING;
        let top = y - if two_rows && !lower_row { SIZE } else { 0. };
        let rect = [left, top, left + SIZE, top + SIZE];
        batches[icon as usize].quad(rect, [u, v, u + SIZE, v + SIZE], [1.; 4]);
        if up == Curse && phase != 0. {
            batches[icon as usize].quad(rect, [256., 80., 280., 104.], [1., 1., 1., ARROW_ALPHA]);
        }
        let arrow = if down.is_some() {
            Some(if active.contains(up) {
                Icon::UpArrow
            } else {
                Icon::DownArrow
            })
        } else {
            (up == ReduceItemEffect).then_some(Icon::DownArrow)
        };
        if let Some(arrow) = arrow {
            let u = if matches!(arrow, Icon::UpArrow) {
                304.
            } else {
                328.
            };
            let left = left + 4.;
            let top = top + 2. - phase * 2.;
            batches[arrow as usize].quad(
                [left, top, left + SIZE, top + SIZE],
                [u, 56., u + SIZE, 80.],
                [1., 1., 1., ARROW_ALPHA],
            );
        }
    }
}

fn left(slot: usize) -> f32 {
    12. + slot as f32 * CARD_SPACING
}

pub(super) fn card_rect(slot: usize) -> [f32; 4] {
    let x = left(slot);
    [x, TOP + 8., x + CARD_SPACING, TOP + 72.]
}

fn vital_position(slot: usize, row: usize) -> [f32; 2] {
    [left(slot) + 50., TOP + 16. + row as f32 * 28.]
}

pub(super) fn recovery_position(slot: usize, row: usize) -> [f32; 2] {
    [104. + slot as f32 * 110., TOP + 8. + row as f32 * 24.]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Portrait {
    pub row: u8,
    pub color: [f32; 4],
    pub offset_y: f32,
}

pub(super) fn portrait(
    actor: &resonance_battle::ActorFrame,
    result: Option<BattleResult>,
    tick: u64,
) -> Portrait {
    let activity = if actor.cast_releasing && actor.hp > 0 && actor.available() {
        Activity::Action
    } else {
        actor.activity
    };
    let conditions = actor.conditions.effective();
    let tint = if conditions.intersects(POISON) {
        [64, 128, 64]
    } else if conditions.contains(Condition::Weak) {
        [128, 80, 80]
    } else {
        [128; 3]
    };
    let brightness = match activity {
        Activity::Casting { held: false } => {
            let angle = ((tick % 360) * 32 % 360) as f32;
            (160. + 32. * angle.to_radians().sin()) as u16
        }
        Activity::Casting { held: true } => 128 + (tick / 2 % 2) as u16 * 64,
        _ if actor.hp <= 0 || !actor.available() => 96,
        _ => 128,
    };
    let color = tint.map(|channel| f32::from(channel * brightness / 128) / 128.);
    let shake = actor.reaction.hit_shake_remaining();
    Portrait {
        row: if actor.hp > 0 && actor.available() && result == Some(BattleResult::Victory) {
            3
        } else {
            match activity {
                Activity::Action => 1,
                Activity::Hurt | Activity::Defeated => 2,
                _ => 0,
            }
        },
        color: [color[0], color[1], color[2], 1.],
        offset_y: -f32::from(shake * (shake % 2)),
    }
}

fn hp_colors(actor: &resonance_battle::ActorFrame, normal: [[u8; 4]; 2]) -> [[u8; 4]; 2] {
    let conditions = actor.conditions.effective();
    if actor.hp <= 0 || conditions.contains(Condition::Weak) {
        HP_CRITICAL
    } else if conditions.intersects(POISON) {
        HP_POISON
    } else if actor.hp <= actor.equipment.max_hp / 4 {
        HP_WARNING
    } else {
        normal
    }
}

/// Four corners use the same order as the authored color data: TL, TR, BL, BR.
pub(super) fn slanted_quad(
    batch: &mut Batch,
    rect: [f32; 4],
    uv: [f32; 4],
    skew: f32,
    colors: [[u8; 4]; 4],
) {
    let start = batch.positions.len();
    batch.quad(rect, uv, [1.; 4]);
    batch.positions[start][0] += skew;
    batch.positions[start + 1][0] += skew;
    for (vertex, color) in [0, 1, 3, 2].into_iter().zip(colors) {
        batch.colors[start + vertex] = hud_color(color);
    }
}

/// A colored strip with separately feathered edges and optional slant.
pub(super) fn backdrop(
    batch: &mut Batch,
    [left, top, right, bottom]: [f32; 4],
    colors: [[u8; 4]; 4],
    edge: f32,
    skew: f32,
) {
    let clear = colors.map(|[r, g, b, _]| [r, g, b, 0]);
    for (rect, colors) in [
        ([left, top, right, bottom], colors),
        (
            [left + skew, top - edge, right + skew, top],
            [clear[0], clear[1], colors[0], colors[1]],
        ),
        (
            [left - skew, bottom, right - skew, bottom + edge],
            [colors[2], colors[3], clear[2], clear[3]],
        ),
        (
            [left - edge, top, left, bottom],
            [clear[0], colors[0], clear[2], colors[2]],
        ),
        (
            [right, top, right + edge, bottom],
            [colors[1], clear[1], colors[3], clear[3]],
        ),
    ] {
        slanted_quad(batch, rect, [0.5; 4], skew, colors);
    }
}

fn bar(
    batch: &mut Batch,
    [x, y]: [f32; 2],
    current: i16,
    trail: i16,
    row: usize,
    colors: &resonance_content::battle_ui::MeterColors,
) {
    let width = |percent: i16| f32::from(percent.clamp(0, 100) * BAR_SIZE[0] as i16 / 100);
    let rect = |width| [x, y, x + width, y + BAR_SIZE[1]];
    let [left, top, right, bottom] = rect(BAR_SIZE[0]);
    slanted_quad(
        batch,
        [left + 1., top + 4., right + 1., bottom + 4.],
        [0.5; 4],
        8.,
        [colors.shadow; 4],
    );
    let current = width(current);
    if current > 0. {
        slanted_quad(batch, rect(current), [0.5; 4], 8., colors.bars[row]);
    }
    if width(trail) > current {
        slanted_quad(
            batch,
            [x + current, y, x + width(trail), y + BAR_SIZE[1]],
            [0.5; 4],
            8.,
            [colors.trail; 4],
        );
    }
}

fn vital(
    (batch, patterned): (&mut Batch, &mut Batch),
    font: &BitmapFont,
    value: i32,
    [x, y]: [f32; 2],
    colors: [[u8; 4]; 2],
    shadow: [u8; 4],
    pattern_tick: Option<u64>,
) -> Result<()> {
    let text = format!("{:4}", value.clamp(0, 9999));
    for (offset, gradient) in [
        (1., [shadow; 4]),
        (0., [colors[0], colors[0], colors[1], colors[1]]),
    ] {
        let target = if offset == 0. && pattern_tick.is_some() {
            &mut *patterned
        } else {
            &mut *batch
        };
        for (column, ch) in text.chars().enumerate().filter(|(_, ch)| *ch != ' ') {
            let glyph = font.glyphs.get(&ch).context("missing battle numeral")?;
            let [u, v, w, h] = glyph.rect.map(|v| v as f32);
            let left = x + column as f32 * NUMBER_ADVANCE + offset;
            let top = y + offset;
            slanted_quad(
                target,
                [left, top, left + NUMBER_SIZE[0], top + NUMBER_SIZE[1]],
                [u, v, u + w, v + h],
                10.,
                gradient,
            );
            if offset == 0.
                && let Some(tick) = pattern_tick
            {
                let [u, v] = VITAL_PATTERN_ORIGIN;
                let u = u + (tick / 2 % VITAL_PATTERN_SIZE) as f32;
                let size = VITAL_PATTERN_SIZE as f32;
                target.pattern_uv.extend([
                    [u, v],
                    [u + size, v],
                    [u + size, v + size],
                    [u, v + size],
                ]);
            }
        }
    }
    Ok(())
}

impl Artwork {
    pub(super) fn render_party(
        &mut self,
        frame: &BattleFrame,
        party: PartyHudInput<'_>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let actors: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .filter(|(_, actor)| actor.side == Side::Party)
            .collect();
        ensure!(
            actors.len() <= 4
                && actors.len() == party.characters.len()
                && actors.len() == party.queued_techniques.len()
                && actors.len() == party.names.len(),
            "battle HUD party differs from frame"
        );
        let mut portraits: [Batch; 9] = std::array::from_fn(|_| Batch::default());
        let mut petrified_portraits: [Batch; 9] = std::array::from_fn(|_| Batch::default());
        let mut stat_icons = self.art.stat_icons.each_ref().map(|_| Batch::default());
        let mut rhythm_prompt = Batch::default();
        let mut gauges = Batch::default();
        let mut digits = Batch::default();
        let mut patterned_digits = Batch::default();
        for (slot, ((id, actor), &character)) in
            actors.into_iter().zip(party.characters).enumerate()
        {
            if actor.availability == resonance_battle::ActorAvailability::Absent {
                continue;
            }
            let character = character.index();
            let x = left(slot);
            let portrait = portrait(
                actor,
                frame.recognized_result,
                self.numbers.presentation_tick,
            );
            if self.art.portraits[character].is_some() {
                let source_top = f32::from(portrait.row) * 64.;
                let top = TOP + 8. + portrait.offset_y;
                let portraits = if actor.conditions.effective().contains(Condition::Petrified) {
                    &mut petrified_portraits
                } else {
                    &mut portraits
                };
                portraits[character].quad(
                    [x, top, x + PORTRAIT_SIZE, top + PORTRAIT_SIZE],
                    [1., source_top + 1., 63., source_top + 63.],
                    portrait.color,
                );
            }
            status_badges(
                &mut stat_icons,
                actor.conditions.effective(),
                [x, TOP - 6. + portrait.offset_y],
                self.numbers.presentation_tick,
            );
            if actor.equipment.casting.rhythm
                && actor.control != resonance_battle::Control::Auto
                && matches!(actor.activity, Activity::Casting { held: false })
                && !actor.cast_releasing
            {
                let top = TOP + 48.;
                let v = 64. + (self.numbers.presentation_tick / 8 % 2) as f32 * 32.;
                rhythm_prompt.quad(
                    [x, top, x + 32., top + 32.],
                    [384., v, 416., v + 32.],
                    [1.; 4],
                );
            }
            let numbers = self.numbers.actors.get(id);
            let values = numbers
                .and_then(|numbers| numbers.vitals)
                .unwrap_or([actor.hp, i32::from(actor.tp)]);
            for (row, (value, current)) in values
                .into_iter()
                .zip([actor.hp_percent(), actor.tp_percent()])
                .enumerate()
            {
                let trail = numbers.map_or(current, |numbers| numbers.trails[row]);
                let position = vital_position(slot, row);
                bar(
                    &mut gauges,
                    [x + 48., position[1] + 18.],
                    current,
                    trail,
                    row,
                    &self.art.meters,
                );
                vital(
                    (&mut digits, &mut patterned_digits),
                    &self.art.font,
                    value,
                    position,
                    if row == 0 {
                        hp_colors(actor, self.art.meters.numbers[0])
                    } else {
                        self.art.meters.numbers[1]
                    },
                    self.art.meters.shadow,
                    (row == 0
                        && (actor.conditions.effective().intersects(POISON)
                            || actor.conditions.effective().contains(Condition::Weak)))
                    .then_some(self.numbers.presentation_tick),
                )?;
            }
        }
        for (layer, batch) in self.ui.portraits.iter_mut().zip(portraits) {
            layer.upload(batch, commands, meshes)?;
        }
        for (layer, batch) in self
            .ui
            .petrified_portraits
            .iter_mut()
            .zip(petrified_portraits)
        {
            layer.upload(batch, commands, meshes)?;
        }
        for (layer, batch) in self.ui.stat_icons.iter_mut().zip(stat_icons) {
            layer.upload(batch, commands, meshes)?;
        }
        self.ui
            .rhythm_prompt
            .upload(rhythm_prompt, commands, meshes)?;
        self.ui.gauges.upload(gauges, commands, meshes)?;
        self.ui
            .patterned_vitals
            .upload(patterned_digits, commands, meshes)?;
        self.ui.font.upload(digits, commands, meshes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portraits_follow_actions_and_outcomes() {
        let mut actor = fixtures::actor(Default::default());
        actor.activity = Activity::Casting { held: false };
        assert_eq!(portrait(&actor, None, 0).row, 0);
        assert_eq!(portrait(&actor, None, 0).color, [1.25, 1.25, 1.25, 1.]);
        assert!(portrait(&actor, None, 3).color[0] > 1.4);
        actor.activity = Activity::Casting { held: true };
        assert_eq!(portrait(&actor, None, 0).color, [1.; 4]);
        assert_eq!(portrait(&actor, None, 2).color, [1.5, 1.5, 1.5, 1.]);
        actor.cast_releasing = true;
        assert_eq!(portrait(&actor, None, 2).row, 1);
        assert_eq!(portrait(&actor, None, 2).color, [1.; 4]);
        actor.hp = 0;
        assert_eq!(portrait(&actor, None, 2).row, 0);
        actor.hp = actor.equipment.max_hp;
        actor.cast_releasing = false;
        actor.activity = Activity::Action;
        assert_eq!(portrait(&actor, None, 0).row, 1);
        assert_eq!(portrait(&actor, Some(BattleResult::Victory), 0).row, 3);
        actor.activity = Activity::Hurt;
        assert_eq!(portrait(&actor, None, 0).row, 2);
        actor.activity = Activity::Idle;
        actor.hp = 0;
        assert_eq!(portrait(&actor, Some(BattleResult::Victory), 0).row, 0);
        assert_eq!(portrait(&actor, None, 0).color, [0.75, 0.75, 0.75, 1.]);
        actor.activity = Activity::Defeated;
        assert_eq!(portrait(&actor, None, 0).row, 2);
        let normal = [[128; 4]; 2];
        assert_eq!(hp_colors(&actor, normal), HP_CRITICAL);
        actor.hp = actor.equipment.max_hp / 4;
        assert_eq!(hp_colors(&actor, normal), HP_WARNING);
        actor.hp += 1;
        assert_eq!(hp_colors(&actor, normal), normal);
        actor.conditions =
            resonance_battle::conditions::Conditions::new(resonance_battle::conditions::Layers {
                base: POISON,
                ..Default::default()
            });
        actor.activity = Activity::Idle;
        assert_eq!(hp_colors(&actor, normal), HP_POISON);
        assert_eq!(portrait(&actor, None, 0).color, [0.5, 1., 0.5, 1.]);
        actor.conditions =
            resonance_battle::conditions::Conditions::new(resonance_battle::conditions::Layers {
                base: Condition::Weak.into(),
                ..Default::default()
            });
        assert_eq!(hp_colors(&actor, normal), HP_CRITICAL);
        assert_eq!(portrait(&actor, None, 0).color, [1., 0.625, 0.625, 1.]);
    }

    #[test]
    #[ignore = "requires current battle artwork; CPU party HUD drawing only"]
    fn vitals_and_recovery_remain_visible_without_portraits() -> Result<()> {
        use resonance_content::diagnostics::Diagnostics;

        let edit = |document: &mut serde_json::Value| {
            document["portraits"][0]["path"] = serde_json::json!("ui/dialogue.json");
        };
        assert!(test_artwork_with(edit, &Diagnostics::new(true)).is_err());
        let diagnostics = Diagnostics::new(false);
        let (mut artwork, materials, app) = test_artwork_with(edit, &diagnostics)?;
        assert!(artwork.layers().all(|layer| {
            app.world()
                .resource::<Assets<Image>>()
                .contains(&layer.definition.image)
        }));
        assert!(diagnostics.has_errors());
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        artwork.prepare(
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
            crate::Resolution::default(),
        );
        queue.apply(&mut world);
        let counts = (materials.len(), meshes.len());
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors = vec![frame.actors[0].clone(); 4];
        frame.actors[0].hp = 1234;
        frame.actors[0].equipment.max_hp = 2000;
        frame.cues.push(resonance_battle::Cue::Recovered {
            floating: true,
            actor: resonance_battle::ActorId::from_index(0)?,
            kind: resonance_battle::RecoveryKind::Hp,
            nominal: 25,
            applied: 25,
        });
        artwork.advance(&frame, false)?;
        for queued in [true, false] {
            artwork.render(
                &frame,
                PartyHudInput {
                    characters: &[
                        Character::Lloyd,
                        Character::Sheena,
                        Character::Zelos,
                        Character::Kratos,
                    ],
                    queued_techniques: &[queued, false, false, false],
                    names: &[Some("Custom Name"), None, None, None],
                },
                None,
                None,
                &[super::ActorMarkers::default(); 4],
                &mut Commands::new(&mut queue, &world),
                &mut meshes,
            )?;
            queue.apply(&mut world);
            assert!(
                artwork.ui.portraits[0]
                    .rendered
                    .as_ref()
                    .unwrap()
                    .uploaded
                    .as_ref()
                    .unwrap()
                    .0
                    .indices
                    .is_empty()
            );
            assert!(
                !artwork.ui.portraits[4]
                    .rendered
                    .as_ref()
                    .unwrap()
                    .uploaded
                    .as_ref()
                    .unwrap()
                    .0
                    .indices
                    .is_empty()
            );
            for layer in [&artwork.ui.font, &artwork.ui.gauges, &artwork.ui.recovery] {
                let batch = &layer
                    .rendered
                    .as_ref()
                    .unwrap()
                    .uploaded
                    .as_ref()
                    .unwrap()
                    .0;
                assert!(!batch.indices.is_empty());
                assert!(batch.positions.iter().all(
                    |p| (-320. ..=320.).contains(&p[0]) && (-240. ..=240.).contains(&p[1])
                ));
            }
            assert_eq!((materials.len(), meshes.len()), counts);
        }
        Ok(())
    }
}
