//! Compact party portraits, slanted vitals, and matching gauges.
use super::*;
use resonance_battle::BattleResult;

pub(super) const TOP: f32 = 388.;
const CARD_SPACING: f32 = 112.;
const BAR_SIZE: [f32; 2] = [56., 8.];
const NUMBER_SIZE: [f32; 2] = [16., 24.];
const NUMBER_ADVANCE: f32 = 13.;
pub(super) const PORTRAIT_SIZE: f32 = 64.;

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
}

pub(super) fn portrait(
    actor: &resonance_battle::ActorFrame,
    result: Option<BattleResult>,
) -> Portrait {
    let unavailable = actor.hp <= 0 || !actor.available();
    Portrait {
        row: if unavailable {
            2
        } else if result == Some(BattleResult::Victory) {
            3
        } else if actor.activity == Activity::Action {
            1
        } else {
            0
        },
        color: if unavailable {
            [0.55, 0.55, 0.55, 1.]
        } else {
            [1.; 4]
        },
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

fn bar(
    batch: &mut Batch,
    [x, y]: [f32; 2],
    current: f32,
    trail: f32,
    row: usize,
    colors: &resonance_content::battle_ui::MeterColors,
) {
    let rect = |fraction: f32| {
        [
            x,
            y,
            x + BAR_SIZE[0] * fraction.clamp(0., 1.),
            y + BAR_SIZE[1],
        ]
    };
    let [left, top, right, bottom] = rect(1.);
    slanted_quad(
        batch,
        [left + 1., top + 4., right + 1., bottom + 4.],
        [0.5; 4],
        8.,
        [colors.shadow; 4],
    );
    if trail > current {
        slanted_quad(batch, rect(trail), [0.5; 4], 8., [colors.trail; 4]);
    }
    if current > 0. {
        slanted_quad(batch, rect(current), [0.5; 4], 8., colors.bars[row]);
    }
}

fn vital(
    batch: &mut Batch,
    font: &BitmapFont,
    value: i32,
    [x, y]: [f32; 2],
    colors: [[u8; 4]; 2],
    shadow: [u8; 4],
) -> Result<()> {
    let text = format!("{:4}", value.clamp(0, 9999));
    for (offset, gradient) in [
        (1., [shadow; 4]),
        (0., [colors[0], colors[0], colors[1], colors[1]]),
    ] {
        for (column, ch) in text.chars().enumerate().filter(|(_, ch)| *ch != ' ') {
            let glyph = font.glyphs.get(&ch).context("missing battle numeral")?;
            let [u, v, w, h] = glyph.rect.map(|v| v as f32);
            let left = x + column as f32 * NUMBER_ADVANCE + offset;
            let top = y + offset;
            slanted_quad(
                batch,
                [left, top, left + NUMBER_SIZE[0], top + NUMBER_SIZE[1]],
                [u, v, u + w, v + h],
                10.,
                gradient,
            );
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
        let mut stat_icons: [Batch; 6] = std::array::from_fn(|_| Batch::default());
        let mut gauges = Batch::default();
        let mut digits = Batch::default();
        for (slot, ((id, actor), &character)) in
            actors.into_iter().zip(party.characters).enumerate()
        {
            if actor.availability == resonance_battle::ActorAvailability::Absent {
                continue;
            }
            ensure!((1..=9).contains(&character), "invalid battle HUD character");
            let character = usize::from(character - 1);
            let x = left(slot);
            if self.art.portraits[character].is_some() {
                let portrait = portrait(actor, frame.recognized_result);
                let source_top = f32::from(portrait.row) * 64.;
                portraits[character].quad(
                    [x, TOP + 8., x + PORTRAIT_SIZE, TOP + 8. + PORTRAIT_SIZE],
                    [1., source_top + 1., 63., source_top + 63.],
                    portrait.color,
                );
            }
            let mut shown = 0;
            use resonance_battle::conditions::Condition::*;
            for (index, condition, uv, increase) in [
                (0, AttackUp, [288., 136., 312., 160.], true),
                (1, DefenseUp, [352., 56., 376., 80.], true),
                (3, MagicAttackUp, [328., 80., 352., 104.], true),
                (4, PhysicalProtection, [256., 56., 280., 80.], false),
                (5, MagicalProtection, [280., 56., 304., 80.], false),
            ] {
                if actor.conditions.effective().contains(condition) {
                    let left = x + shown as f32 * 24.;
                    stat_icons[index].quad([left, TOP - 6., left + 24., TOP + 18.], uv, [1.; 4]);
                    if increase {
                        stat_icons[2].quad(
                            [left + 4., TOP + 2., left + 28., TOP + 18.],
                            [304., 64., 328., 80.],
                            [1.; 4],
                        );
                    }
                    shown += 1;
                }
            }
            let values = [
                (actor.hp, actor.equipment.max_hp),
                (i32::from(actor.tp), i32::from(actor.equipment.max_tp)),
            ];
            for (row, (value, maximum)) in values.into_iter().enumerate() {
                let current = value as f32 / maximum.max(1) as f32;
                let trail = self
                    .numbers
                    .actors
                    .get(id)
                    .map_or(current, |numbers| f32::from(numbers.trails[row]) / 100.);
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
                    &mut digits,
                    &self.art.font,
                    value,
                    position,
                    self.art.meters.numbers[row],
                    self.art.meters.shadow,
                )?;
            }
        }
        for (layer, batch) in self.ui.portraits.iter_mut().zip(portraits) {
            layer.upload(batch, commands, meshes)?;
        }
        for (layer, batch) in self.ui.stat_icons.iter_mut().zip(stat_icons) {
            layer.upload(batch, commands, meshes)?;
        }
        self.ui.gauges.upload(gauges, commands, meshes)?;
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
        assert_eq!(portrait(&actor, None).row, 0);
        actor.activity = Activity::Action;
        assert_eq!(portrait(&actor, None).row, 1);
        assert_eq!(portrait(&actor, Some(BattleResult::Victory)).row, 3);
        actor.hp = 0;
        assert_eq!(portrait(&actor, Some(BattleResult::Victory)).row, 2);
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
                    characters: &[1, 5, 6, 9],
                    queued_techniques: &[queued, false, false, false],
                    names: &[Some("Custom Name"), None, None, None],
                },
                None,
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
