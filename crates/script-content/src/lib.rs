//! Authored behaviors and definitions generated for cooking and development tools.
//! Runtime reads the user's cooked scripts; this crate is not a runtime fallback.
use std::{collections::BTreeMap, fmt::Write};
use symphonia_constants::*;

include!(concat!(env!("OUT_DIR"), "/sources.rs"));
pub const PREVIEW_BINDINGS: &str = include_str!("../../../scripts/preview/bindings.json");

fn enumeration<T: KnownId>(source: &mut String) {
    documentation(source, "", T::DOCUMENTATION);
    writeln!(source, "pub enum {} {{", T::TYPE_NAME).unwrap();
    for &id in T::ALL {
        documentation(source, "    ", id.documentation());
        writeln!(source, "    {} = {},", id.name(), id.script_value()).unwrap();
    }
    source.push_str("}\n");
}

fn documentation(source: &mut String, indent: &str, notes: &str) {
    for line in notes.lines() {
        let line = line.strip_prefix(' ').unwrap_or(line);
        if line.is_empty() {
            writeln!(source, "{indent}//").unwrap();
        } else {
            writeln!(source, "{indent}// {line}").unwrap();
        }
    }
}

/// Definitions are generated from the same declarations used by Rust consumers.
pub fn definitions() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    macro_rules! module {
        ($path:literal, $($id:ty),+ $(,)?) => {{
            let mut source = String::from("script library;\n");
            $(enumeration::<$id>(&mut source);)+
            files.insert(concat!("std/", $path, ".sym").into(), source);
        }};
    }
    module!(
        "characters",
        CharacterId,
        PartyCharacterId,
        TitleId,
        ExSkillId
    );
    module!("techniques", TechniqueId);
    module!("recipes", RecipeId);
    module!(
        "resources",
        BuiltinFieldModelId,
        FieldModelId,
        Field0519ModelId,
        Field0520ModelId,
        Field0521ModelId,
        FieldAnimationId,
        FieldOverlayId,
        VoiceArchiveReference
    );
    module!("skits", SkitId, PortraitId, PortraitTalkMode);
    module!(
        "scenario",
        WaitCondition,
        ActorProperty,
        CameraProperty,
        CameraSelector,
        CameraTrackStart,
        CameraTrackRepeat,
        CameraTrackCommand,
        WorldTransferId,
        CameraPathCommand,
        TransitionMode,
        HeadingOperation,
        MovementFacing,
        AnimationProperty,
        AnimationSlot,
        AnimationFlag,
        AnimationControl,
        AnimationSource,
        TitleSelection,
        TitleLookup,
        PartySelection,
        PartySnapshotOperation,
        FormationSlotSelector,
        SorcerersRingMode,
        RingLightningColor,
        RingCallColor,
        RingBubblePhase,
        ExGemUpdate,
        MusicControl,
        SoundControl,
        AmbientSoundControl,
        SoundVolume,
        SoundBinding,
        SpatialSoundVolume,
        SoundSlotOperation,
        DialogueFlag,
        DialogueAnchor,
        DialogueSlot,
        MessageControl,
        DialogueGrid,
        DialogueFlagOperation,
        DialogueStatus,
        ActorBehavior,
        SceneryLayer,
        SceneryAnimationChannel,
        PartyRecoveryMode,
        SessionCommand,
        BattleRule,
        RenderingCommand,
        RenderingToggle,
        ScreenCopyCommand,
        SessionToggle,
        VoiceBankSlot,
        VoiceControl,
        EmoteId,
        GeometryQuery,
        CoordinateRegister,
        EventKind,
        TriggerKind,
        EventControl,
        ActorSelector,
        OverlayActorId,
        BoneControllerSlot,
        BodyPartId,
        EmitterRecipe,
        ParticleRecipe,
        EffectProperty,
        ModelParticleProperty
    );
    module!("items", ItemId, EquipmentSlot);
    module!("monsters", MonsterId);
    module!("figurines", FigurineId, FigurineOperation);
    module!("fields", FieldId, FieldSystemCommand);
    module!("music", MusicId);
    module!(
        "battle",
        BattleEventId,
        FormationId,
        BattleOutcome,
        BattleDefeatPolicy,
        BattleMusicSelection,
        ArenaId,
        BattleSceneId,
        BattleSkillId,
        BattleCameraCommand,
        BattleControlSlot,
        BattleControlMode
    );
    module!("locations", WorldLocationId);
    module!("effects", EffectId);
    module!(
        "treasure",
        TreasureKind,
        TreasureFlag,
        TreasureRewardEncoding
    );
    module!(
        "story",
        StoryFlag,
        ManaLeafHerbStage,
        ShadowPactStage,
        CelsiusPactStage,
        GnomePactStage,
        KateRescueStage,
        SideEventState,
        StoryFlagBase,
        Field0130FlagBase,
        Field0135FlagBase,
        Field0193FlagBase,
        Field0194FlagBase,
        Field0195FlagBase,
        Field0197FlagBase,
        Field0210FlagBase,
        Field0211FlagBase,
        Field0214FlagBase,
        Field0215FlagBase,
        Field0216FlagBase,
        Field0217FlagBase,
        Field0361FlagBase,
        Field0362FlagBase,
        Field0366FlagBase,
        DebugActorSlotFlag,
        DebugLightningFlag,
        WorldFlag,
        Field0006Flag,
        Field0007Flag,
        Field0008Flag,
        Field0009Flag,
        Field0011Flag,
        Field0013Flag,
        Field0014Flag,
        Field0015Flag,
        Field0016Flag,
        Field0033Flag,
        Field0034Flag,
        Field0068Flag,
        Field0071Flag,
        Field0075Flag,
        Field0076Flag,
        Field0077Flag,
        Field0078Flag,
        Field0081Flag,
        Field0082Flag,
        Field0083Flag,
        Field0084Flag,
        Field0089Flag,
        Field0091Flag,
        Field0092Flag,
        Field0093Flag,
        Field0095Flag,
        Field0096Flag,
        Field0097Flag,
        Field0111Flag,
        Field0112Flag,
        Field0115Flag,
        Field0116Flag,
        Field0117Flag,
        Field0118Flag,
        Field0125Flag,
        Field0126Flag,
        Field0128Flag,
        Field0129Flag,
        Field0130Flag,
        Field0131Flag,
        Field0132Flag,
        Field0133Flag,
        Field0134Flag,
        Field0135Flag,
        Field0136Flag,
        Field0138Flag,
        Field0139Flag,
        Field0143Flag,
        Field0152Flag,
        Field0155Flag,
        Field0156Flag,
        Field0168Flag,
        Field0169Flag,
        Field0170Flag,
        Field0171Flag,
        Field0193Flag,
        Field0197Flag,
        Field0198Flag,
        Field0200Flag,
        Field0201Flag,
        Field0202Flag,
        Field0203Flag,
        Field0205Flag,
        Field0206Flag,
        Field0208Flag,
        Field0210Flag,
        Field0211Flag,
        Field0213Flag,
        Field0214Flag,
        Field0215Flag,
        Field0217Flag,
        Field0218Flag,
        Field0220Flag,
        Field0226Flag,
        Field0227Flag,
        Field0228Flag,
        Field0229Flag,
        Field0230Flag,
        Field0232Flag,
        Field0236Flag,
        Field0237Flag,
        Field0238Flag,
        Field0239Flag,
        Field0243Flag,
        Field0250Flag,
        Field0252Flag,
        Field0253Flag,
        Field0254Flag,
        Field0257Flag,
        Field0260Flag,
        Field0261Flag,
        Field0264Flag,
        Field0265Flag,
        Field0267Flag,
        Field0268Flag,
        Field0269Flag,
        Field0270Flag,
        Field0271Flag,
        Field0272Flag,
        Field0273Flag,
        Field0275Flag,
        Field0276Flag,
        Field0278Flag,
        Field0279Flag,
        Field0280Flag,
        Field0282Flag,
        Field0284Flag,
        Field0300Flag,
        Field0309Flag,
        Field0317Flag,
        Field0322Flag,
        Field0323Flag,
        Field0324Flag,
        Field0326Flag,
        Field0327Flag,
        Field0361Flag,
        Field0362Flag,
        Field0363Flag,
        Field0364Flag,
        Field0366Flag,
        Field0367Flag,
        Field0368Flag,
        Field0380Flag,
        Field0390Flag,
        Field0391Flag,
        Field0392Flag,
        Field0399Flag,
        Field0400Flag,
        Field0401Flag,
        Field0402Flag,
        Field0411Flag,
        Field0412Flag,
        Field0413Flag,
        Field0425Flag,
        Field0434Flag,
        Field0436Flag,
        Field0437Flag,
        Field0438Flag,
        Field0439Flag,
        Field0440Flag,
        Field0441Flag,
        Field0442Flag,
        Field0445Flag,
        Field0447Flag,
        Field0448Flag,
        Field0449Flag,
        Field0450Flag,
        Field0452Flag,
        Field0453Flag,
        Field0455Flag,
        Field0456Flag,
        Field0466Flag,
        Field0467Flag,
        Field0468Flag,
        Field0469Flag,
        Field0475Flag,
        Field0476Flag,
        Field0477Flag,
        Field0478Flag,
        Field0479Flag,
        Field0481Flag,
        Field0482Flag,
        Field0485Flag,
        Field0495Flag,
        Field0497Flag,
        Field0502Flag,
        Field0503Flag,
        Field0505Flag,
        Field0506Flag,
        Field0508Flag,
        Field0509Flag,
        Field0513Flag,
        Field0514Flag,
        Field0515Flag,
        Field0518Flag,
        Field0519Flag,
        Field0520Flag,
        Field0521Flag,
        Field0522Flag,
        Field0530Flag,
        GlobalId,
        PalmacostaGlobalId,
        ByteOffset,
        HalfwordOffset,
        StoryStage,
        Field0006StoryStage,
        Field0221StoryStage,
        Field0351StoryStage,
        Field0370StoryStage,
        Field0371StoryStage,
        ColetteAngelStage,
        FireSealStage,
        WaterSealStage,
        WindSealStage,
        LightSealStage,
        BoltzmanTowerKeyStage,
        PietroInfiltrationStage,
        AsgardRanchEarlyRouteStage,
        AsgardRanchLaterRouteStage,
        BookOfRegenerationStage,
        LinkiteTreeStage,
        LuinRebuildingStage,
        PalmacostaRanchAssaultStage,
        PalmacostaRanchDorrStage,
        AsgardRanchStage
    );
    module!(
        "world",
        WorldId,
        WorldActorId,
        MountId,
        MusicBankId,
        WorldInteraction,
        SylvarantMarkerId,
        TetheAllaMarkerId,
        CaravanStopId
    );
    let mut nodes = String::from("script library;\n");
    for &(symbol, name) in MODEL_NODES {
        writeln!(nodes, "pub const {symbol}: string = {name:?};").unwrap();
    }
    files.insert("std/model_nodes.sym".into(), nodes);
    files
}

/// Paths relative to the cooked `scripts/` directory.
pub fn files() -> BTreeMap<String, String> {
    let mut files = definitions();
    for &(path, source) in AUTHORED_FILES {
        assert!(
            files.insert(path.into(), source.into()).is_none(),
            "authored source shadows generated definition: {path}"
        );
    }
    files
}

pub fn modules() -> BTreeMap<String, String> {
    files()
        .into_iter()
        .filter_map(|(path, source)| {
            path.strip_suffix(".sym")
                .map(|path| (path.replace('/', "::"), source))
        })
        .collect()
}

/// Names come from the final model. Repeated labels use the model service's
/// occurrence selectors; distinct labels that sanitize alike keep distinct symbols.
pub fn node_definitions(names: &[&str]) -> String {
    let mut counts = BTreeMap::new();
    for &name in names {
        *counts.entry(name).or_insert(0) += 1;
    }
    let mut occurrences = BTreeMap::new();
    let mut symbols = std::collections::BTreeSet::new();
    let mut source = String::from("script library;\n");
    for (index, &name) in names.iter().enumerate() {
        let occurrence = occurrences.entry(name).or_insert(0);
        let mut selector = name.replace('#', "##");
        if counts[name] > 1 {
            write!(selector, "#{occurrence}").unwrap();
        }
        *occurrence += 1;
        let mut symbol = String::new();
        for part in name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
        {
            let mut chars = part.chars();
            symbol.extend(chars.next().unwrap().to_uppercase());
            symbol.extend(chars);
        }
        if symbol.is_empty() || symbol.starts_with(|c: char| c.is_ascii_digit()) {
            symbol.insert_str(0, "Node");
        }
        if counts[name] > 1 {
            write!(symbol, "{}", *occurrence - 1).unwrap();
        }
        while !symbols.insert(symbol.clone()) {
            write!(symbol, "_{index}").unwrap();
        }
        writeln!(source, "pub const {symbol}: string = {selector:?};").unwrap();
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_story_notes_keep_provenance_and_unidentified_bits_visible() {
        let files = definitions();
        let story = &files["std/story.sym"];
        let mut module_notes = String::new();
        documentation(&mut module_notes, "", Field0482Flag::DOCUMENTATION);
        assert!(story.contains(&module_notes));
        let notes = Field0482Flag::RetainedCleanupSuppressed.documentation();
        let mut member = notes
            .lines()
            .map(|note| format!("    // {}\n", note.trim()))
            .collect::<String>();
        member.push_str("    RetainedCleanupSuppressed = 196,");
        assert!(story.contains(&member));
        // Explanations do not turn unknown bits into supported identities.
        assert!(Field0482Flag::try_from(191).is_err());
        assert!(Field0254Flag::try_from(152).is_err());
    }

    #[test]
    fn documentation_lines_remain_comments_even_when_they_contain_source_syntax() {
        let mut source = String::new();
        documentation(
            &mut source,
            "    ",
            " heading\n\npub enum Fake { Value = 1 }\n",
        );
        assert_eq!(
            source,
            "    // heading\n    //\n    // pub enum Fake { Value = 1 }\n"
        );
    }

    #[test]
    fn generated_definitions_are_compilable_and_keep_unknown_ids_usable() {
        let mut sources = modules();
        for (path, _) in definitions() {
            let module = path.strip_suffix(".sym").unwrap().replace('/', "::");
            symphonia_script_compiler::check(&module, &sources, &[]).unwrap();
        }
        sources.insert(
            "example".into(),
            r#"script library;
            use std::items::ItemId;
            use std::fields::FieldId;
            use std::locations::WorldLocationId;
            pub fn item() -> i32 { return i32(ItemId::MemoryGem); }
            pub fn field() -> i32 { return i32(FieldId::Iselia0332); }
            pub fn landmark() -> i32 { return i32(WorldLocationId::Iselia); }
            pub fn mod_added() -> i32 { return 60000; }
        "#
            .into(),
        );
        symphonia_script_compiler::check("example", &sources, &[]).unwrap();
    }

    #[test]
    fn model_symbols_handle_duplicates_punctuation_and_escaped_selectors() {
        let source = node_definitions(&[
            "neck",
            "hand",
            "hand",
            "a#b",
            "a-b",
            "a_b",
            "123",
            "quote\"\\",
        ]);
        assert!(source.contains("Neck: string = \"neck\""));
        assert!(source.contains("Hand0: string = \"hand#0\""));
        assert!(source.contains("Hand1: string = \"hand#1\""));
        assert!(source.contains("\"a##b\""));
        symphonia_script_compiler::check("nodes", &BTreeMap::from([("nodes".into(), source)]), &[])
            .unwrap();
    }
}
