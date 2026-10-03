use super::*;
use crate::battle::{DefeatPolicy, Encounter, Setup};

impl NativeHost<'_> {
    pub(super) fn request_enemy_battle(
        &mut self,
        _: NativeCall,
        args: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        let enemy = self
            .world
            .actors
            .get(&args[0])
            .and_then(|a| a.enemy.as_ref())
            .ok_or("battle symbol is missing")?;
        let setup = Setup {
            encounter: Encounter::Pool(enemy.event_parameters[0] as u16),
            arena: enemy.event_parameters[1] as u16,
            defeat: if args[1] & 1 == 0 {
                DefeatPolicy::GameOver
            } else {
                DefeatPolicy::ResumeEvent
            },
            music: None,
            route: [0; 5],
        };
        let request = self.world.request_battle(setup)?;
        *self.wait = Some(Wait::Battle(request.operation));
        Ok(NativeResult::Suspend)
    }

    pub(super) fn request_battle(
        &mut self,
        op: NativeCall,
        args: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        require(self.world.battle_request.is_none(), "nested battle request")?;
        require(self.world.party.is_some(), "battle requires a party")?;
        let (music, route) = if op == NativeCall::Unknown37 {
            (None, [0; 5])
        } else {
            (
                match args[3] {
                    -1 | 0 => None,
                    id => Some(u16::try_from(id).map_err(|_| "invalid battle music ID")?),
                },
                args[4..9].try_into().unwrap(),
            )
        };
        let setup = Setup {
            encounter: Encounter::Formation(
                u16::try_from(args[0]).map_err(|_| "invalid encounter ID")?,
            ),
            arena: u16::try_from(args[1]).map_err(|_| "invalid battle arena ID")?,
            defeat: if args[2] & 1 == 0 {
                DefeatPolicy::GameOver
            } else {
                DefeatPolicy::ResumeEvent
            },
            route,
            music,
        };
        let request = self.world.request_battle(setup)?;
        *self.wait = Some(Wait::Battle(request.operation));
        Ok(NativeResult::Suspend)
    }
}
