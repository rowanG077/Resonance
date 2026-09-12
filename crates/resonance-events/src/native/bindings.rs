//! The supported ABI: each row binds a typed ID, argument count, return shape
//! and service handler. IDs without a row remain unavailable to scripts.
use super::NativeHost;
use symphonia_script::NativeCall::*;
use symphonia_script_vm::{Host, NativeBindings};

macro_rules! bindings {
    (@returns ()) => { false };
    (@returns i32) => { true };
    ($($call:ident($arguments:literal) -> $returns:tt = $handler:ident;)*) => {
        NativeBindings::<Self>::new()$(.register($call as u8, $arguments, bindings!(@returns $returns),
            |host, args, memory| host.invoke($call, args, memory, Self::$handler)))*
    };
}
impl Host for NativeHost<'_> {
    const NATIVES: NativeBindings<Self> = bindings! {
        CloseDialogue(1) -> () = dispatch;
        ConfigureDialogue(8) -> () = dispatch;
        SetDialogueSlotFlag(3) -> () = dispatch;
        CreateSavePoint(4) -> () = field;
        SpawnActor(8) -> () = field;
        DespawnActor(1) -> () = field;
        SetActorHeading(2) -> () = field;
        MoveActor(5) -> () = field;
        SetActorPosition(4) -> () = dispatch;
        GetActorProperty(2) -> i32 = dispatch;
        SetActorProperty(3) -> i32 = dispatch;
        GetItemCount(1) -> i32 = party;
        ChangeItemCount(2) -> i32 = party;
        SelectPartyMember(1) -> i32 = field;
        AddPartyMember(1) -> i32 = party;
        FindPartyMember(1) -> i32 = party;
        LearnTitle(1) -> () = party;
        GetTitle(2) -> i32 = party;
        SetActorOrientation(3) -> () = field;
        EquipItem(2) -> () = party;
        UnequipItem(2) -> () = party;
        LearnTechnique(2) -> () = party;
        ForgetRecipe(1) -> () = party;
        HasRecipe(1) -> i32 = party;
        LearnRecipe(1) -> () = party;
        CreateScriptRecord(8) -> () = field;
        HealParty(1) -> () = party;
        SpawnEvent(1) -> i32 = dispatch;
        ControlEvent(2) -> () = dispatch;
        CreateScriptRecordVariant(11) -> () = field;
        CreateAreaTrigger(14) -> () = field;
        SetTriggerMetadata(4) -> () = field;
        SetTouchTriggerMetadata(4) -> () = field;
        ChangeField(5) -> () = field;
        ConfigureRendering(2) -> () = dispatch;
        PreloadField(1) -> () = field;
        CreateOverlay(13) -> () = dispatch;
        SetEffectSetting(5) -> () = dispatch;
        AudioCommand(1) -> () = field;
        SetAudioFade(3) -> () = field;
        PlaySoundSimple(2) -> () = field;
        PlayMovieBlocking(1) -> () = dispatch;
        PlayMovie(1) -> () = dispatch;
        CreatePortrait(13) -> () = skit;
        LoadPortrait(1) -> i32 = skit;
        WaitMediaPosition(1) -> () = skit;
        ScalePortrait(3) -> () = skit;
        SetPortraitTalking(2) -> () = skit;
        SetSkitSubtitle(3) -> () = skit;
        PlaySkit(3) -> () = request_skit;
        PreviewSkit(1) -> () = request_skit;
        ReturnFieldControl(1) -> () = field;
        DisableMappedInput(0) -> () = field;
        EnableMappedInput(0) -> () = field;
        SetTransitionMode(2) -> () = dispatch;
        YieldCommand(2) -> () = dispatch;
        RandomMod(1) -> i32 = field;
        ShowChoice(5) -> i32 = dispatch;
        OpenMenu(1) -> i32 = request_menu;
        SetEventBit(1) -> () = field;
        ClearEventBit(1) -> () = field;
        TestEventBit(1) -> i32 = field;
        AddGald(1) -> i32 = party;
        ConfigureSession(2) -> i32 = party;
        SetScenarioTimer(3) -> () = field;
        AdjustCharacterAffinity(2) -> i32 = party;
        DiscardValue(1) -> () = dispatch;
        ActorExists(1) -> i32 = dispatch;
        GetScenarioTimerValue(1) -> i32 = field;
        Unknown92(2) -> i32 = field;
        ResolveScriptResource(1) -> i32 = field;
        ReleaseScriptResource(1) -> i32 = field;
        ConfigureActorAnimation(5) -> () = dispatch;
        SetActorAnimationProperty(3) -> i32 = field;
        SelectCamera(1) -> i32 = field;
        SetCameraProperty(2) -> i32 = field;
        GetCameraProperty(1) -> i32 = field;
        SetCameraEye(4) -> () = field;
        SetCameraTransitionValues(4) -> () = field;
        SelectActor(1) -> () = field;
        SetCameraPosition(3) -> () = field;
        ResetCameraBounds(0) -> () = field;
        PlayCameraTrack(3) -> () = dispatch;
        MeasureActorGeometry(3) -> i32 = field;
        SetActorFace(2) -> () = field;
        SetActorMouth(2) -> () = field;
        FindActorBodyPart(2) -> i32 = field;
        AttachActorToMember(3) -> () = field;
        CreateSceneActor(8) -> () = dispatch;
        FindActorNode(2) -> i32 = dispatch;
        ReadCoordinateRegister(1) -> i32 = dispatch;
        ConfigureBattleControl(2) -> i32 = party;
        ConfigureActorAttachment(7) -> () = field;
        ConfigureActorHeadNeck(7) -> () = field;
        SetActorAnimation(3) -> () = field;
        RaisePartyMemberLevel(2) -> () = party;
        ReadActorAttachment(2) -> () = dispatch;
        CreateParticle(13) -> i32 = dispatch;
        SetEffectProperty(3) -> i32 = dispatch;
        CreateEffectObject(14) -> i32 = field;
        MotionCommand(6) -> () = field;
        PlaySound(4) -> () = field;
        ConfigureSound(3) -> () = field;
        SelectAudioBank(1) -> () = field;
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_signatures_agree_with_the_independent_abi_catalog() {
        let catalog = symphonia_script::semantics::NativeRegistry::gqseaf();
        let bindings = NativeHost::NATIVES;
        for opcode in 0..=u8::MAX {
            let Some(binding) = bindings.get(opcode) else {
                continue;
            };
            let spec = catalog
                .get(opcode)
                .expect("registered call is missing from the catalog");
            assert!(!spec.control_flow);
            assert_eq!(
                binding.signature.arguments,
                spec.arguments.len(),
                "{opcode:#04x}"
            );
            if let Some(returns) = spec.returns_value {
                assert_eq!(binding.signature.returns_value, returns, "{opcode:#04x}");
            }
        }
    }
}
