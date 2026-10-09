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
        CreateSealedSavePoint(4) -> () = field;
        CreateRingStation(6) -> () = field;
        CreateTreasureChest(7) -> () = field;
        SetTreasureModel(2) -> () = field;
        SpawnActor(8) -> () = field;
        SpawnEnemyActor(16) -> () = field;
        CreateEnemySource(21) -> () = field;
        SpawnCollisionActor(8) -> () = field;
        SpawnSceneryActor(8) -> () = field;
        DespawnActor(1) -> () = field;
        DespawnActorAfterMovement(1) -> () = dispatch;
        SetActorHeading(2) -> () = field;
        GetActorHeading(1) -> i32 = dispatch;
        IsActorMoving(1) -> i32 = dispatch;
        SetSceneryAnimationRate(3) -> () = field;
        FaceActorAfterMovement(2) -> () = dispatch;
        TransformActorNode(6) -> () = dispatch;
        ConfigureActorBoneRotation(6) -> () = field;
        MoveActorRelative(5) -> () = field;
        MoveActor(5) -> () = field;
        SetActorPathPoint(6) -> () = field;
        SetActorPosition(4) -> () = dispatch;
        GetActorProperty(2) -> i32 = dispatch;
        SetActorProperty(3) -> i32 = dispatch;
        GetItemCount(1) -> i32 = party;
        IsTreasureOpened(1) -> i32 = party;
        MarkTreasureOpened(1) -> () = party;
        GetItemStackLimit(0) -> i32 = party;
        ChangeItemCount(2) -> i32 = party;
        SelectPartyMember(1) -> i32 = field;
        AddPartyMember(1) -> i32 = party;
        RemovePartyMember(1) -> i32 = party;
        FindPartyMember(1) -> i32 = party;
        LearnTitle(1) -> () = party;
        GetTitle(2) -> i32 = party;
        SetCharacterCostume(2) -> i32 = party;
        SetCharacterName(2) -> () = party;
        ConfigureExGem(4) -> () = party;
        BindEffectTexture(3) -> () = field;
        SetActorOrientation(3) -> () = field;
        EquipItem(2) -> () = party;
        GetEquippedItem(2) -> i32 = party;
        UnequipItem(2) -> () = party;
        LearnTechnique(2) -> () = party;
        ForgetRecipe(1) -> () = party;
        HasRecipe(1) -> i32 = party;
        LearnRecipe(1) -> () = party;
        CreateScriptRecord(8) -> () = field;
        CreateAutomaticEventTrigger(8) -> () = field;
        CreateCircleTrigger(6) -> () = field;
        CreateAutomaticCircleTrigger(6) -> () = field;
        CreateConfirmedCircleTrigger(9) -> () = field;
        RemoveAutomaticEventTriggers(1) -> () = field;
        RemoveTouchTriggers(1) -> () = field;
        RemoveConfirmedTriggers(1) -> () = field;
        TriggerExists(2) -> i32 = field;
        CreateTriangleTrigger(11) -> () = field;
        HealParty(1) -> () = party;
        ReleaseResourceInstance(1) -> () = dispatch;
        SpawnEvent(1) -> i32 = dispatch;
        ControlEvent(2) -> () = dispatch;
        CreateScriptRecordVariant(11) -> () = field;
        CreateAreaTrigger(14) -> () = field;
        CreateConfirmedTriangleTrigger(14) -> () = field;
        CreateConfirmedAreaTrigger(17) -> () = field;
        SetTriggerMetadata(4) -> () = field;
        SetTouchTriggerMetadata(4) -> () = field;
        PlayWorldCinematic(6) -> () = field;
        ChangeField(5) -> () = field;
        ConfigureRendering(2) -> () = dispatch;
        ConfigureScreenCopy(2) -> i32 = dispatch;
        PreloadField(1) -> () = field;
        PreloadVoiceBank(2) -> () = field;
        SetSoundReverb(1) -> () = field;
        CreateOverlay(13) -> () = dispatch;
        SetEffectSetting(5) -> () = dispatch;
        AudioCommand(1) -> () = field;
        SetAudioFade(3) -> () = field;
        PlaySoundSimple(2) -> () = field;
        SetActorAmbientSound(4) -> () = field;
        PlayMovieBlocking(1) -> () = dispatch;
        PlayVoice(1) -> () = dispatch;
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
        IsMappedInputDisabled(0) -> i32 = field;
        EnableMappedInput(0) -> () = field;
        SetTransitionMode(2) -> () = dispatch;
        YieldCommand(2) -> () = dispatch;
        RandomMod(1) -> i32 = field;
        SinDegrees(1) -> i32 = dispatch;
        CosDegrees(1) -> i32 = dispatch;
        ScaledSquareRoot(1) -> i32 = dispatch;
        Atan2Degrees(2) -> i32 = dispatch;
        SetRingTimer(1) -> () = party;
        GetRingTimer(0) -> i32 = party;
        GetFieldTicks(0) -> i32 = party;
        SetFieldCountdown(1) -> () = party;
        GetFieldCountdown(0) -> i32 = party;
        ShowChoice(5) -> i32 = dispatch;
        OpenMenu(1) -> i32 = request_menu;
        SetEventBit(1) -> () = field;
        ClearEventBit(1) -> () = field;
        TestEventBit(1) -> i32 = field;
        AddGald(1) -> i32 = party;
        AddGrade(1) -> i32 = party;
        HasTechnique(2) -> i32 = party;
        ForgetTechnique(2) -> () = party;
        ForgetTitle(1) -> () = party;
        ConfigureFigurine(2) -> i32 = party;
        SetEquippedTitle(2) -> i32 = party;
        ResetFieldTicks(0) -> () = party;
        ResetScenarioTicks(0) -> () = party;
        GetScenarioTicks(0) -> i32 = party;
        RecipeProficiency(3) -> i32 = party;
        IsDebugSession(0) -> i32 = dispatch;
        ConfigureSession(2) -> i32 = party;
        ConfigureSorcerersRing(2) -> i32 = party;
        SetPlayerSize(4) -> () = field;
        SnapshotParty(1) -> () = party;
        SetScenarioTimer(3) -> () = field;
        AdjustCharacterAffinity(2) -> i32 = party;
        RankCharacterAffinity(9) -> i32 = party;
        DiscardValue(1) -> () = dispatch;
        GetEventActor(0) -> i32 = dispatch;
        ActorExists(1) -> i32 = dispatch;
        IsSkitViewed(1) -> i32 = party;
        GetScenarioTimerValue(1) -> i32 = field;
        Unknown92(2) -> i32 = field;
        ResolveScriptResource(1) -> i32 = field;
        ReleaseScriptResource(1) -> i32 = field;
        ConfigureActorAnimation(5) -> () = dispatch;
        SetActorAnimationFlags(2) -> () = dispatch;
        ConfigureSceneryAnimation(6) -> () = field;
        ClearSceneryAnimation(2) -> () = field;
        SeekSceneryAnimation(3) -> () = field;
        SetActorAnimationProperty(3) -> i32 = field;
        RumbleController(3) -> () = field;
        ShakeCamera(3) -> () = field;
        SelectCamera(1) -> i32 = field;
        SetCameraProperty(2) -> i32 = field;
        GetCameraProperty(1) -> i32 = field;
        SetCameraEye(4) -> () = field;
        SetCameraTransitionValues(4) -> () = field;
        SelectActor(1) -> () = field;
        SetCameraPosition(3) -> () = field;
        ConfigureCameraParameters(8) -> () = field;
        ConfigureCameraAuxiliary(6) -> () = field;
        ResetCameraBounds(0) -> () = field;
        StopCameraTrack(0) -> () = dispatch;
        PlayCameraTrack(3) -> () = dispatch;
        MapCameraTrackPosition(8) -> () = dispatch;
        ConfigureCameraTrack(2) -> i32 = dispatch;
        MeasureActorGeometry(3) -> i32 = field;
        WaitActorAnimationFrame(2) -> () = dispatch;
        IsActorAnimationFinished(1) -> i32 = dispatch;
        SetActorFace(2) -> () = field;
        SetActorMouth(2) -> () = field;
        FindActorBodyPart(2) -> i32 = field;
        AttachActorToMember(3) -> () = field;
        CreateSceneActor(8) -> () = dispatch;
        SpawnInteractionActor(8) -> () = dispatch;
        FindActorNode(2) -> i32 = dispatch;
        ReadCoordinateRegister(1) -> i32 = dispatch;
        ReadActorLocalOffset(9) -> () = dispatch;
        ReadActorOffset(5) -> () = dispatch;
        StartBattle(12) -> i32 = request_battle;
        Unknown37(3) -> i32 = request_battle;
        StartEnemyBattle(2) -> i32 = request_enemy_battle;
        GetCurrentField(0) -> i32 = field;
        ReadMappedInput(2) -> i32 = field;
        ConfigureBattleControl(2) -> i32 = party;
        ConfigureActorAttachment(7) -> () = field;
        ConfigureActorBoneTranslation(7) -> () = field;
        ConfigureActorBoneScale(7) -> () = field;
        ConfigureActorHeadNeck(7) -> () = field;
        TurnActorHead(6) -> () = field;
        SetActorAnimation(3) -> () = field;
        RaisePartyMemberLevel(2) -> () = party;
        ReadActorAttachment(2) -> () = dispatch;
        CreateParticle(13) -> i32 = dispatch;
        SetEffectProperty(3) -> i32 = dispatch;
        CreateEffectObject(14) -> i32 = field;
        CreateEffectEmitter(18) -> () = field;
        CreateModelParticle(11) -> i32 = field;
        SetModelParticleProperty(3) -> i32 = field;
        MotionCommand(6) -> () = field;
        PlaySound(4) -> () = field;
        PlayActorSound(4) -> () = field;
        ConfigureSound(3) -> () = field;
        SelectAudioBank(1) -> () = field;
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_signatures_match_the_script_analysis_catalog() {
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
            assert_eq!(
                Some(binding.signature.returns_value),
                spec.returns_value,
                "{opcode:#04x}"
            );
        }
    }
}
