//! Native party cards. Vitals stay readable when optional portraits are absent.
use super::*;
use resonance_battle::{BattleResult, conditions::Condition};

pub(super) const TOP: f32 = 388.;
const CARD_SPACING: f32 = 156.;
const BAR_WIDTH: f32 = 90.;
pub(super) const PORTRAIT_SIZE: f32 = 48.;

fn left(slot: usize) -> f32 {
    12. + slot as f32 * CARD_SPACING
}

pub(super) fn card_rect(slot: usize) -> [f32; 4] {
    let x = left(slot);
    [x - 4., TOP, x + 148., 476.]
}

fn vital_position(slot: usize, row: usize) -> [f32; 2] {
    [left(slot) + 54., TOP + 20. + row as f32 * 24.]
}

pub(super) fn recovery_position(slot: usize, row: usize) -> [f32; 2] {
    [left(slot), TOP + 24. + row as f32 * 24.]
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
        } else if matches!(actor.activity, Activity::Action | Activity::Casting { .. }) {
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

fn caption(actor: &resonance_battle::ActorFrame, queued: bool) -> String {
    let conditions = actor.conditions.effective();
    let condition = if conditions.contains(Condition::Petrified) {
        "STONE"
    } else if actor.hp <= 0 || !actor.available() {
        "DOWN"
    } else if conditions.intersects(POISON) {
        "POISON"
    } else if conditions.contains(Condition::Weak) {
        "WEAK"
    } else {
        ""
    };
    let action = if queued {
        "QUEUED"
    } else if actor.stored_spell.is_some() {
        "STORED"
    } else if matches!(
        actor.activity,
        resonance_battle::Activity::Casting { held: true }
    ) {
        "READY"
    } else {
        ""
    };
    format!("{condition} {action}").trim().to_owned()
}

fn text(batch: &mut Batch, font: &BitmapFont, label: &str, position: [f32; 2]) -> Result<()> {
    text::glyphs(
        batch,
        font,
        label,
        position,
        [10., 12.],
        [128, 128, 128, 255],
    )
}

fn bar(batch: &mut Batch, [x, y]: [f32; 2], current: f32, trail: f32, color: [f32; 4]) {
    let rect = |fraction| [x, y + 15., x + BAR_WIDTH * fraction, y + 20.];
    batch.quad(rect(1.), [0.5; 4], [0.2, 0.22, 0.25, 1.]);
    if trail > current {
        batch.quad(rect(trail.clamp(0., 1.)), [0.5; 4], [0.95, 0.8, 0.35, 1.]);
    }
    if current > 0. {
        batch.quad(rect(current.clamp(0., 1.)), [0.5; 4], color);
    }
}

impl Artwork {
    pub(super) fn party_name<'a>(
        &'a self,
        party: PartyHudInput<'a>,
        slot: usize,
    ) -> Result<&'a str> {
        let character = party
            .characters
            .get(slot)
            .context("missing HUD party member")?
            .checked_sub(1)
            .context("invalid HUD character")? as usize;
        let initial = self
            .menu_data
            .initial_names
            .get(character)
            .context("missing HUD character name")?;
        Ok(party
            .names
            .get(slot)
            .copied()
            .flatten()
            .or_else(|| {
                self.results
                    .as_ref()
                    .map(|r| r.model.character_names[character].as_str())
            })
            .unwrap_or(initial))
    }

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
        let mut gauges = Batch::default();
        let mut digits = Batch::default();
        let mut names = Batch::default();
        for (slot, ((id, actor), &character)) in
            actors.into_iter().zip(party.characters).enumerate()
        {
            ensure!((1..=9).contains(&character), "invalid battle HUD character");
            let character = usize::from(character - 1);
            let x = left(slot);
            gauges.quad(card_rect(slot), [0.5; 4], [0.04, 0.05, 0.08, 0.9]);
            let name = self.party_name(party, slot)?;
            text::fit(
                &mut names,
                &self.font,
                name,
                [x, TOP + 4., 140., 14.],
                [128, 128, 128, 255],
            )?;
            if self.art.portraits[character].is_some() {
                let portrait = portrait(actor, frame.recognized_result);
                let source_top = f32::from(portrait.row) * 64.;
                portraits[character].quad(
                    [x, TOP + 20., x + PORTRAIT_SIZE, TOP + 20. + PORTRAIT_SIZE],
                    [1., source_top + 1., 63., source_top + 63.],
                    portrait.color,
                );
            }
            let hp_color = if actor.hp <= actor.equipment.max_hp / 4 {
                [1., 0.3, 0.25, 1.]
            } else {
                [0.3, 0.8, 0.5, 1.]
            };
            let values = [
                ("HP", actor.hp, actor.equipment.max_hp, hp_color),
                (
                    "TP",
                    i32::from(actor.tp),
                    i32::from(actor.equipment.max_tp),
                    [0.4, 0.65, 1., 1.],
                ),
            ];
            for (row, (label, value, maximum, color)) in values.into_iter().enumerate() {
                let current = value as f32 / maximum.max(1) as f32;
                let trail = self
                    .numbers
                    .actors
                    .get(id)
                    .map_or(current, |numbers| f32::from(numbers.trails[row]) / 100.);
                let position = vital_position(slot, row);
                bar(&mut gauges, position, current, trail, color);
                text(
                    &mut digits,
                    &self.art.font,
                    &format!("{label} {value}"),
                    position,
                )?;
            }
            text(
                &mut digits,
                &self.art.font,
                &caption(actor, party.queued_techniques[slot]),
                [x, 460.],
            )?;
        }
        for (layer, batch) in self.ui.portraits.iter_mut().zip(portraits) {
            layer.upload(batch, commands, meshes)?;
        }
        self.ui.gauges.upload(gauges, commands, meshes)?;
        self.ui.font.upload(digits, commands, meshes)?;
        self.ui.party_names.upload(names, commands, meshes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::conditions::{Conditions, Layers};

    #[test]
    fn cards_use_live_conditions_actions_and_outcomes() {
        let mut actor = fixtures::actor(Layers {
            base: Condition::PoisonMild.into(),
            ..Default::default()
        });
        assert_eq!(caption(&actor, true), "POISON QUEUED");
        actor.activity = Activity::Casting { held: true };
        assert_eq!(caption(&actor, false), "POISON READY");
        actor.conditions = Conditions::default();
        assert_eq!(caption(&actor, false), "READY");
        actor.activity = Activity::Casting { held: false };
        assert_eq!(portrait(&actor, None).row, 1);
        assert_eq!(portrait(&actor, Some(BattleResult::Victory)).row, 3);
        actor.hp = 0;
        assert_eq!(portrait(&actor, Some(BattleResult::Victory)).row, 2);
    }

    #[test]
    #[ignore = "requires current battle artwork; CPU party HUD drawing only"]
    fn party_cards_render_names_vitals_and_recovery_without_portraits() -> Result<()> {
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
        artwork.prepare(&mut Commands::new(&mut queue, &world), &mut meshes);
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
            for layer in [
                &artwork.ui.font,
                &artwork.ui.party_names,
                &artwork.ui.gauges,
                &artwork.ui.recovery,
            ] {
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
