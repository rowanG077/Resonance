use super::{NativeHost, require};
use symphonia_script::NativeCall;
use symphonia_script_vm::{Memory, NativeResult};

impl NativeHost<'_> {
    pub(super) fn stage(
        &mut self,
        call: NativeCall,
        arguments: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        let stage = self
            .world
            .battle_event
            .as_mut()
            .ok_or("battle event is not active")?;
        match call {
            NativeCall::StageCommand => {
                stage.camera.command(
                    arguments
                        .try_into()
                        .map_err(|_| "invalid camera arguments")?,
                )?;
                // The measured absolute camera forms return zero after admission.
                Ok(NativeResult::Continue(Some(0)))
            }
            NativeCall::StageOpcode148 if arguments == [4, 4, 0, 0, 0, 0, 0] => {
                stage.activate_reserve = true;
                Ok(NativeResult::Continue(Some(0)))
            }
            NativeCall::StageOpcode148 => {
                stage
                    .fade
                    .command(arguments.try_into().map_err(|_| "invalid fade arguments")?)?;
                Ok(NativeResult::Continue(Some(0)))
            }
            NativeCall::StageOpcode206 => {
                require(
                    arguments[3..].iter().all(|&value| value == 0),
                    "unsupported expression options",
                )?;
                require(
                    matches!(arguments[0], 1 | 2),
                    "unsupported expression channel",
                )?;
                let frame = u8::try_from(arguments[2]).map_err(|_| "invalid expression frame")?;
                let actor = self
                    .world
                    .actors
                    .get_mut(&arguments[1])
                    .ok_or("battle event actor is not bound")?;
                if arguments[0] == 1 {
                    actor.appearance.face = crate::Face::Frame(frame);
                    actor.appearance.eyes = None;
                } else {
                    actor.appearance.mouth = Some(crate::Face::Frame(frame));
                }
                // Direct controlled observations established zero for this service.
                Ok(NativeResult::Continue(Some(0)))
            }
            _ => Err("battle event stage service is not prepared".into()),
        }
    }
}
