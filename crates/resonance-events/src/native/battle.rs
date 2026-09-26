use super::*;
use crate::battle::{DefeatPolicy, Request, Setup};

impl NativeHost<'_> {
    pub(super) fn request_battle(
        &mut self,
        _: NativeCall,
        args: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        require(self.world.battle_request.is_none(), "nested battle request")?;
        require(self.world.party.is_some(), "battle requires a party")?;
        let setup = Setup {
            encounter: u16::try_from(args[0]).map_err(|_| "invalid encounter ID")?,
            arena: u16::try_from(args[1]).map_err(|_| "invalid battle arena ID")?,
            defeat: if args[2] & 1 == 0 {
                DefeatPolicy::GameOver
            } else {
                DefeatPolicy::ResumeEvent
            },
            music: match args[3] {
                -1 | 0 => None,
                id => Some(u16::try_from(id).map_err(|_| "invalid battle music ID")?),
            },
        };
        // fn_8005098C consumes twelve arguments, but reads only the first nine.
        // Nonzero callback/route overrides need their own prepared behavior.
        require(
            args[4..9].iter().all(|&value| value == 0),
            "unsupported battle route overrides",
        )?;
        let operation = self.world.operations.begin()?;
        *self.wait = Some(Wait::Battle(operation.clone()));
        self.world.battle_request = Some(Request { setup, operation });
        Ok(NativeResult::Suspend)
    }
}
