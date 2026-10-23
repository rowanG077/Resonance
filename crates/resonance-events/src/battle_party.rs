//! Optional relaxation of active-lineup checks in field events.
use crate::native::NativeHost;
use symphonia_script::{NativeCall, Op, Program};
use symphonia_script_vm::{Memory, NativeResult};

// Host extension, outside the field script's native service catalogue.
pub(crate) const BATTLE_PARTY_CHECK: u8 = 0xf5;
const ACTIVE_PARTY_SIZE: i32 = 4;

pub fn prepare_field_program(mut program: Program) -> Program {
    // Only specialize the reserve predicate: FindPartyMember(character) > 4.
    // Membership checks and reads of formation slots retain their usual meaning.
    program.specialize_native(
        NativeCall::FindPartyMember as u8,
        BATTLE_PARTY_CHECK,
        &[Op::Push(ACTIVE_PARTY_SIZE), Op::Calculate(0x24)],
    );
    program
}

impl NativeHost<'_> {
    pub(crate) fn battle_party_check(
        &mut self,
        args: &[i32],
        memory: &mut Memory,
    ) -> Result<NativeResult, String> {
        let result = self.party(NativeCall::FindPartyMember, args, memory)?;
        if self
            .world
            .party
            .as_ref()
            .is_some_and(|party| party.settings.preferences.flexible_battle_party)
            && (1..=9).contains(&args[0])
            && let NativeResult::Continue(Some(position)) = result
        {
            return Ok(NativeResult::Continue(Some(
                position.min(ACTIVE_PARTY_SIZE),
            )));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventRuntime, GameWorld, ResourceLibrary,
        party::{Party, tests::data},
    };
    use std::sync::Arc;
    use symphonia_script::Width;

    #[test]
    fn flexible_battle_party_relaxes_only_reserve_checks() {
        for character in [1, 2, 4, 5] {
            for flexible in [false, true] {
                for position in [None, Some(0), Some(3), Some(4), Some(7)] {
                    let data = data();
                    let mut party = Party::new(&data, Default::default()).unwrap();
                    party.formation = (1..=9).filter(|&id| id != character).collect();
                    if let Some(position) = position {
                        party.formation.insert(position, character);
                        party.formation.truncate(8);
                    }
                    party.settings.preferences.flexible_battle_party = flexible;
                    let formation = party.formation.clone();
                    let mut words: Vec<u16> = vec![4, 0, 0, 0];
                    for query in 0..3 {
                        words.extend([0x1200, 0x100 + query * 4]);
                        let arg = if query == 2 {
                            0x10004
                        } else {
                            i32::from(character)
                        };
                        words.extend([
                            0x0200,
                            arg as u16,
                            (arg >> 16) as u16,
                            0x3000,
                            0x4000,
                            0x2000 | NativeCall::FindPartyMember as u16,
                        ]);
                        if query == 1 {
                            words.extend([4, 0x3024]);
                        }
                        // Store the expression result in a separate global for each query.
                        words.extend([0x3010, 0x3000]);
                    }
                    words.push(0x20ff);
                    let bytes: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
                    let program = prepare_field_program(Program::decode(&bytes).unwrap());
                    let events = EventRuntime::with_state(
                        Arc::new(program),
                        Arc::new(ResourceLibrary {
                            session_data: Some(Arc::new(data)),
                            ..Default::default()
                        }),
                        GameWorld {
                            party: Some(party),
                            ..Default::default()
                        },
                        Default::default(),
                    )
                    .unwrap();
                    let read = |address| events.memory().read(address, Width::S32).unwrap();
                    assert_eq!(read(0x100), position.map_or(0, |p| p as i32 + 1));
                    assert_eq!(
                        read(0x104),
                        i32::from(!flexible && position.is_some_and(|p| p >= 4))
                    );
                    assert_eq!(read(0x108), i32::from(formation[4]));
                    assert_eq!(events.world.party.unwrap().formation, formation);
                }
            }
        }
    }
}
