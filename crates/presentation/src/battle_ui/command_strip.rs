//! Native command buttons and item selection on two ordinary HUD layers.
use super::*;
use resonance_game::battle::command::{Command, Frame, View};

fn label(command: Command, escape_requested: bool) -> &'static str {
    match command {
        Command::Tech => "Techniques",
        Command::Unison => "Unison",
        Command::Strategy => "Strategy",
        Command::Equipment => "Equipment",
        Command::Items => "Items",
        Command::Escape if escape_requested => "Cancel escape",
        Command::Escape => "Escape",
    }
}

pub(super) fn validate_font(font: &BitmapFont) -> Result<()> {
    for command in Command::ALL {
        font.validate_text(label(command, false))?;
    }
    font.validate_text(
        "P1234 COMMANDS Cancel escape Unavailable Choose item user Choose item target User: Target: Queued: /",
    )
}

impl Artwork {
    pub fn hide_commands(&mut self, commands: &mut Commands) {
        for layer in &mut self.ui.commands {
            layer.show(false, commands);
        }
    }

    pub fn render_commands(
        &mut self,
        state: Option<&Frame>,
        pending: Option<resonance_battle::item::Release>,
        characters: &[u8],
        user_name: Option<&str>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let pending = pending
            .filter(|_| {
                state.is_some_and(|state| {
                    state.selected == Command::Items && matches!(state.view, View::Strip)
                })
            })
            .map(|release| -> Result<_> {
                let character = characters
                    .get(release.user.index())
                    .and_then(|id| id.checked_sub(1))
                    .context("pending item user has no prepared character")?;
                let initial = self
                    .menu_data
                    .initial_names
                    .get(usize::from(character))
                    .context("invalid pending item character")?;
                let item = self.menu_data.item_text(release.item)?;
                Ok(format!(
                    "Queued: {} / {}",
                    user_name.unwrap_or(initial),
                    item.name
                ))
            })
            .transpose()?;
        let batches = draw(state, pending.as_deref(), &self.font)?;
        for (layer, batch) in self.ui.commands.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

fn draw(state: Option<&Frame>, pending: Option<&str>, font: &BitmapFont) -> Result<[Batch; 2]> {
    let mut panels = Batch::default();
    let mut text = Batch::default();
    let Some(state) =
        state.filter(|state| matches!(state.view, View::Strip | View::User(_) | View::Ally(_)))
    else {
        return Ok([panels, text]);
    };
    ensure!(state.controller < 4, "invalid command controller");
    let white = [128, 128, 128, 255];
    let gold = [128, 112, 65, 255];
    let muted = [70, 74, 80, 255];
    let rejected = [128, 70, 65, 255];
    panels.quad([8., 88., 632., 204.], [0.5; 4], [0.04, 0.05, 0.08, 0.95]);
    text::fit(
        &mut text,
        font,
        &format!("P{} COMMANDS", state.controller + 1),
        [20., 96., 600., 18.],
        white,
    )?;
    for (index, command) in Command::ALL.into_iter().enumerate() {
        let x = 20. + index as f32 * 100.;
        let enabled = state.enabled & command.mask() != 0;
        let selected = state.selected == command;
        if selected {
            panels.quad([x - 2., 122., x + 94., 166.], [0.5; 4], [1., 0.8, 0.35, 1.]);
        }
        panels.quad(
            [x, 124., x + 92., 164.],
            [0.5; 4],
            if selected {
                [0.18, 0.19, 0.22, 1.]
            } else {
                [0.09, 0.1, 0.13, 1.]
            },
        );
        text::fit(
            &mut text,
            font,
            label(command, state.escape_requested),
            [x + 4., 135., 84., 18.],
            if enabled { white } else { muted },
        )?;
    }
    let availability = if state.enabled & state.selected.mask() == 0 {
        "Unavailable"
    } else {
        ""
    };
    let hint = match &state.view {
        View::User(_) => "Choose item user",
        View::Ally(_) => "Choose item target",
        _ if state.selected == Command::Items => pending.unwrap_or(availability),
        _ => availability,
    };
    text::fit(&mut text, font, hint, [20., 178., 600., 18.], white)?;
    if let View::User(selection) | View::Ally(selection) = &state.view {
        ensure!(selection.slot < 4, "invalid item selector slot");
        let [left, top, right, bottom] = party::card_rect(usize::from(selection.slot));
        let color = if selection.eligible {
            [1., 0.8, 0.35, 1.]
        } else {
            [1., 0.4, 0.35, 1.]
        };
        for rect in [
            [left, top, right, top + 2.],
            [left, bottom - 2., right, bottom],
            [left, top, left + 2., bottom],
            [right - 2., top, right, bottom],
        ] {
            panels.quad(rect, [0.5; 4], color);
        }
        panels.quad(
            [left, top - 30., right, top - 2.],
            [0.5; 4],
            [0.04, 0.05, 0.08, 0.95],
        );
        let role = if matches!(state.view, View::Ally(_)) {
            "Target"
        } else {
            "User"
        };
        let caption = if selection.eligible {
            format!("{role}: {}", selection.name)
        } else {
            format!("Unavailable: {}", selection.name)
        };
        text::fit(
            &mut text,
            font,
            &caption,
            [left + 4., top - 24., right - left - 8., 18.],
            if selection.eligible { gold } else { rejected },
        )?;
    }
    Ok([panels, text])
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_battle::ActorId;
    use resonance_game::battle::command::ActorSelection;

    #[test]
    #[ignore = "requires current dialogue font; CPU command drawing only"]
    fn native_commands_show_selection_availability_items_and_party_targets() -> Result<()> {
        let font = &fixtures::dialogue_font()?;
        let actor = ActorId::from_index(0)?;
        let mut frame = Frame {
            actor,
            selected: Command::Tech,
            enabled: u8::MAX,
            controller: 3,
            connected: [true; 4],
            view: View::Strip,
            escape_requested: false,
        };
        let pending = "Queued: R%s / A very long item name that still fits the command panel";
        let ordinary = draw(Some(&frame), Some(pending), font)?;
        let mut dense_font = font.clone();
        dense_font.width *= 2;
        dense_font.height *= 2;
        dense_font.line_height *= 2;
        for glyph in dense_font.glyphs.values_mut() {
            glyph.rect = glyph.rect.map(|value| value * 2);
            glyph.advance *= 2;
        }
        let dense = draw(Some(&frame), Some(pending), &dense_font)?;
        assert_eq!(
            ordinary[1].positions, dense[1].positions,
            "atlas density does not change layout"
        );
        let check_bounds =
            |batches: &[Batch; 2]| {
                assert!(batches.iter().flat_map(|b| &b.positions).all(|p|
                (-312. ..=312.).contains(&p[0]) && (-236. ..=152.).contains(&p[1])));
            };
        for command in Command::ALL {
            frame.selected = command;
            let enabled = draw(Some(&frame), Some(pending), font)?;
            check_bounds(&enabled);
            frame.enabled &= !command.mask();
            let disabled = draw(Some(&frame), Some(pending), font)?;
            assert!(enabled[1] != disabled[1]);
            check_bounds(&disabled);
            frame.enabled |= command.mask();
        }
        let escape = draw(Some(&frame), None, font)?;
        frame.escape_requested = true;
        assert!(escape[1] != draw(Some(&frame), None, font)?[1]);
        frame.selected = Command::Items;
        frame.enabled &= !Command::Items.mask();
        assert!(draw(Some(&frame), Some(pending), font)?[1] != draw(Some(&frame), None, font)?[1]);
        frame.enabled |= Command::Items.mask();
        frame.selected = Command::Tech;
        assert!(draw(Some(&frame), Some(pending), font)? == draw(Some(&frame), None, font)?);
        frame.selected = Command::Items;
        for slot in 0..4 {
            let mut selection = ActorSelection {
                actor,
                slot,
                name: "R%s with a long literal name".into(),
                eligible: true,
            };
            frame.view = View::User(selection.clone());
            let user = draw(Some(&frame), None, font)?;
            check_bounds(&user);
            // The selector surrounds the same card that renders this party slot.
            let [left, top, right, bottom] = party::card_rect(usize::from(slot));
            let border: Vec<_> = user[0]
                .positions
                .iter()
                .filter(|p| p[1] <= 240. - top)
                .collect();
            assert_eq!(
                border.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                left - 320.
            );
            assert_eq!(
                border
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                right - 320.
            );
            assert_eq!(
                border.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                240. - bottom
            );
            frame.view = View::Ally(selection.clone());
            assert!(user[1] != draw(Some(&frame), None, font)?[1]);
            selection.eligible = false;
            frame.view = View::Ally(selection);
            let rejected = draw(Some(&frame), None, font)?;
            check_bounds(&rejected);
            assert!(user[0] != rejected[0]);
        }
        frame.view = View::Enemy { target: actor };
        for state in [Some(&frame), None] {
            assert!(
                draw(state, Some(pending), font)?
                    .iter()
                    .all(|batch| batch.indices.is_empty())
            );
        }
        Ok(())
    }
}
