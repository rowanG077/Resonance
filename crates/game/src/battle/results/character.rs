//! Z rotates player one's character through the automatic formation slots.
use super::*;
use resonance_battle::{Control, ControllerPort};

impl Candidate {
    pub(in crate::battle) fn change_character(
        &mut self,
        battle: &mut Battle,
        controller: ControllerPort,
    ) -> Result<Option<ActorId>> {
        if controller != ControllerPort::One {
            return Ok(None);
        }
        let first = &battle.actors()[self.setup.actors[0].0.index()];
        if !matches!(first.control, Control::Manual | Control::SemiAuto) {
            return Ok(None);
        }
        let slots: Vec<_> = self
            .setup
            .actors
            .iter()
            .enumerate()
            .filter_map(|(slot, &(id, _))| {
                let actor = &battle.actors()[id.index()];
                (actor.availability != ActorAvailability::Absent
                    && (slot == 0 || actor.control == Control::Auto))
                    .then_some(slot)
            })
            .collect();
        if slots.len() < 2 || slots[0] != 0 {
            return Ok(None);
        }
        let mut next = self.setup.actors.clone();
        for (index, &slot) in slots.iter().enumerate() {
            next[slot] = self.setup.actors[slots[(index + 1) % slots.len()]];
        }
        battle.reorder_party(&next.iter().map(|&(actor, _)| actor).collect::<Vec<_>>())?;
        for (slot, &(_, character)) in next.iter().enumerate() {
            self.party.formation[slot] = character as u8;
        }
        self.setup.actors = next;
        self.menu_party = None;
        self.refresh_menu_roster(battle);
        Ok(Some(self.setup.actors[0].0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{command, results::test_support::*};

    fn fixture(formation: &[u8], modes: [u8; 4]) -> Result<PreparedFixture> {
        let mut f = prepared_fixture_with_party(formation, 2, &[], |party, _, _| {
            party.settings.battle_controls = modes;
            Ok(())
        })?;
        enter_battle(&mut f.candidate, &mut f.battle)?;
        command_visit(
            &mut f,
            command::Input {
                open: edge(),
                ..Default::default()
            },
        )?;
        Ok(f)
    }

    fn change(f: &mut PreparedFixture) -> Result<Vec<command::Event>> {
        Ok(command_visit(
            f,
            command::Input {
                change_character: true,
                ..Default::default()
            },
        )?
        .1)
    }

    #[test]
    #[ignore = "requires cooked battle assets; GameCube Z rotation regression"]
    fn z_rotates_party_and_preserves_slot_modes_through_battle_completion() -> Result<()> {
        let mut f = fixture(&[1, 2, 3, 4, 5], [1, 2, 2, 2])?;
        let random = f.battle.random_state();
        let ticks = f.battle.ledger().elapsed_ticks;
        for expected in [
            [2, 3, 4, 1, 5],
            [3, 4, 1, 2, 5],
            [4, 1, 2, 3, 5],
            [1, 2, 3, 4, 5],
        ] {
            let before = f.battle.snapshot();
            assert_eq!(change(&mut f)?, [command::Event::Cue(1)]);
            assert_eq!(f.candidate.persistent_party().formation, expected);
            assert_eq!(
                f.candidate.persistent_party().settings.battle_controls,
                [1, 2, 2, 2]
            );
            let menu = f.lifecycle.command_frame().unwrap();
            assert_eq!(menu.actor, f.candidate.setup.actors[0].0);
            assert_eq!(menu.view, command::View::Strip);
            for (slot, &(id, character)) in f.candidate.setup.actors.iter().enumerate() {
                let actor = &f.battle.actors()[id.index()];
                assert_eq!(character as u8, expected[slot]);
                assert_eq!(actor.control_slot.index(), slot);
                assert_eq!(
                    actor.control,
                    if slot == 0 {
                        Control::SemiAuto
                    } else {
                        Control::Auto
                    }
                );
                assert_eq!(actor.position, before.actors[id.index()].position);
            }
            assert_eq!(f.battle.snapshot().targets, before.targets);
            assert_eq!(f.battle.random_state(), random);
            assert_eq!(f.battle.ledger().elapsed_ticks, ticks);
        }
        change(&mut f)?;
        let outcome = escape(&mut f.candidate, &mut f.battle)?;
        let completed = f.candidate.finish(&f.battle, &outcome)?;
        assert_eq!(completed.party.formation, [2, 3, 4, 1, 5]);
        assert_eq!(completed.party.settings.battle_controls, [1, 2, 2, 2]);
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked battle assets; GameCube Z multiplayer regression"]
    fn z_keeps_other_manual_characters_fixed_and_rebinds_menu_selection() -> Result<()> {
        let mut f = prepared_fixture_with_party(&[1, 2, 3, 4], 2, &[], |party, _, _| {
            party.settings.battle_controls = [1, 0, 2, 2];
            party.members[2].hp = 0;
            Ok(())
        })?;
        enter_battle(&mut f.candidate, &mut f.battle)?;
        command_visit(
            &mut f,
            command::Input {
                open: edge(),
                ..Default::default()
            },
        )?;
        change(&mut f)?;
        assert_eq!(f.candidate.persistent_party().formation, [3, 2, 4, 1]);
        let other_player = &f.battle.actors()[1];
        assert_eq!(other_player.control, Control::Manual);
        assert_eq!(other_player.control_slot, ControllerPort::Two);
        let actor = f.candidate.setup.actors[0].0;
        assert_eq!(f.battle.actors()[actor.index()].hp, 0);
        command_visit(
            &mut f,
            command::Input {
                control_mode: true,
                ..Default::default()
            },
        )?;
        assert_eq!(
            f.lifecycle.command_frame().unwrap().view,
            command::View::Control(command::ControlSelection {
                actor,
                slot: 0,
                mode: Control::SemiAuto,
            })
        );
        command_visit(
            &mut f,
            command::Input {
                control_mode: true,
                ..Default::default()
            },
        )?;
        assert_eq!(f.battle.actors()[actor.index()].control, Control::Auto);
        assert_eq!(
            f.candidate.persistent_party().settings.battle_controls,
            [2, 0, 2, 2]
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires cooked battle assets; GameCube Z eligibility regression"]
    fn z_requires_a_manual_first_slot_and_an_automatic_alternative() -> Result<()> {
        for modes in [[1, 0, 0, 0], [2, 2, 2, 2]] {
            let mut f = fixture(&[1, 2, 3, 4], modes)?;
            assert!(change(&mut f)?.is_empty());
            assert_eq!(f.candidate.persistent_party().formation, [1, 2, 3, 4]);
        }
        let mut f = fixture(&[1], [0, 2, 2, 2])?;
        assert!(change(&mut f)?.is_empty());
        let mut f = fixture(&[1, 2, 3, 4], [0, 0, 2, 2])?;
        let events = command_visit(
            &mut f,
            command::Input {
                controller: ControllerPort::Two,
                change_character: true,
                ..Default::default()
            },
        )?
        .1;
        assert!(events.is_empty());
        assert_eq!(f.candidate.persistent_party().formation, [1, 2, 3, 4]);
        Ok(())
    }
}
