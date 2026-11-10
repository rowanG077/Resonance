use std::{collections::BTreeMap, fs};
use symphonia_script::{Op, Program, scenario as binary};
use symphonia_script_compiler::{
    ScriptKind,
    scenario::{self, Definitions},
};

fn definitions() -> Definitions {
    Definitions::from_sources(&resonance_script_content::modules()).unwrap()
}

fn battle_request() -> &'static str {
    "eval native::start_battle_simple(4, 13, 1);"
}

#[test]
fn sealed_save_points_name_only_the_persisted_unlock_flag() {
    let definitions = definitions();
    let body = "native::create_sealed_save_point(s16(850), u16(491), s32(73), u16(850));
        native::create_sealed_save_point(0, 0, 0, (849 + 1));
        native::create_sealed_save_point(0, 0, 0, 9999);
        eval 850;";
    let bytes = scenario::compile("sealed-save-point", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "x: 850,",
        "y: u16(491),",
        "z: s32(73),",
        "unlock_flag: u16(story::StoryFlag::TrietRuins0220MemoryCircleUnlocked)",
        "encoded(story::StoryFlag::TrietRuins0220MemoryCircleUnlocked, (849 + 1))",
        "unlock_flag: 9999",
        "eval 850;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    let flags = result
        .identities
        .iter()
        .filter(|id| id.native == "create_sealed_save_point")
        .collect::<Vec<_>>();
    assert_eq!(flags.len(), 3);
    assert!(
        flags
            .iter()
            .all(|id| { id.domain == "story::StoryFlag" && id.parameter == "unlock_flag" })
    );
    assert_eq!(
        flags
            .iter()
            .map(|id| (id.value, id.symbolic))
            .collect::<Vec<_>>(),
        [(850, true), (850, true), (9999, false)]
    );
    let formatted = symphonia_script_compiler::format("sealed-save-point", &result.source).unwrap();
    assert_eq!(
        scenario::compile("sealed-save-point", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn battle_options_keep_native_policy_encodings_widths_and_arena_domains() {
    let definitions = definitions();
    let body = "eval native::start_enemy_battle(200, s8(123));
        eval native::start_battle_simple(4, u16(13), s32(0));
        eval native::start_battle(4, s16(2), u16(1), 0, 0, 0, 0, 0, 0, 0, 0, 0);
        eval native::start_battle_simple(4, 74, 2);
        eval native::start_enemy_battle(200, (120 + 3)); eval 123;";
    let bytes = scenario::compile("battle-options", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "defeat_policy: battle::BattleDefeatPolicy::ResumeEventEncoded123",
        "arena_id: u16(battle::ArenaId::IseliaClassroom)",
        "defeat_flags: s32(battle::BattleDefeatPolicy::GameOver)",
        "arena_id: s16(battle::ArenaId::SylvarantForest)",
        "defeat_flags: u16(battle::BattleDefeatPolicy::ResumeEvent)",
        "arena_id: 74",
        "defeat_flags: 2",
        "encoded(battle::BattleDefeatPolicy::ResumeEventEncoded123, (120 + 3))",
        "eval 123;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    let formatted = symphonia_script_compiler::format("battle-options", &result.source).unwrap();
    assert_eq!(
        scenario::compile("battle-options", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn battle_music_distinguishes_automatic_encodings_from_tracks_and_stop_controls() {
    let definitions = definitions();
    let mut body = String::new();
    for music in [
        "s16(-1)", "u16(0)", "s32(87)", "(86 + 1)", "-2", "32768", "65535", "30000",
    ] {
        body.push_str(&format!(
            "eval native::start_battle(4, 13, 0, {music}, 0, 0, 0, 0, 0, 0, 0, 0);"
        ));
    }
    body.push_str("eval -1; eval 0;");
    let bytes = scenario::compile("battle-music", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "music_id: s16(battle::BattleMusicSelection::Automatic)",
        "music_id: u16(battle::BattleMusicSelection::AutomaticEncodedZero)",
        "music_id: s32(music::MusicId::TheStruggleToSurvive)",
        "encoded(music::MusicId::TheStruggleToSurvive, (86 + 1))",
        "music_id: -2",
        "music_id: 32768",
        "music_id: 65535",
        "music_id: 30000",
        "eval -1;",
        "eval 0;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(!result.source.contains("MusicControl::"));
    assert!(!result.source.contains("MusicId::Placeholder0"));
    assert_eq!(
        scenario::compile("battle-music", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn battle_option_names_follow_supplied_definitions_without_builtin_fallbacks() {
    let definitions = definitions();
    let body = "eval native::start_enemy_battle(200, 123);
        eval native::start_battle(4, 13, 1, -1, 0, 0, 0, 0, 0, 0, 0, 0);
        eval native::start_battle(4, 13, 0, 87, 0, 0, 0, 0, 0, 0, 0, 0);";
    let bytes = scenario::compile("battle-options", &source(body), &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::battle").unwrap();
    *module = module
        .replace("ResumeEventEncoded123 = 123,", "ModResume = 123,")
        .replace("Automatic = -1,", "ModAutomatic = -1,");
    let module = modules.get_mut("std::music").unwrap();
    *module = module.replace("TheStruggleToSurvive = 87,", "ModTheme = 87,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    for expected in [
        "BattleDefeatPolicy::ModResume",
        "BattleMusicSelection::ModAutomatic",
        "MusicId::ModTheme",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        scenario::compile("battle-options", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::battle").unwrap();
    *module = module
        .replace("ModResume = 123,", "")
        .replace("ModAutomatic = -1,", "ModAutomatic = -2,");
    let module = modules.get_mut("std::music").unwrap();
    *module = module.replace("ModTheme = 87,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    for expected in ["defeat_policy: 123", "music_id: -1", "music_id: 87"] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        scenario::compile("battle-options", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn every_battle_entry_api_establishes_the_same_result_origin() {
    let definitions = definitions();
    for request in [
        "eval native::start_enemy_battle(200, 1);",
        battle_request(),
        "eval native::start_battle(4, 13, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);",
    ] {
        let text = source(&format!("{request} eval (memory::i32[0x0024] == 3);"));
        let bytes = scenario::compile("battle-api", &text, &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(result.source.contains("battle::BattleOutcome::Defeat"));
        assert_eq!(
            scenario::compile("battle-api", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn battle_outcomes_name_proven_results_and_copies_without_typing_the_shared_word() {
    let definitions = definitions();
    let body = format!(
        "{} eval (memory::i32[0x0024] == u16(3));
        eval (memory::i32[0x0810] = memory::i32[0x0024]);
        eval (memory::i32[0x0024] = 2);
        eval (memory::i32[0x0810] == s32(2));
        eval (memory::i32[0x0024] == 3); eval (memory::i32[0x0020] == 2);
        eval (memory::i16[0x0810] == 2); eval 3;",
        battle_request()
    );
    let bytes = scenario::compile("battle-outcomes", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "memory::i32[0x0024] == u16(battle::BattleOutcome::Defeat)",
        "memory::i32[0x0810] == s32(battle::BattleOutcome::Victory)",
        "memory::i32[0x0024] = 2",
        "memory::i32[0x0024] == 3",
        "memory::i32[0x0020] == 2",
        "memory::i16[0x0810] == 2",
        "eval 3;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.native == "memory::battle_outcome")
            .count(),
        2
    );
    let formatted = symphonia_script_compiler::format("battle-outcomes", &result.source).unwrap();
    assert_eq!(
        scenario::compile("battle-outcomes", &formatted, &definitions).unwrap(),
        bytes
    );

    let inline = source("eval (native::start_battle_simple(4, 13, 1) == 2);");
    let bytes = scenario::compile("inline", &inline, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(result.source.contains("battle::BattleOutcome::Victory"));
    assert_eq!(
        scenario::compile("inline", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn battle_result_proofs_stop_at_calls_aliases_arithmetic_and_unknown_producers() {
    let definitions = definitions();
    for barrier in [
        "native::discard_value(0);",
        "call barrier;",
        "eval (memory::i8[0x0025] = 0);",
        "eval (memory::i16[0x0023] = 0);",
        "eval (memory::i32[0x0900][memory::i32[0x0904]] = 0);",
        "eval ++(memory::i32[0x0024]);",
        "eval (memory::i32[0x0024] += 0);",
        "eval (memory::i32[0x0024] = 2);",
    ] {
        let text = source(&format!(
            "{} {barrier} eval (memory::i32[0x0024] == 2);",
            battle_request()
        ))
        .replace("auxiliary:", "task barrier() { return; }\nauxiliary:");
        let bytes = scenario::compile("barrier", &text, &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.native == "memory::battle_outcome"),
            "crossed {barrier}: {}",
            result.source
        );
        assert!(result.source.contains("memory::i32[0x0024] == 2"));
    }
    let bytes = scenario::compile(
        "unknown",
        &source("eval (memory::i32[0x0024] == 3);"),
        &definitions,
    )
    .unwrap();
    assert!(
        !scenario::decompile(&bytes, &definitions)
            .unwrap()
            .source
            .contains("BattleOutcome::")
    );
}

#[test]
fn battle_result_joins_loops_and_external_entries_require_every_path_to_agree() {
    let definitions = definitions();
    for (body, named) in [
        (
            format!(
                "if (memory::i32[0x0900]) {{ {} }} else {{ {} }}",
                battle_request(),
                battle_request()
            ),
            true,
        ),
        (
            format!(
                "if (memory::i32[0x0900]) {{ {} }} else {{ eval (memory::i32[0x0024] = 2); }}",
                battle_request()
            ),
            false,
        ),
        (
            format!("while (memory::i32[0x0900]) {{ {} }}", battle_request()),
            false,
        ),
        (
            format!(
                "{} while (memory::i32[0x0900]) {{ eval --(memory::i32[0x0900]); }}",
                battle_request()
            ),
            true,
        ),
        (
            format!(
                "{} while (memory::i32[0x0900]) {{ eval (memory::i32[0x0024] = 0); }}",
                battle_request()
            ),
            false,
        ),
    ] {
        let bytes = scenario::compile(
            "joins",
            &source(&format!("{body} eval (memory::i32[0x0024] == 2);")),
            &definitions,
        )
        .unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert_eq!(
            result.source.contains("battle::BattleOutcome::Victory"),
            named,
            "{}",
            result.source
        );
        assert_eq!(
            scenario::compile("joins", &result.source, &definitions).unwrap(),
            bytes
        );
    }
    let text = format!(
        "script scenario; code_base = 10;
        data {{ word(10); pc(entry); word(0); word(1); dword(1); dword(2000); pc32(check); }}
        task entry() {{ {} goto check; }}
        task check() {{ eval (memory::i32[0x0024] == 2); end; }}",
        battle_request()
    );
    let bytes = scenario::compile("external", &text, &definitions).unwrap();
    assert!(
        !scenario::decompile(&bytes, &definitions)
            .unwrap()
            .source
            .contains("battle::BattleOutcome::Victory")
    );
}

#[test]
fn battle_outcome_names_follow_supplied_definitions_and_keep_unknown_results_numeric() {
    let definitions = definitions();
    let text = source(&format!(
        "{} eval (memory::i32[0x0024] == 2); eval (memory::i32[0x0024] <= 0); eval (memory::i32[0x0024] == 4);",
        battle_request()
    ));
    let bytes = scenario::compile("names", &text, &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::battle").unwrap();
    *module = module.replace("Victory = 2,", "ModVictory = 2,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("battle::BattleOutcome::ModVictory"));
    assert!(result.source.contains("memory::i32[0x0024] <= 0"));
    assert!(result.source.contains("memory::i32[0x0024] == 4"));
    assert_eq!(
        scenario::compile("names", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::battle").unwrap();
    *module = module.replace("ModVictory = 2,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("memory::i32[0x0024] == 2"));
    assert_eq!(
        scenario::compile("names", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn battle_formations_name_enemy_groups_without_rewriting_aliases_or_other_operands() {
    let definitions = definitions();
    let body = "eval native::start_battle_simple(u16(4), 13, 0);
        eval native::start_battle(s32(7), 74, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        eval native::start_battle_simple(6, 13, 0);
        eval native::start_battle_simple((3 + 1), 13, 0);
        eval native::start_battle_simple(1000, 13, 0);
        eval native::start_battle_simple(-1, 13, 0); eval 4;";
    let bytes = scenario::compile("formations", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "formation_id: u16(battle::FormationId::Vidarr)",
        "formation_id: s32(battle::FormationId::WhipMasterAndBowmanVariant0007)",
        "formation_id: battle::FormationId::WhipMasterX2Variant0006",
        "encoded(battle::FormationId::Vidarr, (3 + 1))",
        "formation_id: 1000",
        "formation_id: -1",
        "eval 4;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "battle::FormationId" && id.symbolic)
            .count(),
        4
    );
    let formatted = symphonia_script_compiler::format("formations", &result.source).unwrap();
    assert_eq!(
        scenario::compile("formations", &formatted, &definitions).unwrap(),
        bytes
    );
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::battle").unwrap();
    *module = module.replace("Vidarr = 4,", "ModBoss = 4,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("battle::FormationId::ModBoss"));
    assert_eq!(
        scenario::compile("formations", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::battle").unwrap();
    *module = module.replace("ModBoss = 4,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("formation_id: u16(4)"));
    assert!(!result.source.contains("ModBoss"));
    assert_eq!(
        scenario::compile("formations", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn trigger_query_kinds_use_record_types_and_supplied_names() {
    let definitions = definitions();
    let body = "eval native::trigger_exists(u16(1), 1302);
        eval native::trigger_exists(s32(2), 1302);
        eval native::trigger_exists(3, 1302);
        eval native::trigger_exists(0, 1302);
        eval native::trigger_exists(4, 1302); eval 3;";
    let bytes = scenario::compile("trigger-kinds", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "u16(scenario::TriggerKind::Touch)",
        "s32(scenario::TriggerKind::Automatic)",
        "kind: scenario::TriggerKind::Confirmed",
        "kind: 0",
        "kind: 4",
        "eval 3;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(!result.source.contains("EventKind::"));
    assert_eq!(
        scenario::compile("trigger-kinds", &result.source, &definitions).unwrap(),
        bytes
    );
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("Confirmed = 3,", "ModConfirmed = 3,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(
        result
            .source
            .contains("scenario::TriggerKind::ModConfirmed")
    );
    assert_eq!(
        scenario::compile("trigger-kinds", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("Touch = 1,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("kind: u16(1)"));
    assert!(!result.source.contains("EventKind::"));
    assert_eq!(
        scenario::compile("trigger-kinds", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn dialogue_timer_and_yellow_flags_preserve_masks_widths_and_custom_definitions() {
    let definitions = definitions();
    let body = "native::configure_dialogue(0, 1, -2, 4, 0, 0, 0, 0);
        native::configure_dialogue(0, u16(512), -2, 4, 0, 0, 0, 0);
        native::configure_dialogue(0, s32(513), -2, 4, 0, 0, 0, 0);
        native::configure_dialogue(0, s32(8704), -2, 4, 0, 0, 0, 0);
        eval 512;";
    let bytes = scenario::compile("dialogue-flags", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        result
            .source
            .contains("flags: scenario::DialogueFlag::TimedClose")
    );
    assert!(
        result
            .source
            .contains("flags: u16(scenario::DialogueFlag::Yellow)")
    );
    assert!(result.source.contains(
        "s32(flags(scenario::DialogueFlag::TimedClose, scenario::DialogueFlag::Yellow))"
    ));
    assert!(result.source.contains("eval 512;"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "scenario::DialogueFlag" && id.value == 8704 && id.partial_mask)
    );
    assert!(
        result
            .source
            .contains("scenario::DialogueFlag::Yellow, 0x2000")
    );
    let formatted = symphonia_script_compiler::format("dialogue-flags", &result.source).unwrap();
    assert_eq!(
        scenario::compile("dialogue-flags", &formatted, &definitions).unwrap(),
        bytes
    );

    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module
        .replace("TimedClose = 1,", "ModTimer = 1,")
        .replace("Yellow = 512,", "ModTint = 512,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("scenario::DialogueFlag::ModTimer"));
    assert!(result.source.contains("scenario::DialogueFlag::ModTint"));
    assert_eq!(
        scenario::compile("dialogue-flags", &result.source, &custom).unwrap(),
        bytes
    );

    let module = modules.get_mut("std::scenario").unwrap();
    *module = module
        .replace("ModTimer = 1,", "")
        .replace("ModTint = 512,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(!result.source.contains("ModTimer"));
    assert!(!result.source.contains("ModTint"));
    for value in [1, 512] {
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.domain == "scenario::DialogueFlag"
                    && id.value == value
                    && !id.symbolic)
        );
    }
    assert_eq!(
        scenario::compile("dialogue-flags", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn emitter_recipes_name_only_the_constructor_selector_and_follow_supplied_definitions() {
    use symphonia_constants::EmitterRecipe;
    let definitions = definitions();
    let mut body = String::new();
    for recipe in EmitterRecipe::ALL {
        body.push_str(&format!(
            "native::create_effect_emitter(3001, 14, 0, 0, 14, u16({}), 0, 0, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0);\n",
            recipe.raw()
        ));
    }
    for unknown in [-1, 2, 3, 5, 6, 7, 8, 35, 69, 76, 999] {
        body.push_str(&format!(
            "native::create_effect_emitter(3001, 0, 0, 0, 0, s32({unknown}), 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);\n"
        ));
    }
    let bytes = scenario::compile("emitter-recipes", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for recipe in EmitterRecipe::ALL {
        assert!(result.source.contains(&format!(
            "recipe: u16(scenario::EmitterRecipe::{})",
            recipe.name()
        )));
    }
    assert!(result.source.contains("model: 14"));
    assert!(result.source.contains("parameter0: 14"));
    assert!(result.source.contains("x: 14"));
    for unknown in [-1, 2, 3, 5, 6, 7, 8, 35, 69, 76, 999] {
        assert!(result.source.contains(&format!("recipe: s32({unknown})")));
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::EmitterRecipe" && id.symbolic)
            .count(),
        EmitterRecipe::ALL.len()
    );
    assert_eq!(
        scenario::compile("emitter-recipes", &result.source, &definitions).unwrap(),
        bytes
    );

    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("Portal = 14", "ModPortal = 14");
    assert!(module.contains("ModPortal = 14"));
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("scenario::EmitterRecipe::ModPortal"));
    assert_eq!(
        scenario::compile("emitter-recipes", &result.source, &custom).unwrap(),
        bytes
    );

    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("ModPortal = 14,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("recipe: u16(14)"));
    assert!(!result.source.contains("ModPortal"));
    assert_eq!(
        scenario::compile("emitter-recipes", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn emitter_parameter_properties_preserve_slots_widths_and_supplied_names() {
    let definitions = definitions();
    let mut body = String::new();
    for slot in 0..10 {
        let property = 113 + slot;
        body.push_str(&format!(
            "eval native::set_actor_property(3001, u16({property}), -1);\n\
             eval native::get_actor_property(3001, s32({property}));\n"
        ));
    }
    body.push_str("eval native::set_actor_property(113, 123, 122); eval 113;");
    let bytes = scenario::compile("emitter-slots", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for slot in 0..10 {
        let name = format!("scenario::ActorProperty::EmitterParameter{slot}");
        assert!(result.source.contains(&format!("u16({name})")));
        assert!(result.source.contains(&format!("s32({name})")));
    }
    assert!(
        result
            .source
            .contains("actor_id: 113, property: 123, value: 122")
    );
    assert!(result.source.contains("eval 113;"));
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::ActorProperty" && id.symbolic)
            .count(),
        20
    );
    let formatted = symphonia_script_compiler::format("emitter-slots", &result.source).unwrap();
    assert_eq!(
        scenario::compile("emitter-slots", &formatted, &definitions).unwrap(),
        bytes
    );

    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("EmitterParameter2 = 115", "CustomEmitterSlot = 115");
    assert!(module.contains("CustomEmitterSlot = 115"));
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(
        result
            .source
            .contains("scenario::ActorProperty::CustomEmitterSlot")
    );
    assert!(!result.source.contains("EmitterParameter2"));
    assert_eq!(
        scenario::compile("emitter-slots", &result.source, &custom).unwrap(),
        bytes
    );

    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("CustomEmitterSlot = 115,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("property: u16(115)"));
    assert!(!result.source.contains("CustomEmitterSlot"));
    assert_eq!(
        scenario::compile("emitter-slots", &result.source, &missing).unwrap(),
        bytes
    );
}

fn bank_clear_body(base: i32) -> String {
    (0..50)
        .map(|slot| {
            let value = base + slot;
            let argument = match slot % 3 {
                0 => value.to_string(),
                1 => format!("u16({value})"),
                _ => format!("s32({value})"),
            };
            format!("native::clear_event_bit({argument});\n")
        })
        .collect()
}

#[test]
fn complete_bank_resets_name_storage_slots_and_preserve_every_call_and_width() {
    let definitions = definitions();
    for base in [150, 1700, 1800] {
        let bytes =
            scenario::compile("reset", &source(&bank_clear_body(base)), &definitions).unwrap();
        let result = scenario::decompile_with_context(
            &bytes,
            &definitions,
            scenario::Context::FieldScene(33),
        )
        .unwrap();
        let name = format!("story::StoryFlagBase::WorldResetSceneBank{base}");
        assert!(result.source.contains(&format!("flag_slot({name}, 0)")));
        assert!(
            result
                .source
                .contains(&format!("u16(flag_slot({name}, 1))"))
        );
        assert!(
            result
                .source
                .contains(&format!("s32(flag_slot({name}, 2))"))
        );
        assert!(!result.source.contains("StairTransferPending"));
        assert_eq!(
            result.source.matches("native::clear_event_bit(").count(),
            50
        );
        assert_eq!(result.identities.len(), 50);
        for (slot, id) in result.identities.iter().enumerate() {
            assert_eq!(id.native, "bank_reset::clear_event_bit");
            assert_eq!(id.parameter, format!("bit[{slot}]"));
            assert_eq!(id.domain, "story::StoryFlagBase");
            assert_eq!(id.value, base);
            assert!(id.symbolic);
        }
        let formatted = symphonia_script_compiler::format("reset", &result.source).unwrap();
        assert_eq!(
            scenario::compile("reset", &formatted, &definitions).unwrap(),
            bytes
        );
        assert_eq!(
            symphonia_script_compiler::format("reset", &formatted).unwrap(),
            formatted
        );
    }
}

#[test]
fn flag_slot_is_a_checked_single_literal_constructor() {
    let definitions = definitions();
    let annotated = source(
        "native::clear_event_bit(flag_slot(story::StoryFlagBase::WorldResetSceneBank150, 3));
         eval u16(flag_slot(1700, 49)); eval s32(flag_slot(1800, 0));",
    );
    let literal = source("native::clear_event_bit(153); eval u16(1749); eval s32(1800);");
    assert_eq!(
        scenario::compile("slot", &annotated, &definitions).unwrap(),
        scenario::compile("slot", &literal, &definitions).unwrap()
    );
    for invalid in [
        "flag_slot(-1, 0)",
        "flag_slot(150, -1)",
        "flag_slot(2147483647, 1)",
        "flag_slot(9223372036854775807, 1)",
        "flag_slot(150, memory::i32[0x0810])",
        "flag_slot(150, native::get_item_count(1))",
        "flag_slot(150, (1 + 2))",
        "s8(flag_slot(150, 0))",
        "u16(flag_slot(65535, 1))",
    ] {
        assert!(
            scenario::compile("slot", &source(&format!("eval {invalid};")), &definitions).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn incomplete_or_interrupted_clears_do_not_establish_whole_bank_resets() {
    let definitions = definitions();
    let full = bank_clear_body(150);
    for body in [
        full.replace("native::clear_event_bit(u16(199));", ""),
        full.replace(
            "native::clear_event_bit(u16(151));",
            "native::clear_event_bit(152);",
        ),
        full.replace(
            "native::clear_event_bit(u16(151));",
            "native::set_event_bit(151);",
        ),
        full.replace(
            "native::clear_event_bit(u16(151));",
            "native::clear_event_bit(memory::i32[0x0810]);",
        ),
        full.replace(
            "native::clear_event_bit(u16(151));",
            "eval 0; native::clear_event_bit(151);",
        ),
        format!(
            "goto interior; {}",
            full.replace(
                "native::clear_event_bit(u16(151));",
                "interior: native::clear_event_bit(151);"
            )
        ),
        bank_clear_body(623),
    ] {
        let bytes = scenario::compile("not-reset", &source(&body), &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(!result.source.contains("flag_slot("), "{}", result.source);
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.native.starts_with("bank_reset::"))
        );
    }
    let body = format!("goto reset; reset: {full}");
    let bytes = scenario::compile("reset-head", &source(&body), &definitions).unwrap();
    assert!(
        scenario::decompile(&bytes, &definitions)
            .unwrap()
            .source
            .contains("flag_slot(")
    );
}

#[test]
fn reset_bank_annotations_require_the_supplied_base_definitions() {
    let bytes = scenario::compile("reset", &source(&bank_clear_body(150)), &definitions()).unwrap();
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum StoryFlagBase").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "pub enum Unrelated { SameValue = 150 }");
    let without_base = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &without_base).unwrap();
    assert!(!result.source.contains("flag_slot("));
    assert_eq!(
        scenario::compile("reset", &result.source, &without_base).unwrap(),
        bytes
    );
    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum StoryFlagBase { ModSceneBank = 150 }\n");
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &renamed).unwrap();
    assert!(
        result
            .source
            .contains("flag_slot(story::StoryFlagBase::ModSceneBank, 49)")
    );
    assert_eq!(
        scenario::compile("reset", &result.source, &renamed).unwrap(),
        bytes
    );
}

#[test]
fn world_and_field_flags_give_the_same_bit_distinct_scoped_names() {
    use scenario::Context;
    let definitions = definitions();
    let bytes = scenario::compile(
        "shared-bit",
        &source(
            "native::set_event_bit(u16(1749));
            eval native::test_event_bit((1740 + 9));
            native::clear_event_bit(1749);
            eval (memory::i32[0x0810] = s32(1749));
            eval native::test_event_bit(memory::i32[0x0810]);
            native::set_event_bit((1749 + memory::i32[0x0820]));
            eval native::get_item_count(1749);",
        ),
        &definitions,
    )
    .unwrap();
    for (context, domain, name) in [
        (
            Context::World,
            "story::WorldFlag",
            "TrietEntranceArrivalPending",
        ),
        (
            Context::FieldScene(485),
            "story::Field0485Flag",
            "WorldEntranceArrivalPending",
        ),
        (
            Context::FieldScene(530),
            "story::Field0530Flag",
            "RouteVia0129Selected",
        ),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let symbol = format!("{domain}::{name}");
        assert!(result.source.contains(&format!("u16({symbol})")));
        assert!(
            result
                .source
                .contains(&format!("encoded({symbol}, (1740 + 9))"))
        );
        assert!(
            result
                .source
                .contains(&format!("memory::i32[0x0810] = s32({symbol})"))
        );
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == domain)
                .count(),
            4
        );
        assert!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == domain)
                .all(|id| id.value == 1749 && id.symbolic)
        );
        assert!(result.source.contains("(1749 + memory::i32[0x0820])"));
        assert!(result.source.contains("get_item_count(item_id: 1749)"));
        let formatted = symphonia_script_compiler::format("shared-bit", &result.source).unwrap();
        assert_eq!(
            scenario::compile("shared-bit", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [
        Context::Unspecified,
        Context::Field,
        Context::Battle,
        Context::Skit,
        Context::FieldScene(33),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(result.source.contains("set_event_bit(bit: u16(1749))"));
        assert!(!result.source.contains("TrietEntranceArrivalPending"));
        assert!(!result.source.contains("RouteVia0129Selected"));
    }
}

#[test]
fn world_flag_aliases_come_only_from_the_selected_world_enum() {
    use scenario::Context;
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum WorldFlag").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "pub enum OtherWorldFlag { Unrelated = 1749 }");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let bytes = scenario::compile(
        "world-flag",
        &source("native::set_event_bit(1749);"),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile_with_context(&bytes, &definitions, Context::World).unwrap();
    assert!(result.source.contains("set_event_bit(bit: 1749)"));
    assert!(!result.source.contains("Unrelated"));
    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum WorldFlag { ModTrietArrival = 1749 }\n");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile_with_context(&bytes, &definitions, Context::World).unwrap();
    assert!(result.source.contains("story::WorldFlag::ModTrietArrival"));
    assert_eq!(
        scenario::compile("world-flag", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn field_flag_names_require_the_matching_scene_and_preserve_shared_bits() {
    use scenario::Context;
    let definitions = definitions();
    let bytes = scenario::compile(
        "scene-flags",
        &source(
            "native::set_event_bit(u16(150));
            eval native::test_event_bit((149 + 1));
            native::clear_event_bit(150);
            eval (memory::i32[0x0810] = s32(150));
            eval native::test_event_bit(memory::i32[0x0810]);
            native::set_event_bit((150 + memory::i32[0x0820]));
            native::set_event_bit(151);
            eval native::get_item_count(150);",
        ),
        &definitions,
    )
    .unwrap();
    for (field, name) in [
        (33, "StairTransferPending"),
        (309, "OracleConversationAffinityBonusClaimed"),
    ] {
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let domain = format!("story::Field{field:04}Flag");
        let symbol = format!("{domain}::{name}");
        assert!(result.source.contains(&format!("u16({symbol})")));
        assert!(
            result
                .source
                .contains(&format!("encoded({symbol}, (149 + 1))"))
        );
        assert!(
            result
                .source
                .contains(&format!("memory::i32[0x0810] = s32({symbol})"))
        );
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == domain)
                .count(),
            4
        );
        assert!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == domain)
                .all(|id| id.value == 150 && id.symbolic)
        );
        // Neither an indexed range, an unreviewed bit, nor an item ID acquires
        // a scene alias from its coincidentally equal literal.
        assert!(
            result
                .source
                .contains("StoryFlagBase::WorldResetSceneBank150 + memory::i32[0x0820]")
        );
        assert!(result.source.contains("set_event_bit(bit: 151)"));
        assert!(
            !result
                .source
                .contains(&format!("get_item_count(item_id: {symbol}"))
        );
        let formatted = symphonia_script_compiler::format("scene-flags", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scene-flags", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [
        Context::Unspecified,
        Context::Field,
        Context::World,
        Context::Battle,
        Context::Skit,
        // Use an unassigned ID as real fields gain reviewed meanings.
        Context::FieldScene(u16::MAX),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("Field0033Flag::"));
        assert!(!result.source.contains("Field0309Flag::"));
        assert!(result.source.contains("set_event_bit(bit: u16(150))"));
    }
}

#[test]
fn field_flag_names_follow_cooked_definitions_without_an_embedded_fallback() {
    use scenario::Context;
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum Field0033Flag").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "pub enum OtherField0033Flag { NotAFlag = 150 }");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let bytes = scenario::compile(
        "scene-flags",
        &source("native::set_event_bit(150);"),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(33)).unwrap();
    assert!(result.source.contains("set_event_bit(bit: 150)"));
    assert!(!result.source.contains("NotAFlag"));
    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum Field0033Flag { ModStairTransfer = 150 }\n");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(33)).unwrap();
    assert!(
        result
            .source
            .contains("story::Field0033Flag::ModStairTransfer")
    );
    assert_eq!(
        scenario::compile("scene-flags", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn exploration_transfer_marks_keep_the_caravan_target_and_require_world_scope() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "world-marks",
        &source(
            "native::set_event_bit(u16(167));
        native::change_field(247, 24, -143, -6, 175);
        native::set_event_bit(172);
        native::set_event_bit(191);
        native::set_event_bit(193);
        native::set_event_bit(164);",
        ),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::World).unwrap();
    for expected in [
        "u16(story::WorldFlag::Exploration07Caravan0247TransferMarked)",
        "story::WorldFlag::Exploration12Caravan0247TransferMarked",
        "story::WorldFlag::Exploration31Caravan0247TransferMarked",
        "story::WorldFlag::TrietCaravan0247TransferMarked",
        "field_id: fields::FieldId::NovasCaravan0247",
        "set_event_bit(bit: 164)",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "story::StoryFlag" && id.value == 164 && !id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("world-marks", &result.source).unwrap();
    assert_eq!(
        scenario::compile("world-marks", &formatted, &definitions).unwrap(),
        bytes
    );

    for context in [
        scenario::Context::FieldScene(214),
        scenario::Context::FieldScene(482),
        scenario::Context::Field,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("Caravan0247TransferMarked"));
        if context == scenario::Context::FieldScene(214) {
            assert!(
                result
                    .source
                    .contains("story::Field0214Flag::LeaderLowBitSaveMiswrite")
            );
        }
        if context == scenario::Context::FieldScene(482) {
            for value in [191, 193] {
                assert!(
                    result
                        .identities
                        .iter()
                        .any(|id| id.domain == "story::StoryFlag"
                            && id.value == value
                            && !id.symbolic)
                );
            }
        }
        assert_eq!(
            scenario::compile("world-marks", &result.source, &definitions).unwrap(),
            bytes
        );
    }

    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    assert!(story.contains("Exploration07Caravan0247TransferMarked = 167"));
    *story = story.replace(
        "Exploration07Caravan0247TransferMarked = 167",
        "OriginalTransferForMod = 167",
    );
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &renamed, scenario::Context::World).unwrap();
    assert!(
        result
            .source
            .contains("story::WorldFlag::OriginalTransferForMod")
    );
    assert_eq!(
        scenario::compile("world-marks", &result.source, &renamed).unwrap(),
        bytes
    );
}

#[test]
fn asgard_leader_miswrite_alias_preserves_wrong_addresses_and_exact_scene_scope() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "leader-save",
        &source(
            "if ((memory::i32[0x082c] % 2)) {
            native::set_event_bit(u16(172));
        } else {
            native::clear_event_bit(170);
        }
        eval native::test_event_bit(171);",
        ),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(214))
            .unwrap();
    assert!(
        result
            .source
            .contains("u16(story::Field0214Flag::LeaderLowBitSaveMiswrite)")
    );
    assert!(
        result
            .source
            .contains("clear_event_bit(bit: story::Field0214Flag::RestoredFieldLeaderBit2)")
    );
    assert!(result.source.contains("memory::i32[0x082C] % 2"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "story::Field0214Flag"
                && id.value == 172
                && id.native == "set_event_bit"
                && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("leader-save", &result.source).unwrap();
    assert_eq!(
        scenario::compile("leader-save", &formatted, &definitions).unwrap(),
        bytes
    );

    for context in [
        scenario::Context::FieldScene(215),
        scenario::Context::World,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("LeaderLowBitSaveMiswrite"));
        if context == scenario::Context::World {
            assert!(
                result
                    .source
                    .contains("story::WorldFlag::Exploration12Caravan0247TransferMarked")
            );
        } else {
            assert!(
                result
                    .identities
                    .iter()
                    .any(|id| id.domain == "story::StoryFlag" && id.value == 172 && !id.symbolic)
            );
        }
        if context == scenario::Context::FieldScene(215) {
            assert!(
                result
                    .source
                    .contains("story::Field0215Flag::RestoredFieldLeaderBit1")
            );
        }
        assert_eq!(
            scenario::compile("leader-save", &result.source, &definitions).unwrap(),
            bytes
        );
    }

    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    assert!(story.contains("LeaderLowBitSaveMiswrite = 172"));
    *story = story.replace(
        "LeaderLowBitSaveMiswrite = 172",
        "OriginalSaveBugForMod = 172",
    );
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &renamed, scenario::Context::FieldScene(214))
            .unwrap();
    assert!(
        result
            .source
            .contains("story::Field0214Flag::OriginalSaveBugForMod")
    );
    assert_eq!(
        scenario::compile("leader-save", &result.source, &renamed).unwrap(),
        bytes
    );
}

#[test]
fn scene_paths_select_only_explicit_four_digit_field_directories() {
    use scenario::Context;
    use std::path::Path;
    for (path, expected) in [
        (
            "local/assets/fields/asgard-0033/scripts/scenario.sym",
            Context::FieldScene(33),
        ),
        (
            "fields/field-0309/scripts/scenario.ssb",
            Context::FieldScene(309),
        ),
        ("fields/unknown/scripts/scenario.sym", Context::Field),
        ("fields/asgard-33/scripts/scenario.sym", Context::Field),
        ("fields/asgard-00033/scripts/scenario.sym", Context::Field),
        ("fields/-0033/scripts/scenario.sym", Context::Field),
        ("fields/asgard-00x3/scripts/scenario.sym", Context::Field),
        ("world/scripts/scenario.sym", Context::World),
        ("battle/events/asgard-0033/scenario.sym", Context::Battle),
        ("skits/asgard-0033.sym", Context::Skit),
        (
            "/tmp/world/cooked/fields/asgard-0033/scripts/scenario.sym",
            Context::FieldScene(33),
        ),
        (
            "/tmp/fields/cooked/world/scripts/scenario.sym",
            Context::World,
        ),
        ("fields/world/scripts/scenario.sym", Context::Field),
        ("fields/fields/scripts/scenario.sym", Context::Field),
        (
            "/tmp/skits/fields/asgard-0033/scripts/scenario.sym",
            Context::FieldScene(33),
        ),
        ("skits/battle/scenario.sym", Context::Skit),
        ("skits/fields/scenario.sym", Context::Skit),
        ("asgard-0033/scenario.sym", Context::Unspecified),
    ] {
        assert_eq!(Context::from_path(Path::new(path)), expected, "{path}");
    }
}

#[test]
fn indexed_flag_families_name_only_declared_bases_and_preserve_arithmetic() {
    let definitions = definitions();
    let text = source(
        "eval native::test_event_bit((s32(623) + memory::i32[0x0810]));
        native::set_event_bit((memory::i32[0x0810] + u16(623)));
        native::clear_event_bit(((620 + 3) + memory::i32[0x0810]));
        native::set_event_bit((623 - memory::i32[0x0810]));
        native::set_event_bit((623 * memory::i32[0x0810]));
        native::set_event_bit((104 + memory::i32[0x0810]));
        eval native::get_item_count((623 + memory::i32[0x0810]));
        native::set_event_bit(623);",
    );
    let bytes = scenario::compile("indexed-flags", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    let base = "story::StoryFlagBase::TempleOfDarknessGatheredShadows";
    for expected in [
        format!("s32({base}) + memory::i32[0x0810]"),
        format!("memory::i32[0x0810] + u16({base})"),
        format!("encoded({base}, (620 + 3)) + memory::i32[0x0810]"),
        "(623 - memory::i32[0x0810])".into(),
        "(623 * memory::i32[0x0810])".into(),
        "(104 + memory::i32[0x0810])".into(),
        "get_item_count(item_id: (623 + memory::i32[0x0810]))".into(),
        "set_event_bit(bit: story::StoryFlag::TempleOfDarknessShadow2301Gathered)".into(),
    ] {
        assert!(
            result.source.contains(&expected),
            "{expected}: {}",
            result.source
        );
    }
    let bases: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.domain == "story::StoryFlagBase")
        .collect();
    assert_eq!(bases.len(), 3);
    assert!(bases.iter().all(|id| id.value == 623 && id.symbolic));
    assert!(bases.iter().all(|id| id.native.starts_with("indexed::")));
    assert!(
        !result
            .identities
            .iter()
            .any(|id| { id.domain == "story::StoryFlag" && id.native.starts_with("indexed::") })
    );
    let formatted = symphonia_script_compiler::format("indexed-flags", &result.source).unwrap();
    assert_eq!(
        scenario::compile("indexed-flags", &formatted, &definitions).unwrap(),
        bytes
    );
    assert_eq!(
        symphonia_script_compiler::format("indexed-flags", &formatted).unwrap(),
        formatted
    );
}

#[test]
fn dog_and_long_range_flag_banks_keep_individual_flags_and_scalars_separate() {
    let definitions = definitions();
    let text = source(
        "eval native::test_event_bit((s32(1214) + memory::i32[0x0810]));
        native::set_event_bit((900 + memory::i32[0x0810]));
        native::set_event_bit((memory::i32[0x0810] + u16(911)));
        native::clear_event_bit(((1200 + 14) + memory::i32[0x0810]));
        native::set_event_bit(1214); native::set_event_bit(900); native::set_event_bit(911);
        eval native::get_item_count((1214 + memory::i32[0x0810]));
        native::set_event_bit((1214 - memory::i32[0x0810]));
        eval (911 + memory::i32[0x0810]);",
    );
    let bytes = scenario::compile("dog-map-banks", &text, &definitions).unwrap();
    for context in [
        scenario::Context::FieldScene(6),
        scenario::Context::Field,
        scenario::Context::World,
        scenario::Context::Battle,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(story::StoryFlagBase::DogNamingBank) + memory::i32[0x0810]",
            "story::StoryFlagBase::LongRangeSylvarantBank + memory::i32[0x0810]",
            "memory::i32[0x0810] + u16(story::StoryFlagBase::LongRangeTetheallaBank)",
            "encoded(story::StoryFlagBase::DogNamingBank, (1200 + 14)) + memory::i32[0x0810]",
            "story::StoryFlag::DogBobNamed",
            "story::StoryFlag::LongRangeIseliaRegionUnlocked",
            "story::StoryFlag::LongRangeMeltokioRegionFlag1",
            "get_item_count(item_id: (1214 + memory::i32[0x0810]))",
            "(1214 - memory::i32[0x0810])",
            "eval (911 + memory::i32[0x0810]);",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        let uses: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.domain == "story::StoryFlagBase")
            .collect();
        assert_eq!(uses.len(), 4);
        assert!(
            uses.iter()
                .all(|id| id.symbolic && id.native.starts_with("indexed::"))
        );
        let formatted = symphonia_script_compiler::format("dog-map-banks", &result.source).unwrap();
        assert_eq!(
            scenario::compile("dog-map-banks", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn dog_and_map_flag_bank_names_follow_optional_selected_definitions() {
    let text = source(
        "native::set_event_bit((1214 + memory::i32[0x0810])); native::set_event_bit((900 + memory::i32[0x0810])); native::set_event_bit((911 + memory::i32[0x0810]));",
    );
    let bytes = scenario::compile("dog-map-banks", &text, &definitions()).unwrap();
    for mode in ["renamed", "missing", "moved"] {
        let mut modules = resonance_script_content::modules();
        let story = modules.get_mut("std::story").unwrap();
        for (name, value) in [
            ("DogNamingBank", 1214),
            ("LongRangeSylvarantBank", 900),
            ("LongRangeTetheallaBank", 911),
        ] {
            let old = format!("{name} = {value},");
            assert!(story.contains(&old));
            let new = if mode == "missing" {
                String::new()
            } else {
                format!(
                    "Mod{name} = {},",
                    if mode == "moved" {
                        value + 10000
                    } else {
                        value
                    }
                )
            };
            *story = story.replace(&old, &new);
        }
        let selected = Definitions::from_sources(&modules).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &selected, scenario::Context::Field).unwrap();
        assert_eq!(
            result.source.matches("story::StoryFlagBase::Mod").count(),
            if mode == "renamed" { 3 } else { 0 }
        );
        if mode != "renamed" {
            for value in [1214, 900, 911] {
                assert!(
                    result
                        .source
                        .contains(&format!("({value} + memory::i32[0x0810])"))
                );
            }
        }
        assert_eq!(
            scenario::compile("dog-map-banks", &result.source, &selected).unwrap(),
            bytes
        );
    }
}

#[test]
fn nested_flag_family_offsets_preserve_structure_and_field_banks() {
    let definitions = definitions();
    let text = source(
        "native::set_event_bit(((s32(150) + (memory::i32[0x0810] * 3)) + 1));
        native::clear_event_bit((2 + ((memory::i32[0x0810] * 3) + u16(150))));
        eval native::test_event_bit(((150 + (memory::i32[0x0810] * 3)) + (1 + 1)));
        native::set_event_bit((((100 + 50) + (memory::i32[0x0810] * 3)) + 2));
        native::set_event_bit((((150 + memory::i32[0x0810]) + 1) + 2));
        native::set_event_bit((1 + ((150 + memory::i32[0x0810]) + 2)));
        native::set_event_bit(((150 + memory::i32[0x0810]) - 1));
        native::set_event_bit(((150 + memory::i32[0x0810]) * 2));
        native::set_event_bit(((150 + memory::i32[0x0810]) + memory::i32[0x0814]));
        eval native::get_item_count(((150 + memory::i32[0x0810]) + 1));
        eval ((150 + memory::i32[0x0810]) + 1);
        native::set_event_bit(((623 + memory::i32[0x0810]) + 1));",
    );
    let bytes = scenario::compile("nested-flag-banks", &text, &definitions).unwrap();
    for context in [
        scenario::Context::FieldScene(193),
        scenario::Context::FieldScene(211),
        scenario::Context::FieldScene(362),
        scenario::Context::FieldScene(138),
        scenario::Context::Field,
        scenario::Context::World,
        scenario::Context::Battle,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let base = match context {
            scenario::Context::FieldScene(field @ (193 | 211 | 362)) => {
                format!("story::Field{field:04}FlagBase::PartyAssignmentBank")
            }
            _ => "story::StoryFlagBase::WorldResetSceneBank150".into(),
        };
        assert_eq!(result.source.matches(&base).count(), 6, "{}", result.source);
        for expected in [
            format!("((s32({base}) + (memory::i32[0x0810] * 3)) + 1)"),
            format!("(2 + ((memory::i32[0x0810] * 3) + u16({base})))"),
            format!("(({base} + (memory::i32[0x0810] * 3)) + (1 + 1))"),
            format!("((encoded({base}, (100 + 50)) + (memory::i32[0x0810] * 3)) + 2)"),
            format!("((({base} + memory::i32[0x0810]) + 1) + 2)"),
            format!("(1 + (({base} + memory::i32[0x0810]) + 2))"),
        ] {
            assert!(
                result.source.contains(&expected),
                "{expected}\n{}",
                result.source
            );
        }
        for raw in [
            "((150 + memory::i32[0x0810]) - 1)",
            "((150 + memory::i32[0x0810]) * 2)",
            "((150 + memory::i32[0x0810]) + memory::i32[0x0814])",
            "get_item_count(item_id: ((150 + memory::i32[0x0810]) + 1))",
            "eval ((150 + memory::i32[0x0810]) + 1);",
        ] {
            assert!(result.source.contains(raw), "{raw}\n{}", result.source);
        }
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.native.starts_with("indexed::") && id.value == 150)
                .count(),
            6
        );
        assert!(result.source.contains(
            "story::StoryFlagBase::TempleOfDarknessGatheredShadows + memory::i32[0x0810]) + 1"
        ));
        let formatted =
            symphonia_script_compiler::format("nested-flag-banks", &result.source).unwrap();
        assert_eq!(
            scenario::compile("nested-flag-banks", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn nested_flag_banks_follow_selected_members_fallback_and_outer_precedence() {
    let bytes = scenario::compile(
        "nested-flag-banks",
        &source("native::set_event_bit(((150 + memory::i32[0x0810]) + 1));"),
        &definitions(),
    )
    .unwrap();
    for mode in ["renamed", "moved", "missing", "missing_all", "outer"] {
        let mut modules = resonance_script_content::modules();
        let story = modules.get_mut("std::story").unwrap();
        let start = story.find("pub enum Field0211FlagBase ").unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        let block = story[start..end].to_owned();
        story.replace_range(
            start..end,
            &match mode {
                "missing" | "missing_all" | "outer" => String::new(),
                "moved" => block.replace("PartyAssignmentBank = 150", "CustomBank = 151"),
                _ => block.replace("PartyAssignmentBank = 150", "CustomBank = 150"),
            },
        );
        if mode == "missing_all" {
            let start = story.find("pub enum StoryFlagBase ").unwrap();
            let end = start + story[start..].find('}').unwrap() + 1;
            story.replace_range(start..end, "");
        }
        if mode == "outer" {
            story.push_str("\npub enum Field0211FlagBase { CustomBank = 150, OuterBase = 1 }\n");
        }
        let selected = Definitions::from_sources(&modules).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &selected, scenario::Context::FieldScene(211))
                .unwrap();
        let expected = match mode {
            "renamed" => "((story::Field0211FlagBase::CustomBank + memory::i32[0x0810]) + 1)",
            "outer" => "((150 + memory::i32[0x0810]) + story::Field0211FlagBase::OuterBase)",
            "missing_all" => "((150 + memory::i32[0x0810]) + 1)",
            _ => "((story::StoryFlagBase::WorldResetSceneBank150 + memory::i32[0x0810]) + 1)",
        };
        assert!(
            result.source.contains(expected),
            "{expected}\n{}",
            result.source
        );
        assert_eq!(
            scenario::compile("nested-flag-banks", &result.source, &selected).unwrap(),
            bytes
        );
    }
}

#[test]
fn scoped_indexed_flag_families_keep_field_context_and_arithmetic() {
    let definitions = definitions();
    let text = source(
        "native::set_event_bit((s32(164) + memory::i32[0x0810]));
        eval native::test_event_bit((memory::i32[0x0810] + u16(164)));
        native::clear_event_bit(((160 + 4) + memory::i32[0x0810]));
        native::set_event_bit((164 - memory::i32[0x0810]));
        native::set_event_bit((164 * memory::i32[0x0810]));
        native::set_event_bit(164); native::set_event_bit((164 + 1));
        eval native::get_item_count((164 + memory::i32[0x0810]));
        eval (164 + memory::i32[0x0810]);
        native::set_event_bit((623 + memory::i32[0x0810]));",
    );
    let bytes = scenario::compile("scoped-indexed-flags", &text, &definitions).unwrap();
    for context in [
        scenario::Context::FieldScene(130),
        scenario::Context::FieldScene(135),
        scenario::Context::FieldScene(138),
        scenario::Context::Field,
        scenario::Context::World,
        scenario::Context::Battle,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let field = match context {
            scenario::Context::FieldScene(130) => Some(130),
            scenario::Context::FieldScene(135) => Some(135),
            _ => None,
        };
        let uses: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.domain.ends_with("FlagBase") && id.value == 164)
            .collect();
        assert_eq!(uses.len(), if field.is_some() { 3 } else { 0 });
        if let Some(field) = field {
            let base = format!("story::Field{field:04}FlagBase::BlockTransferPending");
            for expected in [
                format!("s32({base}) + memory::i32[0x0810]"),
                format!("memory::i32[0x0810] + u16({base})"),
                format!("encoded({base}, (160 + 4)) + memory::i32[0x0810]"),
            ] {
                assert!(
                    result.source.contains(&expected),
                    "{expected}\n{}",
                    result.source
                );
            }
            assert!(
                uses.iter()
                    .all(|id| id.symbolic && id.native.starts_with("indexed::"))
            );
        } else {
            assert!(!result.source.contains("BlockTransferPending"));
        }
        for raw in [
            "(164 - memory::i32[0x0810])",
            "(164 * memory::i32[0x0810])",
            "get_item_count(item_id: (164 + memory::i32[0x0810]))",
            "eval (164 + memory::i32[0x0810]);",
        ] {
            assert!(result.source.contains(raw), "{raw}\n{}", result.source);
        }
        assert!(
            result
                .source
                .contains("story::StoryFlagBase::TempleOfDarknessGatheredShadows")
        );
        let formatted =
            symphonia_script_compiler::format("scoped-indexed-flags", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scoped-indexed-flags", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn scoped_indexed_flag_bases_use_optional_selected_and_mod_definitions() {
    let bytes = scenario::compile(
        "scoped-indexed-flags",
        &source("native::set_event_bit((164 + memory::i32[0x0810]));"),
        &definitions(),
    )
    .unwrap();
    for mode in ["renamed", "moved", "missing", "mod"] {
        let mut modules = resonance_script_content::modules();
        let story = modules.get_mut("std::story").unwrap();
        let start = story.find("pub enum Field0130FlagBase ").unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        let block = story[start..end].to_owned();
        story.replace_range(
            start..end,
            &match mode {
                "missing" | "mod" => String::new(),
                "moved" => block.replace("BlockTransferPending = 164", "CustomTransfer = 165"),
                _ => block.replace("BlockTransferPending = 164", "CustomTransfer = 164"),
            },
        );
        story.push_str("\npub enum UnrelatedFlagBase { Decoy = 164 }\n");
        *story = story.replacen(
            "pub enum Field0130Flag {",
            "pub enum Field0130Flag { IndividualDecoy = 164,",
            1,
        );
        if mode == "mod" {
            story.push_str("\npub enum Field0999FlagBase { ModTransfer = 164 }\n");
        }
        let selected = Definitions::from_sources(&modules).unwrap();
        for field in [130, 999] {
            let result = scenario::decompile_with_context(
                &bytes,
                &selected,
                scenario::Context::FieldScene(field),
            )
            .unwrap();
            let expected = if field == 130 && mode == "renamed" {
                Some("story::Field0130FlagBase::CustomTransfer")
            } else if field == 999 && mode == "mod" {
                Some("story::Field0999FlagBase::ModTransfer")
            } else {
                None
            };
            if let Some(expected) = expected {
                assert!(result.source.contains(expected), "{}", result.source);
            } else {
                assert!(
                    result.source.contains("(164 + memory::i32[0x0810])"),
                    "{}",
                    result.source
                );
            }
            assert!(!result.source.contains("Decoy"));
            assert_eq!(
                scenario::compile("scoped-indexed-flags", &result.source, &selected).unwrap(),
                bytes
            );
        }
    }
}

#[test]
fn indexed_flag_bases_follow_the_supplied_definitions() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "indexed-flags",
        &source("native::set_event_bit((623 + memory::i32[0x0810]));"),
        &definitions,
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum StoryFlagBase").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "");
    let without_base = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &without_base).unwrap();
    assert!(result.source.contains("(623 + memory::i32[0x0810])"));
    assert!(!result.source.contains("StoryFlag::"));
    assert!(!result.source.contains("StoryFlagBase::"));
    assert_eq!(
        scenario::compile("indexed-flags", &result.source, &without_base).unwrap(),
        bytes
    );

    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum StoryFlagBase { ModShadowGroup = 623 }\n");
    let renamed_base = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &renamed_base).unwrap();
    assert!(
        result
            .source
            .contains("story::StoryFlagBase::ModShadowGroup")
    );
    assert_eq!(
        scenario::compile("indexed-flags", &result.source, &renamed_base).unwrap(),
        bytes
    );
}

#[test]
fn bound_before_is_one_constant_push_with_checked_signed_arithmetic() {
    let definitions = definitions();
    let text = source(
        "eval bound_before(1000);
        eval s32(bound_before(1000));
        eval s8(bound_before(-127));
        eval bound_before(0);
        eval bound_before(-2147483647);
        eval bound_before(2147483647);
        eval encoded(bound_before(1000), (500 + 499));",
    );
    let reference = source(
        "eval 999;
        eval s32(999);
        eval s8(-128);
        eval -1;
        eval -2147483648;
        eval 2147483646;
        eval (500 + 499);",
    );
    let bytes = scenario::compile("bounds", &text, &definitions).unwrap();
    assert_eq!(
        bytes,
        scenario::compile("reference", &reference, &definitions).unwrap()
    );
    let formatted = symphonia_script_compiler::format("bounds", &text).unwrap();
    assert_eq!(
        scenario::compile("bounds", &formatted, &definitions).unwrap(),
        bytes
    );
    for invalid in [
        "bound_before(-2147483648)",
        "bound_before(2147483648)",
        "bound_before(-2147483649)",
        "bound_before(native::get_item_count(1))",
        "bound_before(memory::i32[0])",
        "bound_before(1 + 1)",
        "bound_before(1, 2)",
        "bound_before()",
    ] {
        assert!(
            scenario::compile(
                "invalid-bound",
                &source(&format!("eval {invalid};")),
                &definitions
            )
            .is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn bound_before_names_only_stage_upper_bounds_and_preserves_snapshot_reads() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::Story] <= s32(12802999));
        eval (12802999 >= globals[story::GlobalId::Story]);
        eval (globals[story::GlobalId::Story] < 12802999);
        eval (globals[story::GlobalId::Story] == 12802999);
        eval (globals[story::GlobalId::Story] = 12802999);
        eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        goto check; check:
        eval (memory::i32[0x0810] <= 12802999);
        eval (memory::i32[0x0810] <= (12802000 + 999));
        eval (memory::i32[0x0814] <= 12802999);
        eval (globals[story::GlobalId::ColetteAngelProgress] <= 12802999);
        eval (globals[story::GlobalId::ColetteAngelProgress] <= 300);",
    );
    let bytes = scenario::compile("bound-comparisons", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "Story] <= bound_before(story::StoryStage::RaineSearchWithMithosAgreed)",
        "bound_before(story::StoryStage::RaineSearchWithMithosAgreed) >= globals[story::GlobalId::Story]",
        "Story] < 12802999",
        "Story] == 12802999",
        "Story] = 12802999",
        "memory::i32[0x0810] <= bound_before(story::StoryStage::RaineSearchWithMithosAgreed)",
        "encoded(bound_before(story::StoryStage::RaineSearchWithMithosAgreed), (12802000 + 999))",
        "memory::i32[0x0814] <= 12802999",
        "ColetteAngelProgress] <= 12802999",
        "ColetteAngelProgress] <= story::ColetteAngelStage::VoicelessEligibilityLowerBound",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32" && id.value == 12802999 && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("bound-comparisons", &result.source).unwrap();
    assert_eq!(
        scenario::compile("bound-comparisons", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn bound_before_anchors_use_actual_scene_and_modded_definitions() {
    let text = source(
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        goto check; check: eval (memory::i32[0x0810] <= s32(99));",
    );
    let bytes = scenario::compile("scoped-bound", &text, &definitions()).unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story.replace("DarkBottleTestPreset = 100", "ModBottleStage = 100");
    let definitions = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(351))
            .unwrap();
    assert!(
        result
            .source
            .contains("s32(bound_before(story::Field0351StoryStage::ModBottleStage))"),
        "{}",
        result.source
    );
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "story::Field0351StoryStage" && id.value == 99 && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("scoped-bound", &result.source).unwrap();
    assert_eq!(
        scenario::compile("scoped-bound", &formatted, &definitions).unwrap(),
        bytes
    );
    let unscoped = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(!unscoped.source.contains("bound_before"));
    let story = modules.get_mut("std::story").unwrap();
    *story = story.replace("ModBottleStage = 100", "ModBottleStage = 101");
    let changed = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &changed, scenario::Context::FieldScene(351))
            .unwrap();
    assert!(!result.source.contains("bound_before"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.value == 99 && !id.symbolic)
    );
}

#[test]
fn bound_before_does_not_infer_unregistered_character_ids_from_the_next_id() {
    let text = source("eval (globals[story::GlobalId::FlanoirCompanion] <= 2);");
    let bytes = scenario::compile("character-bound", &text, &definitions()).unwrap();
    let mut modules = resonance_script_content::modules();
    *modules.get_mut("std::characters").unwrap() =
        "script library; pub enum CharacterId { ModCompanion = 3 }".into();
    let definitions = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(result.source.contains("FlanoirCompanion] <= 2"));
    assert!(!result.source.contains("bound_before"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "characters::CharacterId" && id.value == 2 && !id.symbolic)
    );
}

#[test]
fn global_snapshots_cross_jumps_without_rewriting_reads_or_typing_reused_slots() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        eval (memory::i32[0x0814] = memory::i32[0x0810]);
        goto check;
        check:
        eval (memory::i32[0x0814] >= s32(1304000));
        eval (1305000 == memory::i32[0x0814]);
        eval (memory::i32[0x0814] < (1303999 + 1));
        eval (memory::i32[0x0814] = 1305000);
        eval (memory::i32[0x0814] == 1305000);",
    );
    let bytes = scenario::compile("snapshot", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "memory::i32[0x0810] = globals[story::GlobalId::Story]",
        "memory::i32[0x0814] = memory::i32[0x0810]",
        "memory::i32[0x0814] >= story::StoryStage::FirstAngelNightCampPending",
        "story::StoryStage::FirstAngelNightCampDepartureReady == memory::i32[0x0814]",
        "encoded(story::StoryStage::FirstAngelNightCampPending, (1303999 + 1))",
        "memory::i32[0x0814] = 1305000",
        "memory::i32[0x0814] == 1305000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32"
                && id.parameter == "0x814 from 0x40/0x23"
                && id.value == 1304000
                && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("snapshot", &result.source).unwrap();
    assert_eq!(
        scenario::compile("snapshot", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn global_snapshots_require_agreeing_paths_and_a_stable_loop_fixed_point() {
    let definitions = definitions();
    let agreeing = source(
        "if (memory::i32[0x0900]) {
            eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        } else {
            eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        }
        while (memory::i32[0x0900]) { eval (memory::i32[0x0900] -= 1); }
        eval (memory::i32[0x0810] == 1304000);",
    );
    let bytes = scenario::compile("agreeing", &agreeing, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        result
            .source
            .contains("memory::i32[0x0810] == story::StoryStage::FirstAngelNightCampPending")
    );
    for body in [
        "if (memory::i32[0x0900]) { eval (memory::i32[0x0810] = globals[story::GlobalId::Story]); }
         else { eval (memory::i32[0x0810] = globals[story::GlobalId::WaterSealProgress]); }",
        "if (memory::i32[0x0900]) { eval (memory::i32[0x0810] = globals[story::GlobalId::Story]); }",
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
         while (memory::i32[0x0900]) { eval (memory::i32[0x0810] = 0); }",
    ] {
        let text = source(&format!("{body} eval (memory::i32[0x0810] == 1304000);"));
        let bytes = scenario::compile("conflicting", &text, &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.native == "memory::snapshot32"),
            "{}",
            result.source
        );
        assert!(result.source.contains("memory::i32[0x0810] == 1304000"));
    }
}

#[test]
fn global_snapshots_stop_at_aliasing_writes_calls_and_changed_values() {
    let definitions = definitions();
    for barrier in [
        "native::discard_value(0);",
        "call barrier;",
        "eval (memory::i8[0x0811] = 0);",
        "eval (memory::i16[0x080f] = 0);",
        "eval (memory::i32[0x0900][memory::i32[0x0a00]] = 0);",
        "eval ++(memory::i32[0x0810]);",
        "eval ((memory::i32[0x0810] + 1) = 0);",
        "eval (memory::i32[0x0810] = (memory::i32[0x0810] + 1));",
    ] {
        let text = source(&format!(
            "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
             {barrier} eval (memory::i32[0x0810] == 1304000);"
        ))
        .replace("auxiliary:", "task barrier() { return; }\nauxiliary:");
        let bytes = scenario::compile("barrier", &text, &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.native == "memory::snapshot32"),
            "crossed {barrier}: {}",
            result.source
        );
        assert!(result.source.contains("memory::i32[0x0810] == 1304000"));
    }
}

#[test]
fn global_snapshots_keep_captured_domains_after_source_and_unrelated_writes() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        eval (globals[story::GlobalId::Story] = 1305000);
        eval (memory::i8[0x0814] = 0);
        eval (memory::i32[0x0810] == 1304000);
        eval (memory::i32[0x0818] = globals[story::GlobalId::ColetteAngelProgress]);
        eval (memory::i32[0x0818] == 301);
        eval (memory::i32[0x0820] = globals[story::GlobalId::WindSealProgress]);
        eval (memory::i32[0x0820] == s32(21000));
        eval (memory::i32[0x0824] = globals[story::GlobalId::WaterSealProgress]);
        eval (memory::i32[0x0824] == 21000);",
    );
    let bytes = scenario::compile("captured", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "memory::i32[0x0810] == story::StoryStage::FirstAngelNightCampPending",
        "memory::i32[0x0818] == story::ColetteAngelStage::FourthSealNightComplete",
        "memory::i32[0x0820] == s32(story::WindSealStage::SealReleaseConversationComplete)",
        "memory::i32[0x0824] == story::WaterSealStage::SealReleaseConversationComplete",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.native == "memory::snapshot32")
            .count(),
        4
    );
}

#[test]
fn global_snapshots_do_not_leak_into_external_event_entries_or_callees() {
    let definitions = definitions();
    let text = "script scenario; use std::story; code_base = 10;
        data { word(10); pc(entry); word(0); word(1); dword(1); dword(2000); pc32(event); }
        task entry() { eval (memory::i32[0x0810] = globals[story::GlobalId::Story]); goto event; }
        task event() { eval (memory::i32[0x0810] == 1304000); end; }";
    let bytes = scenario::compile("event-entry", text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32")
    );
    let text = source(
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]); call barrier;
        eval (memory::i32[0x0810] == 1304000);",
    )
    .replace(
        "auxiliary:",
        "task barrier() { eval (memory::i32[0x0810] == 1304000); return; }\nauxiliary:",
    );
    let bytes = scenario::compile("callee", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32")
    );
}

#[test]
fn global_snapshots_use_scene_aliases_and_supplied_standard_names() {
    let text = source(
        "eval (memory::i32[0x0810] = globals[story::GlobalId::Story]);
        goto check; check: eval (memory::i32[0x0810] == 100);",
    );
    let bytes = scenario::compile("scoped-copy", &text, &definitions()).unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story.replace("DarkBottleTestPreset = 100", "ModBottleStage = 100");
    let definitions = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(351))
            .unwrap();
    assert!(
        result
            .source
            .contains("memory::i32[0x0810] == story::Field0351StoryStage::ModBottleStage")
    );
    assert!(
        result.identities.iter().any(
            |id| id.native == "memory::snapshot32" && id.domain == "story::Field0351StoryStage"
        )
    );
    let unscoped = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(unscoped.source.contains("memory::i32[0x0810] == 100"));
    assert_eq!(
        scenario::compile("scoped-copy", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn copied_item_and_flag_literals_gain_names_without_folding_memory_reads() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0808] = u16(115));
        eval (memory::i32[0x080c] = memory::i32[0x0808]);
        eval native::get_item_count(memory::i32[0x080c]);
        eval (memory::i32[0x0810] = s16(104));
        native::set_event_bit(memory::i32[0x0810]);",
    );
    let bytes = scenario::compile("copies", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "memory::i32[0x0808] = u16(items::ItemId::Kirima)",
        "memory::i32[0x080C] = memory::i32[0x0808]",
        "get_item_count(item_id: memory::i32[0x080C])",
        "memory::i32[0x0810] = s16(story::StoryFlag::ManualMovementLearned)",
        "set_event_bit(bit: memory::i32[0x0810])",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.native == "memory::copy" && id.symbolic)
            .count(),
        2
    );
    assert_eq!(
        scenario::compile("copies", &result.source, &definitions).unwrap(),
        bytes
    );
    let formatted = symphonia_script_compiler::format("copies", &result.source).unwrap();
    assert_eq!(
        scenario::compile("copies", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn calculated_ids_copied_to_memory_retain_their_calculator_encoding() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0808] = (110 + 5));
        eval native::get_item_count(memory::i32[0x0808]);
        eval (memory::i32[0x0810] = -(s16(-104)));
        native::set_event_bit(memory::i32[0x0810]);",
    );
    let bytes = scenario::compile("computed-copy", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        result
            .source
            .contains("encoded(items::ItemId::Kirima, (110 + 5))"),
        "{}",
        result.source
    );
    assert!(
        result
            .source
            .contains("encoded(story::StoryFlag::ManualMovementLearned, -(s16(-104)))"),
        "{}",
        result.source
    );
    assert_eq!(
        scenario::compile("computed-copy", &result.source, &definitions).unwrap(),
        bytes
    );
    let formatted = symphonia_script_compiler::format("computed-copy", &result.source).unwrap();
    assert_eq!(
        scenario::compile("computed-copy", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn copied_identities_do_not_cross_a_script_call_or_fold_memory_based_arithmetic() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0808] = 115);
        call barrier;
        eval native::get_item_count(memory::i32[0x0808]);
        eval (memory::i32[0x0810] = (memory::i32[0x0808] + 1));
        eval native::get_item_count(memory::i32[0x0810]);",
    )
    .replace("auxiliary:", "task barrier() { return; }\nauxiliary:");
    let bytes = scenario::compile("call-barrier", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(!result.source.contains("items::ItemId::"));
    assert_eq!(
        scenario::compile("call-barrier", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn copied_identities_stop_at_aliasing_writes_calls_and_control_flow() {
    let definitions = definitions();
    for barrier in [
        "native::discard_value(0);",
        "if (memory::i32[0x0900]) { eval 1; }",
        "eval (memory::i8[0x0809] = 7);",
        "eval (memory::i32[0x0900][memory::i32[0x0a00]] = 0);",
        "eval ++(memory::i32[0x0808]);",
        "eval (memory::i32[0x0808] = 116);",
    ] {
        let text = source(&format!(
            "eval (memory::i32[0x0808] = 115); {barrier}
            eval native::get_item_count(memory::i32[0x0808]);"
        ));
        let bytes = scenario::compile("barrier", &text, &definitions).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(
            !result.source.contains("ItemId::Kirima"),
            "crossed {barrier}: {}",
            result.source
        );
        if barrier.contains("= 116") {
            assert!(result.source.contains("ItemId::Amango"));
        }
        assert_eq!(
            scenario::compile("barrier", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn one_copied_literal_used_in_different_identity_domains_stays_numeric() {
    let definitions = definitions();
    let text = source("eval (memory::i32[0x0808] = 104);
        eval native::change_item_count(memory::i32[0x0808], native::test_event_bit(memory::i32[0x0808]));");
    let bytes = scenario::compile("conflicting", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(!result.source.contains("StoryFlag::ManualMovementLearned"));
    assert!(!result.source.contains("items::ItemId::"));
    assert_eq!(
        scenario::compile("conflicting", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn copied_identities_require_the_selected_standard_definitions() {
    let bytes = scenario::compile(
        "missing",
        &source(
            "eval (memory::i32[0x0808] = 115);
        eval native::get_item_count(memory::i32[0x0808]);",
        ),
        &Definitions::default(),
    )
    .unwrap();
    let error = scenario::decompile(&bytes, &Definitions::default())
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::items::ItemId definitions"),
        "{error}"
    );
}

#[test]
fn formation_slot_encodings_and_conditional_native_results_keep_their_domains() {
    let definitions = definitions();
    let text = source(
        "if ((native::find_party_member((-65536 | 1)) == 2)) { eval 1; }
        if ((native::get_title(2, 0) == 257)) { eval 1; }
        if ((native::field_system(13, 0) == 9)) { eval 1; }",
    );
    let bytes = scenario::compile("slots", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for name in [
        "encoded(scenario::FormationSlotSelector::Slot1, (-65536 | 1))",
        "characters::CharacterId::Colette",
        "characters::TitleId::ColetteFledglingChosen",
        "characters::CharacterId::Kratos",
    ] {
        assert!(
            result.source.contains(name),
            "missing {name}: {}",
            result.source
        );
    }
    assert_eq!(
        scenario::compile("slots", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn calculated_identities_use_verified_symbols_without_replacing_their_encoding() {
    let definitions = definitions();
    let text = source(
        "native::set_event_bit((100 + 4));
        native::configure_dialogue(0, ((16 | 2) | 32), -1, (1 + 1), 0, 0, 0, 0);
        eval encoded(-1, (s32(-2147483648) / -1));
        eval encoded(0, (5 << 32));
        eval encoded(5, (5 << 64));
        eval encoded(0, (5 / 0));
        eval encoded(-1, (-5 / 0));",
    )
    .replace("use std::story;", "use std::story; use std::scenario;");
    let bytes = scenario::compile("calculated", &text, &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    assert!(
        result
            .source
            .contains("encoded(story::StoryFlag::ManualMovementLearned, (100 + 4))")
    );
    assert!(
        result
            .source
            .contains("encoded(characters::CharacterId::Colette, (1 + 1))")
    );
    for flag in ["Green", "Persistent", "AutoPages"] {
        assert!(
            result
                .source
                .contains(&format!("scenario::DialogueFlag::{flag}"))
        );
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.value == 104 && id.domain == "story::StoryFlag" && id.symbolic)
    );
    assert_eq!(
        scenario::compile("calculated", &result.source, &definitions).unwrap(),
        bytes
    );
    let formatted = symphonia_script_compiler::format("calculated", &result.source).unwrap();
    assert_eq!(
        scenario::compile("calculated", &formatted, &definitions).unwrap(),
        bytes
    );
    for invalid in [
        "eval encoded(2, (1 + 2));",
        "eval encoded(0, memory::i32[0]);",
        "eval encoded(0, native::get_item_count(1));",
        "eval encoded(0, (memory::i32[0] = 0));",
    ] {
        assert!(
            scenario::compile("invalid", &source(invalid), &definitions).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn ring_modes_colors_and_query_results_use_distinct_operand_domains() {
    let definitions = definitions();
    let text = source(
        "eval native::configure_sorcerers_ring(13, 2);
        eval native::configure_sorcerers_ring(18, 2);
        eval native::configure_sorcerers_ring(19, 1);
        if ((native::configure_sorcerers_ring(-1, 0) == 14)) { eval 1; }",
    );
    let bytes = scenario::compile("ring", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for symbol in [
        "SorcerersRingMode::Lightning",
        "RingLightningColor::Red",
        "RingCallColor::Blue",
        "RingBubblePhase::Float",
        "SorcerersRingMode::Query",
        "SorcerersRingMode::Ice",
    ] {
        assert!(
            result.source.contains(symbol),
            "missing {symbol}: {}",
            result.source
        );
    }
    assert_eq!(
        scenario::compile("ring", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn retained_field_story_stages_override_only_declared_values_in_the_selected_scene() {
    use scenario::Context;
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::Story] = 1601000);
        if ((globals[story::GlobalId::Story] == (1600000 + 2000))) { eval 0; }
        if ((1603000 <= globals[story::GlobalId::Story])) { eval 0; }
        eval (globals[story::GlobalId::Story] = 903000);
        eval (globals[story::GlobalId::ColetteAngelProgress] = 1601000);
        eval (globals[story::GlobalId::Story] += 1601000);
        eval (memory::i32[0x0810] = 1601000);",
    );
    let bytes = scenario::compile("scoped-stages", &text, &definitions).unwrap();
    for field in [370, 371] {
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let domain = format!("story::Field{field:04}StoryStage");
        for name in [
            "DirkFoundAlive",
            "GenisKeyCrestRepairNightAgreed",
            "GenisKeyCrestFittedAndDepartureComplete",
        ] {
            assert!(
                result.source.contains(&format!("{domain}::{name}")),
                "{}",
                result.source
            );
        }
        assert!(result.source.contains(&format!(
            "encoded({domain}::GenisKeyCrestRepairNightAgreed, (1600000 + 2000))"
        )));
        assert!(result.source.contains("story::StoryStage::IseliaExile"));
        assert!(result.source.contains("ColetteAngelProgress] = 1601000"));
        assert!(result.source.contains("Story] += 1601000"));
        assert!(result.source.contains("memory::i32[0x0810] = 1601000"));
        assert!(!result.source.contains("PalmacostaFirstArrivalComplete"));
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.native == "memory::constant32"
                    && id.domain == domain
                    && id.value == 1601000
                    && id.symbolic)
        );
        let formatted = symphonia_script_compiler::format("scoped-stages", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scoped-stages", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [
        Context::Unspecified,
        Context::Field,
        Context::FieldScene(55),
        Context::World,
        Context::Battle,
        Context::Skit,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(
            result
                .source
                .contains("story::StoryStage::PalmacostaFirstArrivalComplete")
        );
        assert!(
            result
                .source
                .contains("story::StoryStage::PalmaPotionReplacementAgreed")
        );
        assert!(!result.source.contains("DirkFoundAlive"));
    }
}

#[test]
fn unused_global_stage_writes_keep_their_original_pc_and_scoped_name() {
    let definitions = definitions();
    let bytes = binary::assemble(
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nend\nload.s32 64\npush.s32 1601000\ncalc 0x10\ncalc 0\nret\n",
    ).unwrap();
    assert!(Program::decode(&bytes).unwrap().instruction(1).is_none());
    let (load, diagnostics) = binary::decode_at(&bytes, 1).unwrap();
    assert!(diagnostics.is_empty() && !load.raw);
    assert_eq!(load.mnemonic, "load.s32");
    assert_eq!(load.operands, [64]);
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(370))
            .unwrap();
    assert!(
        result
            .source
            .contains("story::Field0370StoryStage::DirkFoundAlive")
    );
    assert!(result.identities.iter().any(|id| id.pc == 1
        && id.native == "memory::constant32"
        && id.parameter == "0x40/0x10"
        && id.domain == "story::Field0370StoryStage"
        && id.value == 1601000
        && id.symbolic));
    assert_eq!(
        scenario::compile("unused-stage", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn scoped_story_stage_names_are_opted_in_by_the_supplied_standard_definitions() {
    use scenario::Context;
    let mut sources = resonance_script_content::modules();
    let source_text = source("eval (globals[story::GlobalId::Story] = 1601000);");
    let bytes = scenario::compile("stage-opt-in", &source_text, &definitions()).unwrap();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum Field0370StoryStage").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(
        start..end,
        "pub enum UnrelatedStage { DirkFoundAlive = 1601000 }",
    );
    for name in [
        "Field370StoryStage",
        "Field00370StoryStage",
        "Field0370Stage",
    ] {
        story.push_str(&format!("\npub enum {name} {{ Decoy = 1601000 }}\n"));
    }
    let selected = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &selected, Context::FieldScene(370)).unwrap();
    assert!(
        result
            .source
            .contains("story::StoryStage::PalmacostaFirstArrivalComplete")
    );
    assert!(!result.source.contains("DirkFoundAlive"));
    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum Field0370StoryStage { ModdedDirkName = 1601000 }\n");
    let selected = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &selected, Context::FieldScene(370)).unwrap();
    assert!(
        result
            .source
            .contains("story::Field0370StoryStage::ModdedDirkName")
    );
    assert!(!result.source.contains("PalmacostaFirstArrivalComplete"));
    assert_eq!(
        scenario::compile("stage-opt-in", &result.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn global_stage_audits_distinguish_literals_from_deltas_counters_and_narrow_memory() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x0040] = s32(903000));
        if ((memory::i32[0x0040] < 1234567)) { eval 0; }
        eval (memory::i32[0x004c] = 1000);
        eval (memory::i32[0x0040] += 1000);
        eval (memory::i32[0x0040] + 1000);
        eval (memory::i16[0x0040] = 1000);
        eval (memory::i32[0x00fc] = 1000);
        eval (memory::i32[0x00e8] = 1000);
        eval (memory::i32[0x043c] = 1000);",
    );
    let bytes = scenario::compile("global-audit", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    let globals: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.native == "memory::constant32")
        .map(|id| {
            (
                id.domain.as_str(),
                id.parameter.as_str(),
                id.value,
                id.symbolic,
            )
        })
        .collect();
    assert_eq!(
        globals,
        [
            ("story::StoryStage", "0x40/0x10", 903000, true),
            ("story::StoryStage", "0x40/0x25", 1234567, false),
            ("story::ColetteAngelStage", "0x4c/0x10", 1000, true),
            ("story::CelsiusPactStage", "0xe8/0x10", 1000, true),
        ]
    );
    assert_eq!(
        scenario::compile("global-audit", &result.source, &definitions).unwrap(),
        bytes
    );

    let bytes = binary::assemble(
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nload.s32 64\nliteral:\npush.s32 903000\ncalc 0x10\ncalc 0\nend\njump literal\n",
    ).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.native == "memory::constant32")
    );
}

#[test]
fn luin_rebuilding_stages_are_scoped_to_the_stage_word_and_keep_literal_encodings() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::LuinRebuilding] = s32(1000));
        if ((globals[story::GlobalId::LuinRebuilding] < (9000 + 1000))) { eval 0; }
        if ((600000 <= globals[story::GlobalId::LuinRebuilding])) { eval 0; }
        eval (globals[story::GlobalId::LuinRebuilding] = 40001);
        eval (globals[story::GlobalId::LuinRebuilding] = 1601000);
        eval (globals[story::GlobalId::LuinRebuilding] += 1000);
        eval (globals[story::GlobalId::LuinDonationProgress] = 1000);
        eval (globals[story::GlobalId::PalmacostaRanchAssaultRoute] = 1000);
        eval (globals[story::GlobalId::Story] = 1000);
        eval (memory::i16[0x00f8] = 1000);
        eval (memory::i32[0x0810] = 1000);",
    );
    let bytes = scenario::compile("luin-stage", &text, &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(370))
            .unwrap();
    for expected in [
        "LuinRebuilding] = s32(story::LuinRebuildingStage::Ruined)",
        "encoded(story::LuinRebuildingStage::EarlyReconstructionStarted, (9000 + 1000))",
        "story::LuinRebuildingStage::ExpandedCityStatuePhase <= globals[story::GlobalId::LuinRebuilding]",
        "LuinRebuilding] = 40001",
        "LuinRebuilding] = 1601000",
        "LuinRebuilding] += 1000",
        "LuinDonationProgress] = 1000",
        "PalmacostaRanchAssaultRoute] = story::PalmacostaRanchAssaultStage::AssaultChosen",
        "Story] = story::StoryStage::DayOfProphecyClassInterrupted",
        "memory::i16[0x00F8] = 1000",
        "memory::i32[0x0810] = 1000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    let identities: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.domain == "story::LuinRebuildingStage")
        .collect();
    assert_eq!(identities.len(), 3);
    assert!(
        identities
            .iter()
            .all(|id| id.native == "memory::constant32")
    );
    assert!(
        identities
            .iter()
            .any(|id| id.value == 1000 && id.parameter == "0xf8/0x10" && id.symbolic)
    );
    assert!(
        identities
            .iter()
            .any(|id| id.value == 40001 && !id.symbolic)
    );
    assert!(
        identities
            .iter()
            .any(|id| id.value == 1601000 && !id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("luin-stage", &result.source).unwrap();
    assert_eq!(
        scenario::compile("luin-stage", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn luin_stage_names_come_from_supplied_definitions_and_require_the_correct_domain() {
    let bytes = scenario::compile(
        "luin-stage",
        &source("eval (globals[story::GlobalId::LuinRebuilding] = 1000);"),
        &definitions(),
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    assert!(story.contains("Ruined = 1000"));
    *story = story.replace("Ruined = 1000", "RuinedForMod = 1000");
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &renamed).unwrap();
    assert!(result.source.contains("LuinRebuildingStage::RuinedForMod"));
    assert_eq!(
        scenario::compile("luin-stage", &result.source, &renamed).unwrap(),
        bytes
    );

    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum LuinRebuildingStage").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "");
    assert!(story.contains("pub enum BookOfRegenerationStage"));
    let partial = Definitions::from_sources(&sources).unwrap();
    let error = scenario::decompile(&bytes, &partial)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::story::LuinRebuildingStage definitions"),
        "{error}"
    );
}

#[test]
fn book_linkite_and_luin_stages_keep_equal_values_in_their_own_quest_words() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::BookOfRegeneration] = 40000);
        eval (globals[story::GlobalId::LinkiteTree] = 40000);
        eval (globals[story::GlobalId::LuinRebuilding] = 40000);
        if ((globals[story::GlobalId::BookOfRegeneration] < 50001)) { eval 0; }
        if (((9000 + 1000) == globals[story::GlobalId::BookOfRegeneration])) { eval 0; }
        eval (globals[story::GlobalId::LinkiteTree] = s32(30000));
        eval (globals[story::GlobalId::LinkiteTree] = 1500);
        eval (globals[story::GlobalId::LinkiteTree] = (globals[story::GlobalId::LinkiteTree] + 30000));
        eval (globals[story::GlobalId::BookOfRegeneration] += 1000);
        eval (globals[story::GlobalId::LuinDonationProgress] = 40000);",
    );
    let bytes = scenario::compile("quest-stages", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "BookOfRegeneration] = story::BookOfRegenerationStage::ThodaStatueRetrievalPhase",
        "LinkiteTree] = story::LinkiteTreeStage::LinkiteOcarinaReceived",
        "LuinRebuilding] = story::LuinRebuildingStage::WorldModelRestoredBoundary",
        "story::BookOfRegenerationStage::ConsultationTimerUpperBound",
        "encoded(story::BookOfRegenerationStage::HakonesiaImposterPursuitAgreed, (9000 + 1000))",
        "LinkiteTree] = s32(story::LinkiteTreeStage::FluteConstructionPlanAgreed)",
        "LinkiteTree] = 1500",
        "LinkiteTree] = (globals[story::GlobalId::LinkiteTree] + 30000)",
        "BookOfRegeneration] += 1000",
        "LuinDonationProgress] = 40000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (address, domain) in [
        ("0xe4", "story::BookOfRegenerationStage"),
        ("0x130", "story::LinkiteTreeStage"),
        ("0xf8", "story::LuinRebuildingStage"),
    ] {
        assert!(result.identities.iter().any(|id| id.domain == domain
            && id.value == 40000
            && id.parameter == format!("{address}/0x10")
            && id.symbolic));
    }
    let formatted = symphonia_script_compiler::format("quest-stages", &result.source).unwrap();
    assert_eq!(
        scenario::compile("quest-stages", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn book_and_linkite_require_their_supplied_stage_catalogues_without_equal_value_fallbacks() {
    for (global, domain, name) in [
        (
            "BookOfRegeneration",
            "BookOfRegenerationStage",
            "ThodaStatueRetrievalPhase",
        ),
        ("LinkiteTree", "LinkiteTreeStage", "LinkiteOcarinaReceived"),
    ] {
        let bytes = scenario::compile(
            "quest-stages",
            &source(&format!(
                "eval (globals[story::GlobalId::{global}] = 40000);"
            )),
            &definitions(),
        )
        .unwrap();
        let mut sources = resonance_script_content::modules();
        let story = sources.get_mut("std::story").unwrap();
        let declaration = format!("{name} = 40000");
        assert!(story.contains(&declaration));
        *story = story.replace(&declaration, &format!("{name}ForMod = 40000"));
        let renamed = Definitions::from_sources(&sources).unwrap();
        let result = scenario::decompile(&bytes, &renamed).unwrap();
        assert!(result.source.contains(&format!("{domain}::{name}ForMod")));
        assert_eq!(
            scenario::compile("quest-stages", &result.source, &renamed).unwrap(),
            bytes
        );
        let story = sources.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {domain}")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        assert!(story.contains("pub enum LuinRebuildingStage"));
        let partial = Definitions::from_sources(&sources).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::story::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn ranch_stage_domains_distinguish_equal_values_and_preserve_arithmetic_and_widths() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::PalmacostaRanchAssaultRoute] = s32(6000));
        eval (globals[story::GlobalId::PalmacostaRanchDorrRoute] = 6000);
        eval (globals[story::GlobalId::AsgardRanch] = 6000);
        eval (globals[story::GlobalId::AsgardRanch] = (3000 + 100));
        if ((11000 == globals[story::GlobalId::AsgardRanch])) { eval 0; }
        eval (globals[story::GlobalId::AsgardRanch] = 10000);
        eval (globals[story::GlobalId::PalmacostaRanchDorrRoute] = 1100);
        eval (globals[story::GlobalId::PalmacostaRanchAssaultRoute] += 6000);
        eval (globals[story::GlobalId::AsgardRanch] = (globals[story::GlobalId::AsgardRanch] + 3000));
        eval (memory::i16[0x00e0] = 1000);",
    );
    let bytes = scenario::compile("ranch-stages", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "PalmacostaRanchAssaultRoute] = s32(story::PalmacostaRanchAssaultStage::MagniusConfrontationStarted)",
        "PalmacostaRanchDorrRoute] = story::PalmacostaRanchDorrStage::NeilCardKeyInfiltrationAgreed",
        "AsgardRanch] = story::AsgardRanchStage::AsgardRestDepartureAgreed",
        "encoded(story::AsgardRanchStage::SupportTeamArrivalPending, (3000 + 100))",
        "story::AsgardRanchStage::ColetteHumanityDiscussionComplete == globals[story::GlobalId::AsgardRanch]",
        "AsgardRanch] = 10000",
        "PalmacostaRanchDorrRoute] = 1100",
        "PalmacostaRanchAssaultRoute] += 6000",
        "AsgardRanch] = (globals[story::GlobalId::AsgardRanch] + 3000)",
        "memory::i16[0x00E0] = 1000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (address, domain) in [
        ("0xb8", "story::PalmacostaRanchAssaultStage"),
        ("0xbc", "story::PalmacostaRanchDorrStage"),
        ("0xe0", "story::AsgardRanchStage"),
    ] {
        assert!(result.identities.iter().any(|id| id.domain == domain
            && id.value == 6000
            && id.parameter == format!("{address}/0x10")
            && id.symbolic));
    }
    let formatted = symphonia_script_compiler::format("ranch-stages", &result.source).unwrap();
    assert_eq!(
        scenario::compile("ranch-stages", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn ranch_stage_catalogues_require_the_correct_type_and_use_modded_variant_names() {
    for (global, domain, name) in [
        (
            "PalmacostaRanchAssaultRoute",
            "PalmacostaRanchAssaultStage",
            "MagniusConfrontationStarted",
        ),
        (
            "PalmacostaRanchDorrRoute",
            "PalmacostaRanchDorrStage",
            "NeilCardKeyInfiltrationAgreed",
        ),
        (
            "AsgardRanch",
            "AsgardRanchStage",
            "AsgardRestDepartureAgreed",
        ),
    ] {
        let bytes = scenario::compile(
            "ranch-stages",
            &source(&format!(
                "eval (globals[story::GlobalId::{global}] = 6000);"
            )),
            &definitions(),
        )
        .unwrap();
        let mut sources = resonance_script_content::modules();
        let story = sources.get_mut("std::story").unwrap();
        let declaration = format!("{name} = 6000");
        assert!(story.contains(&declaration));
        *story = story.replace(&declaration, &format!("{name}ForMod = 6000"));
        let renamed = Definitions::from_sources(&sources).unwrap();
        let result = scenario::decompile(&bytes, &renamed).unwrap();
        assert!(result.source.contains(&format!("{domain}::{name}ForMod")));
        assert_eq!(
            scenario::compile("ranch-stages", &result.source, &renamed).unwrap(),
            bytes
        );
        let story = sources.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {domain}")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        let partial = Definitions::from_sources(&sources).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::story::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn seal_progress_domains_keep_guardian_blessing_and_pact_markers_separate() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::FireSealProgress] = s32(21000));
        eval (globals[story::GlobalId::WaterSealProgress] = 21000);
        eval (globals[story::GlobalId::FireSealProgress] = 50000);
        eval (globals[story::GlobalId::WaterSealProgress] = 50000);
        eval (globals[story::GlobalId::WindSealProgress] = 21000);
        eval (globals[story::GlobalId::LightSealProgress] = 21000);
        eval (globals[story::GlobalId::WindSealProgress] = 50000);
        eval (globals[story::GlobalId::WindSealProgress] = (7000 + 500));
        eval (globals[story::GlobalId::WindSealProgress] == 2500);
        eval (globals[story::GlobalId::WindSealProgress] < 8000);
        eval (globals[story::GlobalId::WindSealProgress] == 999999);
        eval (globals[story::GlobalId::WindSealProgress] += 1000);
        eval (globals[story::GlobalId::FireSealProgress] = 7500);
        eval (globals[story::GlobalId::WaterSealProgress] = (30000 + 1000));
        eval (globals[story::GlobalId::FireSealProgress] = 1000);
        eval (globals[story::GlobalId::WaterSealProgress] = 30001);
        eval (globals[story::GlobalId::FireSealProgress] += 50000);
        eval (globals[story::GlobalId::WaterSealProgress] = (globals[story::GlobalId::WaterSealProgress] + 31000));
        eval (memory::i16[0x00c4] = 21000);
        eval (memory::i16[0x00c8] = 21000);
        eval (memory::i32[0x0810] = 50000);",
    );
    let bytes = scenario::compile("seal-stages", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "FireSealProgress] = s32(story::FireSealStage::GuardianBattleAftermath)",
        "WaterSealProgress] = story::WaterSealStage::SealReleaseConversationComplete",
        "FireSealProgress] = story::FireSealStage::EfreetVictoryAftermathStarted",
        "WaterSealProgress] = story::WaterSealStage::UndinePactCompleteAfterAngelNight",
        "WindSealProgress] = story::WindSealStage::SealReleaseConversationComplete",
        "LightSealProgress] = story::LightSealStage::SealReleaseConversationComplete",
        "WindSealProgress] = story::WindSealStage::SylphPactComplete",
        "encoded(story::WindSealStage::BalacrufMapReceivedAfterEarlierVisit, (7000 + 500))",
        "WindSealProgress] == story::WindSealStage::RuinBombIncidentCompleteAfterBalacrufVisit",
        "WindSealProgress] < story::WindSealStage::BalacrufMapTimerUpperBound",
        "WindSealProgress] == story::WindSealStage::RuinHistoryCorrectAnswerSelector",
        "WindSealProgress] += 1000",
        "FireSealProgress] = 7500",
        "encoded(story::WaterSealStage::PostSealAngelNightComplete, (30000 + 1000))",
        "FireSealProgress] = 1000",
        "WaterSealProgress] = 30001",
        "FireSealProgress] += 50000",
        "WaterSealProgress] = (globals[story::GlobalId::WaterSealProgress] + 31000)",
        "memory::i16[0x00C4] = 21000",
        "memory::i16[0x00C8] = 21000",
        "memory::i32[0x0810] = 50000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (address, domain) in [
        ("0xc0", "story::FireSealStage"),
        ("0xc4", "story::WaterSealStage"),
        ("0xc8", "story::WindSealStage"),
        ("0xcc", "story::LightSealStage"),
    ] {
        assert!(result.identities.iter().any(|id| id.domain == domain
            && id.value == 21000
            && id.parameter == format!("{address}/0x10")
            && id.symbolic));
    }
    let formatted = symphonia_script_compiler::format("seal-stages", &result.source).unwrap();
    assert_eq!(
        scenario::compile("seal-stages", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn seal_progress_catalogues_use_supplied_names_and_require_the_correct_domain() {
    for (global, domain, name) in [
        (
            "FireSealProgress",
            "FireSealStage",
            "GuardianBattleAftermath",
        ),
        (
            "WaterSealProgress",
            "WaterSealStage",
            "SealReleaseConversationComplete",
        ),
        (
            "WindSealProgress",
            "WindSealStage",
            "SealReleaseConversationComplete",
        ),
        (
            "LightSealProgress",
            "LightSealStage",
            "SealReleaseConversationComplete",
        ),
    ] {
        let bytes = scenario::compile(
            "seal-stages",
            &source(&format!(
                "eval (globals[story::GlobalId::{global}] = 21000);"
            )),
            &definitions(),
        )
        .unwrap();
        let mut sources = resonance_script_content::modules();
        let story = sources.get_mut("std::story").unwrap();
        let declaration = format!("{name} = 21000");
        assert!(story.contains(&declaration));
        *story = story.replace(&declaration, &format!("{name}ForMod = 21000"));
        let renamed = Definitions::from_sources(&sources).unwrap();
        let result = scenario::decompile(&bytes, &renamed).unwrap();
        assert!(result.source.contains(&format!("{domain}::{name}ForMod")));
        assert_eq!(
            scenario::compile("seal-stages", &result.source, &renamed).unwrap(),
            bytes
        );
        let story = sources.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {domain}")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        let partial = Definitions::from_sources(&sources).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::story::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn light_seal_puzzle_stages_preserve_encoding_bounds_and_untyped_memory() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::LightSealProgress] = s32(10000));
        eval (globals[story::GlobalId::LightSealProgress] = (12200 + 50));
        eval (globals[story::GlobalId::LightSealProgress] == 1);
        eval (globals[story::GlobalId::LightSealProgress] >= 1001);
        eval (globals[story::GlobalId::LightSealProgress] <= 20000);
        eval (globals[story::GlobalId::LightSealProgress] = 13500);
        eval (globals[story::GlobalId::LightSealProgress] = 13600);
        eval (globals[story::GlobalId::LightSealProgress] += 1000);
        eval (globals[story::GlobalId::WindSealProgress] = 13500);
        eval (globals[story::GlobalId::LightSealProgress] = 99999);
        eval (globals[story::GlobalId::LightSealProgress] = 99998);
        eval (memory::i16[0x00cc] = 13500);
        eval (memory::i32[0x0810] = 13500);",
    );
    let bytes = scenario::compile("light-puzzle", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "LightSealProgress] = s32(story::LightSealStage::LibraryTeamSplitPlanAgreed)",
        "encoded(story::LightSealStage::Room0366MirrorPositionsInitialized, (12200 + 50))",
        "LightSealProgress] == story::LightSealStage::LuinLostTowerKeyHintSelector",
        "LightSealProgress] >= story::LightSealStage::PostIntroductionTimerLowerBound",
        "LightSealProgress] <= story::LightSealStage::GuardianEntryUpperBound",
        "LightSealProgress] = story::LightSealStage::RegroupCallComplete",
        "LightSealProgress] = story::LightSealStage::PartyReunited",
        "LightSealProgress] += 1000",
        "WindSealProgress] = 13500",
        "LightSealProgress] = story::LightSealStage::LibraryReunitedSetupUpperBound",
        "LightSealProgress] = 99998",
        "memory::i16[0x00CC] = 13500",
        "memory::i32[0x0810] = 13500",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "story::LightSealStage" && id.value == 99998 && !id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("light-puzzle", &result.source).unwrap();
    assert_eq!(
        scenario::compile("light-puzzle", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn boltzman_stages_keep_key_search_and_pietro_bargain_separate() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::BoltzmansBookRouteA] = s32(40000));
        eval (globals[story::GlobalId::BoltzmansBookRouteA] = (30000 + 10000));
        eval (globals[story::GlobalId::BoltzmansBookRouteA] = 11000);
        eval (globals[story::GlobalId::BoltzmansBookRouteA] = 12000);
        eval (globals[story::GlobalId::BoltzmansBookRouteB] = 11000);
        eval (globals[story::GlobalId::BoltzmansBookRouteB] = 12000);
        eval (globals[story::GlobalId::BoltzmansBookRouteB] = 40000);
        eval (globals[story::GlobalId::BoltzmansBookRouteA] += 11000);
        eval (globals[story::GlobalId::BoltzmansBookRouteB] = (globals[story::GlobalId::BoltzmansBookRouteB] + 12000));
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] = 20000);
        eval (memory::i16[0x00d0] = 40000);
        eval (memory::i32[0x0810] = 12000);",
    );
    let bytes = scenario::compile("boltzman", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "BoltzmansBookRouteA] = s32(story::BoltzmanTowerKeyStage::TowerKeyReceivedFromPiccaring)",
        "encoded(story::BoltzmanTowerKeyStage::TowerKeyReceivedFromPiccaring, (30000 + 10000))",
        "BoltzmansBookRouteA] = 11000",
        "BoltzmansBookRouteA] = 12000",
        "BoltzmansBookRouteB] = story::PietroInfiltrationStage::SophiaReportedPietroDead",
        "BoltzmansBookRouteB] = story::PietroInfiltrationStage::RanchInfiltrationDealAgreed",
        "BoltzmansBookRouteB] = 40000",
        "BoltzmansBookRouteA] += 11000",
        "BoltzmansBookRouteB] = (globals[story::GlobalId::BoltzmansBookRouteB] + 12000)",
        "AsgardRanchFirstPartRouteA] = 20000",
        "memory::i16[0x00D0] = 40000",
        "memory::i32[0x0810] = 12000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (domain, value, symbolic) in [
        ("story::BoltzmanTowerKeyStage", 40000, true),
        ("story::BoltzmanTowerKeyStage", 12000, false),
        ("story::PietroInfiltrationStage", 12000, true),
        ("story::PietroInfiltrationStage", 40000, false),
    ] {
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.domain == domain && id.value == value && id.symbolic == symbolic)
        );
    }
    let formatted = symphonia_script_compiler::format("boltzman", &result.source).unwrap();
    assert_eq!(
        scenario::compile("boltzman", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn ranch_and_boltzman_stage_names_follow_supplied_source_definitions() {
    for (global, domain, name, value) in [
        (
            "BoltzmansBookRouteA",
            "BoltzmanTowerKeyStage",
            "PiccaringPilgrimageLearned",
            20000,
        ),
        (
            "BoltzmansBookRouteB",
            "PietroInfiltrationStage",
            "SophiaReportedPietroDead",
            11000,
        ),
        (
            "AsgardRanchFirstPartRouteA",
            "AsgardRanchEarlyRouteStage",
            "LuinEscapeAftermathComplete",
            50000,
        ),
        (
            "AsgardRanchFirstPartRouteB",
            "AsgardRanchLaterRouteStage",
            "HimaPietroVisitAgreed",
            50000,
        ),
    ] {
        let bytes = scenario::compile(
            "boltzman",
            &source(&format!(
                "eval (globals[story::GlobalId::{global}] = {value});"
            )),
            &definitions(),
        )
        .unwrap();
        let mut sources = resonance_script_content::modules();
        let story = sources.get_mut("std::story").unwrap();
        let declaration = format!("{name} = {value}");
        assert!(story.contains(&declaration));
        *story = story.replace(&declaration, &format!("{name}ForMod = {value}"));
        let renamed = Definitions::from_sources(&sources).unwrap();
        let result = scenario::decompile(&bytes, &renamed).unwrap();
        assert!(result.source.contains(&format!("{domain}::{name}ForMod")));
        assert_eq!(
            scenario::compile("boltzman", &result.source, &renamed).unwrap(),
            bytes
        );
        let story = sources.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {domain}")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        let partial = Definitions::from_sources(&sources).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::story::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn first_asgard_ranch_routes_keep_their_different_aftermath_and_hima_timing() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] = 50000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteB] = 50000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] < 41000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteB] = 41000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] = 32000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteB] = 32000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] = (35000 + 5000));
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteB] = s32(40000));
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] += 50000);
        eval (globals[story::GlobalId::AsgardRanchFirstPartRouteA] = 20000);
        eval (globals[story::GlobalId::BoltzmansBookRouteA] = 50000);
        eval (memory::i16[0x00d8] = 50000);
        eval (memory::i32[0x0810] = 41000);",
    );
    let bytes = scenario::compile("first-ranch", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "AsgardRanchFirstPartRouteA] = story::AsgardRanchEarlyRouteStage::LuinEscapeAftermathComplete",
        "AsgardRanchFirstPartRouteB] = story::AsgardRanchLaterRouteStage::HimaPietroVisitAgreed",
        "AsgardRanchFirstPartRouteA] < story::AsgardRanchEarlyRouteStage::PostEscapeNpcDialogueBoundary",
        "AsgardRanchFirstPartRouteB] = story::AsgardRanchLaterRouteStage::LuinEscapeAftermathComplete",
        "AsgardRanchFirstPartRouteA] = story::AsgardRanchEarlyRouteStage::VoicesBeforeBottaEncounterHeard",
        "AsgardRanchFirstPartRouteB] = story::AsgardRanchLaterRouteStage::VoicesBeforeBottaEncounterHeard",
        "encoded(story::AsgardRanchEarlyRouteStage::KratosPrivateExsphereDiscussionComplete, (35000 + 5000))",
        "AsgardRanchFirstPartRouteB] = s32(story::AsgardRanchLaterRouteStage::KratosPrivateExsphereDiscussionComplete)",
        "AsgardRanchFirstPartRouteA] += 50000",
        "AsgardRanchFirstPartRouteA] = 20000",
        "BoltzmansBookRouteA] = 50000",
        "memory::i16[0x00D8] = 50000",
        "memory::i32[0x0810] = 41000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (address, domain) in [
        ("0xd8", "story::AsgardRanchEarlyRouteStage"),
        ("0xdc", "story::AsgardRanchLaterRouteStage"),
    ] {
        assert!(result.identities.iter().any(|id| id.domain == domain
            && id.value == 50000
            && id.parameter == format!("{address}/0x10")
            && id.symbolic));
    }
    let formatted = symphonia_script_compiler::format("first-ranch", &result.source).unwrap();
    assert_eq!(
        scenario::compile("first-ranch", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn colette_seal_nights_and_reader_bounds_preserve_original_quirks() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::ColetteAngelProgress] == 101);
        if ((globals[story::GlobalId::ColetteAngelProgress] = 201)) { eval 1; }
        if ((globals[story::GlobalId::ColetteAngelProgress] < s32(301)) && (globals[story::GlobalId::ColetteAngelProgress] >= 20000)) { eval 1; }
        eval (globals[story::GlobalId::ColetteAngelProgress] >= (200 + 100));
        eval (globals[story::GlobalId::ColetteAngelProgress] += 100);
        eval (globals[story::GlobalId::Story] = 1305000);
        eval (memory::i16[0x004c] = 301);
        eval (memory::i32[0x0810] = 300);
        eval (globals[story::GlobalId::WindSealProgress] = 300);",
    );
    let bytes = scenario::compile("colette-stages", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "ColetteAngelProgress] == story::ColetteAngelStage::SecondSealNightComplete",
        "if ((globals[story::GlobalId::ColetteAngelProgress] = story::ColetteAngelStage::ThirdSealNightComplete))",
        "ColetteAngelProgress] < s32(story::ColetteAngelStage::FourthSealNightComplete)) && (globals[story::GlobalId::ColetteAngelProgress] >= story::ColetteAngelStage::IseliaEntryAppealLowerBound)",
        "encoded(story::ColetteAngelStage::VoicelessEligibilityLowerBound, (200 + 100))",
        "ColetteAngelProgress] += 100",
        "Story] = story::StoryStage::FirstAngelNightCampDepartureReady",
        "memory::i16[0x004C] = 301",
        "memory::i32[0x0810] = 300",
        "WindSealProgress] = 300",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (value, operator) in [(101, "0x20"), (201, "0x10"), (301, "0x25"), (20000, "0x23")] {
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.domain == "story::ColetteAngelStage"
                    && id.value == value
                    && id.parameter == format!("0x4c/{operator}")
                    && id.symbolic)
        );
    }
    let formatted = symphonia_script_compiler::format("colette-stages", &result.source).unwrap();
    assert_eq!(
        scenario::compile("colette-stages", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn main_story_reader_bounds_preserve_inclusive_timer_overlap() {
    let definitions = definitions();
    let text = source(
        "eval (globals[story::GlobalId::Story] < 20000000);
        eval (globals[story::GlobalId::Story] <= 20000000);
        if ((globals[story::GlobalId::Story] >= 20000000) && (globals[story::GlobalId::Story] < 21306000)) { eval 1; }
        eval (globals[story::GlobalId::Story] >= 21306000);
        eval (globals[story::GlobalId::Story] < (19999999 + 1));
        eval (globals[story::GlobalId::Story] <= 12400000);
        eval (globals[story::GlobalId::ColetteAngelProgress] = 20000000);
        eval (memory::i32[0x0810] = 21306000);",
    );
    let bytes = scenario::compile("main-bounds", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "Story] < story::StoryStage::LightPactEraBoundary",
        "Story] <= story::StoryStage::LightPactEraBoundary",
        "Story] >= story::StoryStage::LightPactEraBoundary",
        "Story] < story::StoryStage::RuneCrestMaterialsTimerBoundary",
        "Story] >= story::StoryStage::RuneCrestMaterialsTimerBoundary",
        "encoded(story::StoryStage::LightPactEraBoundary, (19999999 + 1))",
        "Story] <= story::StoryStage::FlyingDragonWorldVisibilityUpperBound",
        "ColetteAngelProgress] = 20000000",
        "memory::i32[0x0810] = 21306000",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    for (value, operator) in [
        (20000000, "0x25"),
        (20000000, "0x22"),
        (20000000, "0x23"),
        (21306000, "0x25"),
        (21306000, "0x23"),
    ] {
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.domain == "story::StoryStage"
                    && id.value == value
                    && id.parameter == format!("0x40/{operator}")
                    && id.symbolic)
        );
    }
    let formatted = symphonia_script_compiler::format("main-bounds", &result.source).unwrap();
    assert_eq!(
        scenario::compile("main-bounds", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn recovered_globals_keep_quest_scopes_character_identities_and_plain_counters() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x00c0] = 1000);
        eval (memory::i32[0x0124] = 2);
        eval (memory::i32[0x00fc] = 1000);
        eval (memory::i32[0x00e8] = 1000);
        eval (memory::i32[0x043c] = 1000);
        eval (memory::i32[0x004c] = 1000);
        if ((memory::i32[0x004c] = 201)) { eval 1; }",
    );
    let bytes = scenario::compile("globals", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        result
            .source
            .contains("globals[story::GlobalId::FireSealProgress] = 1000")
    );
    assert!(
        result.source.contains(
            "globals[story::GlobalId::FlanoirCompanion] = characters::CharacterId::Colette"
        )
    );
    assert!(
        result
            .source
            .contains("globals[story::GlobalId::LuinDonationProgress] = 1000")
    );
    assert!(result.source.contains("globals[story::GlobalId::CelsiusPactRoute] = story::CelsiusPactStage::PenguinistGlovesRequirementExplained"));
    assert!(result.source.contains("memory::i32[0x043C] = 1000"));
    assert!(result.source.contains(
        "globals[story::GlobalId::ColetteAngelProgress] = story::ColetteAngelStage::SoulLost"
    ));
    // The retail assignment-in-condition bug stays an assignment, with its
    // scoped value. Naming a stage must not repair original control flow.
    assert!(result.source.contains("if ((globals[story::GlobalId::ColetteAngelProgress] = story::ColetteAngelStage::ThirdSealNightComplete))"));
    assert_eq!(
        scenario::compile("globals", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn treasure_save_flags_and_gald_encoding_keep_item_and_story_domains_separate() {
    let definitions = definitions();
    let text = source(
        "native::create_treasure_chest(22, 2, 0, 0, 0, 0, 0);
        native::create_treasure_chest(22, (1024 + 500), 0, 0, 0, 0, 0);
        native::create_treasure_chest(22, u16(1024), 0, 0, 0, 0, 0);
        eval native::is_treasure_opened(22); native::mark_treasure_opened(22);
        native::set_event_bit(22);",
    )
    .replace("use std::story;", "use std::story; use std::treasure;");
    let bytes = scenario::compile("loot", &text, &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for symbol in [
        "treasure::TreasureFlag::TrietRuins0219LemonGel",
        "items::ItemId::LemonGel",
        "(treasure::TreasureRewardEncoding::GaldBase + 500)",
        "u16(treasure::TreasureRewardEncoding::GaldBase)",
        "story::StoryFlag::RheairdsObtained",
    ] {
        assert!(
            result.source.contains(symbol),
            "missing {symbol}: {}",
            result.source
        );
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "treasure::TreasureRewardEncoding"
                && id.parameter == "reward_base")
    );
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.domain == "items::ItemId" && id.value == 1024)
    );
    assert_eq!(
        scenario::compile("loot", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn ordinary_spawn_domains_follow_the_field_emote_and_battle_branches() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "spawn-protocols",
        &source(
            "native::spawn_actor(1, 0, 0, 0, s16(13), 1, u16(0), s32(-1));
        native::spawn_actor(-100, 0, 0, 0, 13, 999999, 0, native::random_mod(2));
        native::spawn_actor(999999, 0, 0, 0, 13, 363, 0, native::random_mod(2));",
        ),
        &definitions,
    )
    .unwrap();
    let field =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for expected in [
        "actor_id: characters::CharacterId::Lloyd",
        "movement_kind: u16(scenario::ActorBehavior::Stationary)",
        "yaw: scenario::EmoteId::Hearts",
        "resource_id: scenario::ActorSelector::Controlled",
        "actor_id: 999999",
        "resource_id: 363",
        "speed_or_emote_duration: native::random_mod(",
    ] {
        assert!(field.source.contains(expected), "{}", field.source);
    }
    assert_eq!(field.identities.len(), 4);
    assert_eq!(field.source.matches("ActorSelector::Controlled").count(), 1);
    assert!(!field.source.contains("SorcerersRingStation"));
    assert_eq!(
        scenario::compile("spawn-protocols", &field.source, &definitions).unwrap(),
        bytes
    );
    let mut partial_sources = resonance_script_content::modules();
    let module = partial_sources.get_mut("std::scenario").unwrap();
    for domain in ["ActorBehavior", "EmoteId", "ActorSelector"] {
        let start = module.find(&format!("pub enum {domain}")).unwrap();
        let end = start + module[start..].find('}').unwrap() + 1;
        module.replace_range(start..end, "");
    }
    let partial = Definitions::from_sources(&partial_sources).unwrap();
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &partial, context).unwrap();
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.domain == "characters::CharacterId")
        );
        assert!(result.source.contains("movement_kind: u16(0)"));
        assert!(result.source.contains("yaw: s16(13)"));
        assert!(result.source.contains("resource_id: 999999"));
        assert!(result.source.contains("actor_id: 999999"));
        assert!(
            result
                .source
                .contains("speed_or_emote_duration: native::random_mod(")
        );
        let formatted =
            symphonia_script_compiler::format("spawn-protocols", &result.source).unwrap();
        assert_eq!(
            scenario::compile("spawn-protocols", &formatted, &partial).unwrap(),
            bytes
        );
    }
    let error = scenario::decompile_with_context(&bytes, &partial, scenario::Context::Field)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::scenario::ActorBehavior definitions"),
        "{error}"
    );
}

#[test]
fn message_controls_name_substitutions_and_preserve_expression_widths_and_side_effects() {
    use symphonia_script::message::{self, Expression, Token};
    let definitions = definitions();
    let text = "script scenario; use std::scenario; use std::items; use std::characters; use std::techniques;
        code_base = 4; data { word(4); pc(entry); offset(auxiliary); word(0); }
        task entry() { end; }
        auxiliary: data { relative32(line, auxiliary); }
        line: data {
            sjis(\"X\"); hex(\"1F\");
            message_control(scenario::MessageControl::CharacterName, u16(1));
            message_control(scenario::MessageControl::ItemName, s32(1));
            message_control(scenario::MessageControl::TitleName, s16(1));
            message_control(scenario::MessageControl::TechniqueName, 1);
            message_control(scenario::MessageControl::Number, 1);
            message_control(scenario::MessageControl::CharacterName, 999999);
            message_control(scenario::MessageControl::ItemName, (490 + 1));
            message_control(scenario::MessageControl::ItemName, 20000);
            message_control(scenario::MessageControl::Evaluate, (memory::i32[0x810] = native::get_item_count(1)));
            message_control(scenario::MessageControl::Delay, 100);
            message_control(scenario::MessageControl::Color, 3);
            message_control(scenario::MessageControl::Control7, 7);
            message_control(scenario::MessageControl::NumberInput, 2);
            message_control(scenario::MessageControl::Voice, -1);
            byte(0);
        }";
    let bytes = scenario::compile("controls", text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "u16(characters::CharacterId::Lloyd)",
        "s32(items::ItemId::AppleGel)",
        "s16(characters::TitleId::LloydSwordsman)",
        "techniques::TechniqueId::DemonFangLloyd",
        "scenario::MessageControl::Number, 1",
        "scenario::ActorSelector::Controlled",
        "encoded(items::ItemId::MemoryGem, (490 + 1))",
        "scenario::MessageControl::ItemName, 20000",
        "memory::i32[0x0810] = native::get_item_count(item_id: items::ItemId::AppleGel)",
        "scenario::MessageControl::Voice, scenario::VoiceControl::Stop",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert_eq!(result.message_controls, 14);
    assert_eq!(result.promoted_message_controls, 14);
    assert!(result.identities.is_empty());
    assert_eq!(result.message_identities.len(), 9);
    assert_eq!(
        result
            .message_identities
            .iter()
            .filter(|id| !id.symbolic)
            .count(),
        1
    );
    assert!(
        result
            .message_identities
            .iter()
            .any(|id| !id.byte_offset.is_multiple_of(2))
    );
    let formatted = symphonia_script_compiler::format("controls", &result.source).unwrap();
    assert_eq!(
        scenario::compile("controls", &formatted, &definitions).unwrap(),
        bytes
    );
    let edited = scenario::compile(
        "edited",
        &formatted.replace(
            "s32(items::ItemId::AppleGel)",
            "s32(items::ItemId::MemoryGem)",
        ),
        &definitions,
    )
    .unwrap();
    let auxiliary = binary::analyze(&bytes).unwrap().header.auxiliary_offset();
    let originals = message::parse(&bytes[auxiliary..]).unwrap();
    let mut expected = originals.clone();
    expected[0].tokens[2] = Token::Control {
        opcode: 4,
        expression: Expression::Constant(491),
    };
    assert_eq!(message::parse(&edited[auxiliary..]).unwrap(), expected);
    assert_eq!(edited[..auxiliary], bytes[..auxiliary]);
}

#[test]
fn message_control_errors_and_supplied_names_are_checked_without_losing_unknown_values() {
    let definitions = definitions();
    let prefix = "script scenario; use std::scenario; use std::items; code_base = 4; data { word(4); pc(entry); offset(auxiliary); word(0); } task entry() { end; } auxiliary: data { relative32(line, auxiliary); } line: data { ";
    for (body, error) in [
        ("message_control(0, 1);", "unrecognized message control"),
        ("message_control(256, 1);", "must fit u8"),
        (
            "message_control(4, stack_depth());",
            "invalid message control expression",
        ),
    ] {
        let result = scenario::compile(
            "invalid",
            &format!("{prefix}{body} byte(0); }}"),
            &definitions,
        )
        .unwrap_err()
        .to_string();
        assert!(result.contains(error), "{result}");
    }
    let text = format!("{prefix}message_control(4, u16(1)); byte(0); }}");
    let bytes = scenario::compile("controls", &text, &definitions).unwrap();
    let mut sources = resonance_script_content::modules();
    let items = sources.get_mut("std::items").unwrap();
    *items = items.replace("AppleGel = 1,", "ModdedGel = 1,");
    let custom = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("u16(items::ItemId::ModdedGel)"));
    assert_eq!(
        scenario::compile("custom", &result.source, &custom).unwrap(),
        bytes
    );
    sources.remove("std::items");
    let missing = Definitions::from_sources(&sources).unwrap();
    assert!(
        scenario::decompile(&bytes, &missing)
            .err()
            .unwrap()
            .to_string()
            .contains("missing std::items::ItemId definitions")
    );
}

#[test]
fn auxiliary_text_edits_relocate_both_table_widths_aliases_and_odd_byte_targets() {
    use symphonia_script::message::{self, Token};
    let definitions = definitions();
    for width in [2, 4] {
        let first = width * 3;
        let mut auxiliary = Vec::new();
        for offset in [first, first, first + 7] {
            if width == 4 {
                auxiliary.extend((offset as u32).to_be_bytes());
            } else {
                auxiliary.extend((offset as u16).to_be_bytes());
            }
        }
        auxiliary.extend(b"Hello!\0");
        auxiliary.extend([0x82, 0xa0]); // あ
        auxiliary.extend(b"\"\\\nNext\0");
        let mut bytes = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
        bytes.extend(&auxiliary);
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert!(
            result
                .source
                .contains(&format!("relative{}(aux_0000, auxiliary)", width * 8))
        );
        assert!(
            result.source.contains("sjis(\"Hello!\")"),
            "{}",
            result.source
        );
        assert!(
            result.source.contains("あ\\\"\\\\\\nNext"),
            "{}",
            result.source
        );
        assert_eq!(
            scenario::compile("auxiliary", &result.source, &definitions).unwrap(),
            bytes
        );
        let formatted = symphonia_script_compiler::format("auxiliary", &result.source).unwrap();
        assert_eq!(
            scenario::compile("auxiliary", &formatted, &definitions).unwrap(),
            bytes
        );
        let edited = scenario::compile(
            "edited",
            &formatted.replace("Hello!", "A much longer greeting!"),
            &definitions,
        )
        .unwrap();
        let messages = message::parse(&edited[10..]).unwrap();
        assert_eq!(
            messages[0].tokens,
            [Token::Text {
                text: "A much longer greeting!".into()
            }]
        );
        assert_eq!(messages[0], messages[1]);
        assert_eq!(
            messages[2].tokens,
            [Token::Text {
                text: "あ\"\\\nNext".into()
            }]
        );
        let table = message::offset_table(&edited[10..]).unwrap().unwrap();
        assert_eq!(table.entry_bytes, width);
        assert_eq!(
            table.offsets,
            [first, first, first + "A much longer greeting!".len() + 1]
        );
        assert_eq!(edited[..10], bytes[..10]);
        if width == 4 {
            assert_eq!(
                Program::decode(&edited).unwrap().string(2).unwrap(),
                "あ\"\\\nNext"
            );
        }
    }
}

#[test]
fn auxiliary_text_preserves_controls_expressions_formatting_and_invalid_bytes() {
    use symphonia_script::message::{self, Expression, Token};
    let definitions = definitions();
    let mut auxiliary = vec![0, 0, 0, 4];
    auxiliary.extend(b"Hello ");
    let control = [0x1f, 1, 0, 7, 0x30, 0, 0x20, 0xff];
    auxiliary.extend(control);
    auxiliary.extend(b" item!\x0cNext page\0");
    let mut bytes = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
    bytes.extend(auxiliary);
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for text in [
        "sjis(\"Hello \")",
        "sjis(\" item!\")",
        "sjis(\"Next page\")",
    ] {
        assert!(result.source.contains(text), "{}", result.source);
    }
    assert!(!result.source.contains("sjis(\"0\")"));
    let edited = scenario::compile(
        "edited",
        &result.source.replace("Hello ", "Greetings "),
        &definitions,
    )
    .unwrap();
    assert!(
        edited
            .windows(control.len())
            .any(|window| window == control)
    );
    assert_eq!(
        message::parse(&edited[10..]).unwrap()[0].tokens,
        [
            Token::Text {
                text: "Greetings ".into()
            },
            Token::Control {
                opcode: 1,
                expression: Expression::Constant(7)
            },
            Token::Text {
                text: " item!\u{c}Next page".into()
            },
        ]
    );
    // An invalid Shift-JIS body stays raw, without preventing another valid body.
    let mut invalid = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
    invalid.extend([0, 0, 0, 8, 0, 0, 0, 10, 0x81, 0]);
    invalid.extend(b"Good\0");
    let result = scenario::decompile(&invalid, &definitions).unwrap();
    assert!(result.source.contains("sjis(\"Good\")"));
    assert!(result.source.contains("8100"));
    assert_eq!(
        scenario::compile("invalid", &result.source, &definitions).unwrap(),
        invalid
    );
    // EEEF and FA40 decode to the same character; preserve the original spelling.
    let mut noncanonical = vec![0, 4, 0, 0, 0, 5, 0, 0, 0x20, 0xff];
    noncanonical.extend([0, 0, 0, 8, 0, 0, 0, 11, 0xee, 0xef, 0]);
    noncanonical.extend(b"Good\0");
    let result = scenario::decompile(&noncanonical, &definitions).unwrap();
    assert!(result.source.contains("EEEF00"), "{}", result.source);
    assert!(result.source.contains("sjis(\"Good\")"));
    assert_eq!(
        scenario::compile("noncanonical", &result.source, &definitions).unwrap(),
        noncanonical
    );
}

#[test]
fn auxiliary_data_diagnoses_unrepresentable_text_and_invalid_relative_references() {
    let definitions = definitions();
    let prefix = "script scenario; code_base = 4; data { word(4); pc(entry); word(0); word(0); } task entry() { end; } ";
    for (suffix, expected) in [
        (
            "data { sjis(\"😀\"); }",
            "cannot be represented in Shift-JIS",
        ),
        ("data { sjis(7); }", "expected Shift-JIS text literal"),
        (
            "data { relative32(missing, entry); }",
            "undefined label 'missing'",
        ),
        (
            "data { relative32(entry, missing); }",
            "undefined label 'missing'",
        ),
        (
            "data { relative32(entry, later); } later:",
            "precedes base 'later'",
        ),
    ] {
        let error = scenario::compile("invalid", &format!("{prefix}{suffix}"), &definitions)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
    let payload = "A".repeat(65536);
    let text =
        format!("{prefix} base: data {{ relative16(target, base); sjis(\"{payload}\"); }} target:");
    assert!(
        scenario::compile("overflow", &text, &definitions)
            .unwrap_err()
            .to_string()
            .contains("relative byte offset exceeds u16")
    );
    assert!(
        scenario::compile(
            "wide",
            &text.replace("relative16", "relative32"),
            &definitions
        )
        .is_ok()
    );
}

#[test]
fn model_node_queries_name_field_selectors_and_distinguish_names_from_body_parts() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "node-queries",
        &source(
            "eval native::find_actor_node(999996,u16(50));
        eval native::find_actor_node(999998,memory::i32[0x810]);
        eval native::find_actor_node(999997,0);
        eval native::find_actor_node(999980,2);
        eval native::find_actor_node(999999,52);
        eval native::find_actor_body_part(999999,u16(2));
        eval native::find_actor_body_part(999996,(1 + 1));
        eval native::find_actor_body_part(2,56);
        eval native::find_actor_body_part(0,2);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(373)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: scenario::ActorSelector::MainScenery",
            "actor_id: scenario::ActorSelector::SecondScenery",
            "actor_id: scenario::ActorSelector::ThirdScenery",
            "actor_id: scenario::ActorSelector::FourthScenery",
            "actor_id: scenario::ActorSelector::Controlled",
            "name_index: u16(50)",
            "name_index: memory::i32[0x0810]",
            "name_index: 52",
            "body_part: u16(scenario::BodyPartId::Spine1)",
            "encoded(scenario::BodyPartId::Spine1, (1 + 1))",
            "body_part: 56",
            "actor_id: characters::CharacterId::Colette",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 12);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.parameter != "name_index")
        );
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            1
        );
        let formatted = symphonia_script_compiler::format("node-queries", &result.source).unwrap();
        assert_eq!(
            scenario::compile("node-queries", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("scenario::ActorSelector"));
        assert!(result.source.contains("actor_id: 999999"));
        assert!(
            result
                .source
                .contains("body_part: scenario::BodyPartId::Spine1")
        );
        assert_eq!(
            scenario::compile("node-queries", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn particle_constructors_name_shared_recipes_without_typing_scalar_operands() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "particles",
        &source(
            "eval native::create_particle(u16(25),s32(32767),73,491,-1,10,20,30,50,255,-8,33,8);
        eval native::create_particle((20 + 6),10,0,0,0,0,0,0,1,255,0,0,-1);
        eval native::create_effect_object(32,60,0,0,0,3,4,0,199,25,200,-1,34,0);
        eval native::create_effect_object(39,60,0,0,0,3,4,0,-199,25,200,-1,34,2);
        eval native::create_particle(501,20,0,0,0,0,0,0,25,255,0,0,0);
        eval native::create_effect_object(memory::i32[0x810],20,0,0,0,0,0,0,0,25,255,0,0,0);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::Battle,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "sprite: u16(scenario::ParticleRecipe::FallingLeaf)",
            "encoded(scenario::ParticleRecipe::RisingLeaf, (20 + 6))",
            "sprite: scenario::ParticleRecipe::TextureSlot0",
            "sprite: scenario::ParticleRecipe::TextureSlot7",
            "lifetime: s32(32767)",
            "x: 73",
            "y: 491",
            "z: -1",
            "velocity_x: 10",
            "velocity_y: 20",
            "velocity_z: 30",
            "size: 50",
            "alpha: 255",
            "fade: -8",
            "palette: 33",
            "variant: 8",
            "direction_x: 3",
            "direction_y: 4",
            "speed_percent: 199",
            "speed_percent: -199",
            "sprite: 501",
            "sprite: memory::i32[0x0810]",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 5);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.domain == "scenario::ParticleRecipe" && id.parameter == "sprite")
        );
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            1
        );
        let formatted = symphonia_script_compiler::format("particles", &result.source).unwrap();
        assert_eq!(
            scenario::compile("particles", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn attachment_queries_name_field_actors_and_preserve_battle_ignored_operands() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "attachment-query",
        &source(
            "native::read_actor_attachment(999999,84);
        native::read_actor_attachment(999996,1);
        native::read_actor_attachment(999997,0);
        native::read_actor_attachment(999998,memory::i32[0x810]);
        native::read_actor_attachment(999980,84);
        native::read_actor_attachment((1 + 0),84);
        native::read_actor_attachment(5,0);
        native::read_actor_attachment(1,-1);
        native::read_actor_attachment(999999,-1);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(511)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: scenario::ActorSelector::Controlled",
            "actor_id: scenario::ActorSelector::MainScenery",
            "actor_id: scenario::ActorSelector::SecondScenery",
            "actor_id: scenario::ActorSelector::ThirdScenery",
            "actor_id: scenario::ActorSelector::FourthScenery",
            "encoded(characters::CharacterId::Lloyd, (1 + 0))",
            "actor_id: characters::CharacterId::Sheena",
            "bone_index: 84",
            "bone_index: memory::i32[0x0810]",
            "actor_id: 1, bone_index: -1",
            "actor_id: 999999, bone_index: -1",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 7);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.parameter == "actor_id" && id.symbolic)
        );
        let formatted =
            symphonia_script_compiler::format("attachment-query", &result.source).unwrap();
        assert_eq!(
            scenario::compile("attachment-query", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(result.identities.is_empty(), "{}", result.source);
        assert!(result.source.contains("actor_id: 999999, bone_index: 84"));
        assert_eq!(
            scenario::compile("attachment-query", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn bone_rotation_and_scale_signatures_distinguish_axes_slots_and_duration() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "bone-controllers",
        &source(
            "native::configure_actor_bone_rotation(999999,u16(2),84,s32(10),u16(20),s16(30));
        native::configure_actor_attachment(999996,(1 + 1),memory::i32[0x810],10,20,30,0);
        native::configure_actor_bone_scale(999998,7,84,-100,0,200,0);
        native::configure_actor_bone_rotation(1,99,-1,native::random_mod(2),0,0);
        native::configure_actor_attachment(999999,99,-1,0,0,0,-1);
        native::configure_actor_bone_scale(1,99,-1,0,0,0,-1);
        native::configure_actor_attachment(4,8,84,0,0,0,1);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(511)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: scenario::ActorSelector::Controlled",
            "actor_id: scenario::ActorSelector::MainScenery",
            "actor_id: scenario::ActorSelector::ThirdScenery",
            "controller_slot: u16(scenario::BoneControllerSlot::Slot2)",
            "encoded(scenario::BoneControllerSlot::Slot2, (1 + 1))",
            "controller_slot: scenario::BoneControllerSlot::Slot7",
            "rotation_x: s32(10)",
            "rotation_z: u16(20)",
            "rotation_y: s16(30)",
            "duration: 0",
            "bone_index: 84",
            "bone_index: memory::i32[0x0810]",
            "scale_x_percent: -100",
            "scale_y_percent: 0",
            "scale_z_percent: 200",
            "actor_id: 1",
            "bone_index: -1",
            "rotation_x: native::random_mod(",
            "controller_slot: 99",
            "duration: -1",
            "controller_slot: 8",
            "actor_id: characters::CharacterId::Raine",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.parameter == "bone_index")
        );
        assert_eq!(result.identities.len(), 8);
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            1
        );
        let formatted =
            symphonia_script_compiler::format("bone-controllers", &result.source).unwrap();
        assert_eq!(
            scenario::compile("bone-controllers", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("scenario::ActorSelector"));
        assert!(result.source.contains("actor_id: 999999"));
        assert_eq!(
            scenario::compile("bone-controllers", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn bone_translation_and_frame_waits_name_lookups_without_typing_bone_indices() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "bone-abi",
        &source(
            "native::configure_actor_bone_translation(999996,u16(2),84,s32(73),u16(491),s16(-1),0);
        native::configure_actor_bone_translation(999998,7,memory::i32[0x810],0,0,0,30);
        native::configure_actor_bone_translation(999997,8,84,0,0,0,1);
        native::configure_actor_bone_translation(999999,0,84,0,0,0,1);
        native::configure_actor_bone_translation(1,0,-1,native::random_mod(2),0,0,-1);
        native::wait_actor_animation_frame(999999,s32(3));
        native::wait_actor_animation_frame(4,(2 + 1));",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(511)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: scenario::ActorSelector::MainScenery",
            "actor_id: scenario::ActorSelector::ThirdScenery",
            "actor_id: scenario::ActorSelector::SecondScenery",
            "actor_id: scenario::ActorSelector::Controlled",
            "controller_slot: u16(scenario::BoneControllerSlot::Slot2)",
            "controller_slot: scenario::BoneControllerSlot::Slot7",
            "controller_slot: 8",
            "bone_index: 84",
            "bone_index: memory::i32[0x0810]",
            "x: s32(73)",
            "y: u16(491)",
            "z: s16(-1)",
            "duration: 0",
            "actor_id: 1",
            "bone_index: -1",
            "x: native::random_mod(",
            "frame: s32(3)",
            "actor_id: characters::CharacterId::Raine",
            "frame: (2 + 1)",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.parameter == "bone_index")
        );
        assert_eq!(result.identities.len(), 10);
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            1
        );
        let formatted = symphonia_script_compiler::format("bone-abi", &result.source).unwrap();
        assert_eq!(
            scenario::compile("bone-abi", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("scenario::ActorSelector"));
        assert!(result.source.contains("actor_id: 999999"));
        assert_eq!(
            scenario::compile("bone-abi", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn session_commands_type_only_consumed_toggle_values_and_keep_ignored_expressions() {
    let definitions = definitions();
    let mut body = String::new();
    for command in [3, 4, 5, 12, 15] {
        for value in [-1, 0, 1, 2, 65535] {
            body.push_str(&format!(
                "eval native::configure_session({command},{value});\n"
            ));
        }
    }
    for command in [10, 11, 13, 14, 16, 18] {
        body.push_str(&format!("eval native::configure_session({command},0);\n"));
    }
    body.push_str("eval native::configure_session(9,-1); eval native::configure_session(memory::i32[0x810],-1); eval native::configure_session(11,native::random_mod(2)); eval native::configure_session((3 + 9),u16(1));");
    let bytes = scenario::compile("session-commands", &source(&body), &definitions).unwrap();
    for context in [scenario::Context::Field, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for command in symphonia_constants::SessionCommand::ALL {
            assert!(
                result.source.contains(&format!(
                    "command: scenario::SessionCommand::{}",
                    command.name()
                )),
                "{}",
                result.source
            );
        }
        for expected in [
            "value: scenario::SessionToggle::Query",
            "value: scenario::SessionToggle::Disabled",
            "value: scenario::SessionToggle::Enabled",
            "value: 2",
            "value: 65535",
            "command: 9, value: -1",
            "command: memory::i32[0x0810], value: -1",
            "command: scenario::SessionCommand::GameClearCount, value: 0",
            "value: native::random_mod(",
            "encoded(scenario::SessionCommand::LeaderLocked, (3 + 9))",
            "value: u16(scenario::SessionToggle::Enabled)",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 60);
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "scenario::SessionToggle")
                .count(),
            26
        );
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            11
        );
        let formatted =
            symphonia_script_compiler::format("session-commands", &result.source).unwrap();
        assert_eq!(
            scenario::compile("session-commands", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn party_recovery_modes_name_operations_and_keep_gald_as_a_signed_amount() {
    let definitions = definitions();
    let mut body = (0..=25)
        .map(|mode| format!("native::heal_party({mode});\n"))
        .collect::<String>();
    body.push_str("native::heal_party(-1); native::heal_party((3 + 1)); native::heal_party(u16(23)); native::heal_party(s32(0)); eval native::add_gald(-100); eval native::add_gald((1000 - 500));");
    let bytes = scenario::compile("party-operations", &source(&body), &definitions).unwrap();
    for context in [scenario::Context::Field, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for mode in symphonia_constants::PartyRecoveryMode::ALL {
            assert!(
                result.source.contains(&format!(
                    "mode: scenario::PartyRecoveryMode::{}",
                    mode.name()
                )),
                "{}",
                result.source
            );
        }
        for expected in [
            "mode: 1",
            "mode: 24",
            "mode: 25",
            "mode: -1",
            "encoded(scenario::PartyRecoveryMode::RestoreHp10Percent, (3 + 1))",
            "mode: u16(scenario::PartyRecoveryMode::ReviveIncapacitated)",
            "mode: s32(scenario::PartyRecoveryMode::Rest)",
            "native::add_gald(amount: -100)",
            "native::add_gald(amount: (1000 - 500))",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 30);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.domain == "scenario::PartyRecoveryMode" && id.native == "heal_party")
        );
        assert_eq!(
            result.identities.iter().filter(|id| id.symbolic).count(),
            26
        );
        let formatted =
            symphonia_script_compiler::format("party-operations", &result.source).unwrap();
        assert_eq!(
            scenario::compile("party-operations", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn trigger_geometry_arguments_keep_record_keys_and_coordinates_numeric() {
    let definitions = definitions();
    let bytes = scenario::compile("trigger-geometry", &source(
        "native::create_automatic_event_trigger(491, u16(491), s32(73), s16(-1), (360 + 3), 13, 90, u16(850));
        native::create_script_record(999999, 491, 73, 13, 363, 90, 0, -1);
        native::create_script_record_variant(73, u16(491), s32(850), -1, 1, 2, 3, 4, 5, 6, 200);
        native::create_triangle_trigger(1, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100);
        native::remove_automatic_event_triggers(491);
        native::remove_touch_triggers(999999);
        native::remove_confirmed_triggers(73);"), &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(511),
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "record_key: 491",
            "record_key: 999999",
            "record_key: 73",
            "record_key: 1",
            "x0: u16(491)",
            "y0: s32(73)",
            "z0: s16(-1)",
            "x1: (360 + 3)",
            "height: u16(850)",
            "value_10: u16(491)",
            "value_12: s32(850)",
            "value_14: -1",
            "x2: 70",
            "y2: 80",
            "z2: 90",
            "height: 100",
            "native::remove_automatic_event_triggers(record_key: 491)",
            "native::remove_touch_triggers(record_key: 999999)",
            "native::remove_confirmed_triggers(record_key: 73)",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert!(result.identities.is_empty(), "{:?}", result.identities);
        let formatted =
            symphonia_script_compiler::format("trigger-geometry", &result.source).unwrap();
        assert_eq!(
            scenario::compile("trigger-geometry", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn scenery_animation_domains_follow_each_handlers_layer_routing() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "scenery-domains",
        &source(
            "native::configure_scenery_animation(999980, s16(-1), u16(178), 0, -100, (2 | 8));
        native::configure_scenery_animation(999997, 0, (177 + 1), 1, 100, 1);
        native::configure_scenery_animation(4, 3, -1179648, 0, 0, 0);
        native::configure_scenery_animation(0, 4, 60000, 0, 100, 64);
        native::configure_scenery_animation(999996, 2, -1179648, 0, 100, s32(65544));
        native::clear_scenery_animation(999980, -1);
        native::seek_scenery_animation(999997, 0, 178);
        native::set_scenery_animation_rate(999998, 1, -100);
        native::clear_scenery_animation(4, 2);
        native::clear_scenery_animation(2, 3);
        native::seek_scenery_animation(3, -1, 10);
        native::set_scenery_animation_rate(999996, 0, 100);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(520)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "layer: scenario::SceneryLayer::FourthActor",
            "layer: scenario::SceneryLayer::SecondActor",
            "layer: scenario::SceneryLayer::Fourth",
            "layer: scenario::SceneryLayer::Default",
            "channel: s16(scenario::SceneryAnimationChannel::Base)",
            "channel: scenario::SceneryAnimationChannel::Slot3",
            "resource_id: u16(resources::FieldAnimationId::UnassignedMotion178)",
            "encoded(resources::FieldAnimationId::UnassignedMotion178, (177 + 1))",
            "flags: (scenario::AnimationFlag::Paused | scenario::AnimationFlag::Once)",
            "flags: scenario::AnimationFlag::Default",
            "flags: scenario::AnimationFlag::None",
            "flags: flags(scenario::AnimationFlag::Once, 0x10000)",
            "layer: 999980",
            "layer: 999997",
            "layer: 999998",
            "layer: 4",
            "layer: scenario::SceneryLayer::Second",
            "layer: scenario::SceneryLayer::Third",
            "layer: scenario::SceneryLayer::MainActor",
            "resource_id: -1179648",
            "resource_id: 60000",
            "channel: 4",
            "flags: 64",
            "frame: 178",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert!(
            !result
                .identities
                .iter()
                .any(|id| id.native != "configure_scenery_animation"
                    && id.parameter == "layer"
                    && matches!(id.value, 4 | 999980 | 999997 | 999998))
        );
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "resources::FieldAnimationId")
                .count(),
            3
        );
        let formatted =
            symphonia_script_compiler::format("scenery-domains", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scenery-domains", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("resources::FieldAnimationId"));
        assert!(result.source.contains("scenario::AnimationFlag::Once"));
        assert_eq!(
            scenario::compile("scenery-domains", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn no_decision_behavior_encodings_remain_distinct_and_keep_full_input_values() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "no-decision-behaviors",
        &source(
            "native::spawn_actor(100, 0, 0, 0, 0, 363, s16(13), 0);
        native::spawn_actor(101, 0, 0, 0, 0, 363, (80 + 10), 0);
        native::spawn_actor(102, 0, 0, 0, 0, 363, 300, 0);
        native::create_scene_actor(103, 0, 0, 0, 0, 24, u16(90), 0);
        eval native::set_actor_property(100, 34, s32(13));
        eval 13; eval 90; eval 300;",
        ),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for expected in [
        "movement_kind: s16(scenario::ActorBehavior::NoDecision13)",
        "encoded(scenario::ActorBehavior::NoDecision90, (80 + 10))",
        "movement_kind: 300",
        "behavior: u16(scenario::ActorBehavior::NoDecision90)",
        "value: s32(scenario::ActorBehavior::NoDecision13)",
        "eval 13;",
        "eval 90;",
        "eval 300;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    let behaviors = result
        .identities
        .iter()
        .filter(|id| id.domain == "scenario::ActorBehavior")
        .collect::<Vec<_>>();
    assert_eq!(
        behaviors
            .iter()
            .map(|id| (id.value, id.symbolic))
            .collect::<Vec<_>>(),
        [(13, true), (90, true), (300, false), (90, true), (13, true)]
    );
    let formatted =
        symphonia_script_compiler::format("no-decision-behaviors", &result.source).unwrap();
    assert_eq!(
        scenario::compile("no-decision-behaviors", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn builtin_sphere_names_only_consumed_field_models_and_keeps_the_encoding() {
    let definitions = definitions();
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    let mut body = String::new();
    for (opcode, slot, expression) in [
        (0x10, 5, "u16(24)"),
        (0xb9, 5, "s32(24)"),
        (0xba, 5, "(20 + 4)"),
        (0xbb, 5, "24"),
        (0xd6, 5, "24"),
        (0x5c, 5, "24"),
        (0x1f, 10, "24"),
        (0xc6, 11, "24"),
    ] {
        let native = registry.get(opcode).unwrap();
        let mut args = vec!["0"; native.arguments.len()];
        args[0] = "400";
        args[1] = "24"; // Equal coordinates and other operands are not models.
        args[slot] = expression;
        body.push_str(&format!(
            "{}native::{}({});\n",
            if native.returns_value == Some(true) {
                "eval "
            } else {
                ""
            },
            native.name,
            args.join(", ")
        ));
    }
    body.push_str(
        "native::spawn_actor(-100, 24, 24, 24, 0, 24, 0, 0);
        native::spawn_actor(999999, 24, 24, 24, 0, 24, 0, 0);
        native::spawn_actor(memory::i32[0x810], 0, 0, 0, 0, 24, 0, 0);
        native::spawn_actor(400, 0, 0, 0, 0, 25, 0, 0);
        eval 24;",
    );
    let bytes = scenario::compile("sphere-consumers", &source(&body), &definitions).unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(373)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let models = result
            .identities
            .iter()
            .filter(|id| id.domain == "resources::BuiltinFieldModelId")
            .collect::<Vec<_>>();
        assert_eq!(models.len(), 8);
        assert!(models.iter().all(|id| id.symbolic && id.value == 24));
        for expected in [
            "u16(resources::BuiltinFieldModelId::Sphere)",
            "s32(resources::BuiltinFieldModelId::Sphere)",
            "encoded(resources::BuiltinFieldModelId::Sphere, (20 + 4))",
            "resource_id: 24",
            "resource_id: 25",
            "eval 24;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        let formatted =
            symphonia_script_compiler::format("sphere-consumers", &result.source).unwrap();
        assert_eq!(
            scenario::compile("sphere-consumers", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [
        scenario::Context::Battle,
        scenario::Context::World,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("BuiltinFieldModelId"));
        assert_eq!(
            scenario::compile("sphere-consumers", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn builtin_sphere_uses_optional_selected_names_and_explicit_scene_overrides() {
    let bytes = scenario::compile("sphere-definitions", &source("native::spawn_actor(400, 0, 0, 0, 0, 24, 0, 0); native::spawn_actor(401, 0, 0, 0, 0, 25, 0, 0);"), &definitions()).unwrap();
    for (replacement, expected) in [
        (
            "ModSphere = 24,",
            Some("resources::BuiltinFieldModelId::ModSphere"),
        ),
        ("", None),
        ("MovedSphere = 25,", None),
    ] {
        let mut modules = resonance_script_content::modules();
        let resources = modules.get_mut("std::resources").unwrap();
        assert_eq!(resources.matches("Sphere = 24,").count(), 1);
        *resources = resources.replace("Sphere = 24,", replacement);
        let selected = Definitions::from_sources(&modules).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &selected, scenario::Context::Field).unwrap();
        if let Some(name) = expected {
            assert!(result.source.contains(name));
        } else {
            assert!(!result.source.contains("BuiltinFieldModelId"));
        }
        assert!(result.source.contains("resource_id: 25"));
        assert_eq!(
            scenario::compile("sphere-definitions", &result.source, &selected).unwrap(),
            bytes
        );
    }
    let mut modules = resonance_script_content::modules();
    let resources = modules.get_mut("std::resources").unwrap();
    let start = resources.find("pub enum BuiltinFieldModelId").unwrap();
    let end = start + resources[start..].find('}').unwrap() + 1;
    resources.replace_range(start..end, "");
    let legacy = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &legacy, scenario::Context::Field).unwrap();
    assert!(!result.source.contains("BuiltinFieldModelId"));
    assert_eq!(
        scenario::compile("sphere-definitions", &result.source, &legacy).unwrap(),
        bytes
    );
    modules
        .get_mut("std::resources")
        .unwrap()
        .push_str("\npub enum Field0373ModelId { CustomMarker = 24, }\n");
    let scoped = Definitions::from_sources(&modules).unwrap();
    for field in [372, 373] {
        let result =
            scenario::decompile_with_context(&bytes, &scoped, scenario::Context::FieldScene(field))
                .unwrap();
        assert_eq!(
            result
                .source
                .contains("resources::Field0373ModelId::CustomMarker"),
            field == 373
        );
        assert_eq!(
            scenario::compile("sphere-definitions", &result.source, &scoped).unwrap(),
            bytes
        );
    }
    // A scene definition also takes precedence when the built-in enum exists.
    let mut modules = resonance_script_content::modules();
    modules
        .get_mut("std::resources")
        .unwrap()
        .push_str("\npub enum Field0373ModelId { CustomMarker = 24, }\n");
    let scoped = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &scoped, scenario::Context::FieldScene(373))
            .unwrap();
    assert!(
        result
            .source
            .contains("resources::Field0373ModelId::CustomMarker")
    );
    assert!(!result.source.contains("BuiltinFieldModelId"));
    assert_eq!(
        scenario::compile("sphere-definitions", &result.source, &scoped).unwrap(),
        bytes
    );
}

#[test]
fn scene_constructor_arguments_name_resources_and_behaviors_independently_of_keys() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "scene-constructors",
        &source(
            "native::create_scene_actor(4, u16(363), s32(13), s16(-1), 90, u16(363), 0, 1);
        native::spawn_interaction_actor(-100, 0, 0, 0, 13, (360 + 3), 0, 0);
        native::create_scene_actor(999999, 0, 0, 0, 0, -1179648, 90, 0);
        native::spawn_interaction_actor(memory::i32[0x810], 0, 0, 0, 0, 24, 300, 0);
        native::create_scene_actor(100, 0, 0, 0, 0, 60000, 13, 0);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(520)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: 4",
            "actor_id: -100",
            "actor_id: 999999",
            "actor_id: memory::i32[0x0810]",
            "x: u16(363)",
            "y: s32(13)",
            "z: s16(-1)",
            "heading: 90",
            "resource: u16(resources::FieldModelId::SorcerersRingStation)",
            "encoded(resources::FieldModelId::SorcerersRingStation, (360 + 3))",
            "behavior: scenario::ActorBehavior::Stationary",
            "resource: resources::BuiltinFieldModelId::Sphere",
            "resource: 60000",
            "behavior: scenario::ActorBehavior::NoDecision90",
            "behavior: 300",
            "behavior: scenario::ActorBehavior::NoDecision13",
            "speed: 1",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        let scoped = context == scenario::Context::FieldScene(520);
        assert!(result.source.contains(if scoped {
            "resource: resources::Field0520ModelId::PinkFlower"
        } else {
            "resource: -1179648"
        }));
        assert_eq!(result.identities.len(), if scoped { 10 } else { 9 });
        assert!(result.identities.iter().all(|id| matches!(
            (id.domain.as_str(), id.parameter.as_str()),
            (
                "resources::FieldModelId"
                    | "resources::Field0520ModelId"
                    | "resources::BuiltinFieldModelId",
                "resource"
            ) | ("scenario::ActorBehavior", "behavior")
        )));
        assert_eq!(
            result.identities.iter().filter(|id| id.symbolic).count(),
            if scoped { 8 } else { 7 }
        );
        let formatted =
            symphonia_script_compiler::format("scene-constructors", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scene-constructors", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    // These handlers have no mode9 branch, so behavior still has its domain.
    // Shared field models require a known field host.
    for context in [scenario::Context::Battle, scenario::Context::Unspecified] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("resources::FieldModelId"));
        assert_eq!(result.identities.len(), 5);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.domain == "scenario::ActorBehavior")
        );
        assert_eq!(
            scenario::compile("scene-constructors", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn physics_spawn_keys_do_not_select_emotes_or_existing_actors() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "physics-spawn",
        &source(
            "native::spawn_collision_actor(-100, 0, 0, 0, 13, u16(363), 0, 0);
        native::spawn_scenery_actor(999999, 0, 0, 0, 13, (360 + 3), 0, 0);
        native::spawn_collision_actor(memory::i32[0x810], 0, 0, 0, 13, 363, 0, 0);
        native::spawn_scenery_actor(4, 0, 0, 0, 13, 60000, 13, 0);",
        ),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for expected in [
        "actor_id: -100",
        "actor_id: 999999",
        "actor_id: 4",
        "actor_id: memory::i32[0x0810]",
        "resource: u16(resources::FieldModelId::SorcerersRingStation)",
        "encoded(resources::FieldModelId::SorcerersRingStation, (360 + 3))",
        "resource: resources::FieldModelId::SorcerersRingStation",
        "behavior: scenario::ActorBehavior::Stationary",
        "resource: 60000",
        "behavior: scenario::ActorBehavior::NoDecision13",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(result.identities.len(), 8);
    assert!(result.identities.iter().all(|id| matches!(
        id.domain.as_str(),
        "resources::FieldModelId" | "scenario::ActorBehavior"
    )));
    let formatted = symphonia_script_compiler::format("physics-spawn", &result.source).unwrap();
    assert_eq!(
        scenario::compile("physics-spawn", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn ring_station_source_names_only_the_model_and_keeps_the_skipped_expression() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "ring-station",
        &source(
            "native::create_ring_station(999999, u16(363), s32(13), s16(-1), 13, u16(363));
        native::create_ring_station(4, 0, 0, 363, (10 + 3), (360 + 3));
        native::create_ring_station(100, 0, 0, 0, native::random_mod(2), memory::i32[0x810]);
        native::create_ring_station(100, 0, 0, 0, 13, 60000);
        native::create_ring_station(100, 0, 0, 0, 13, 24);
        native::create_ring_station(100, 0, 0, 0, 13, -1);",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::FieldScene(412)] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "actor_id: 999999",
            "actor_id: 4",
            "x: u16(363)",
            "y: s32(13)",
            "z: s16(-1)",
            "unused_4: 13",
            "unused_4: (10 + 3)",
            "unused_4: native::random_mod(",
            "resource_id: u16(resources::FieldModelId::SorcerersRingStation)",
            "encoded(resources::FieldModelId::SorcerersRingStation, (360 + 3))",
            "resource_id: memory::i32[0x0810]",
            "resource_id: 60000",
            "resource_id: resources::BuiltinFieldModelId::Sphere",
            "resource_id: -1",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        assert_eq!(result.identities.len(), 4);
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.native == "create_ring_station"
                    && id.parameter == "resource_id"
                    && matches!(
                        id.domain.as_str(),
                        "resources::FieldModelId" | "resources::BuiltinFieldModelId"
                    ))
        );
        assert_eq!(
            result
                .identities
                .iter()
                .map(|id| (id.value, id.symbolic))
                .collect::<Vec<_>>(),
            [(363, true), (363, true), (60000, false), (24, true)]
        );
        let formatted = symphonia_script_compiler::format("ring-station", &result.source).unwrap();
        assert_eq!(
            scenario::compile("ring-station", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn ring_station_model_names_follow_field_context_and_the_actual_definitions() {
    let bytes = scenario::compile(
        "ring-station",
        &source("native::create_ring_station(999999, 363, 363, 363, 13, 363);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let resources = modules.get_mut("std::resources").unwrap();
    *resources = resources.replace("SorcerersRingStation = 363", "CustomStation = 363");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &custom, scenario::Context::Field).unwrap();
    assert!(
        result
            .source
            .contains("resource_id: resources::FieldModelId::CustomStation")
    );
    assert_eq!(
        scenario::compile("ring-station", &result.source, &custom).unwrap(),
        bytes
    );
    let resources = modules.get_mut("std::resources").unwrap();
    *resources = resources.replace("CustomStation = 363", "CustomStation = 50000");
    let moved = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &moved, scenario::Context::Field).unwrap();
    assert!(result.source.contains("resource_id: 363"));
    assert_eq!(result.identities.len(), 1);
    assert!(!result.identities[0].symbolic);
    assert_eq!(
        scenario::compile("ring-station", &result.source, &moved).unwrap(),
        bytes
    );
    modules.remove("std::resources");
    let partial = Definitions::from_sources(&modules).unwrap();
    let error = scenario::decompile_with_context(&bytes, &partial, scenario::Context::Field)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::resources::FieldModelId definitions"),
        "{error}"
    );
    for context in [
        scenario::Context::Unspecified,
        scenario::Context::Battle,
        scenario::Context::World,
        scenario::Context::Skit,
    ] {
        let result = scenario::decompile_with_context(&bytes, &partial, context).unwrap();
        assert!(result.identities.is_empty());
        assert!(result.source.contains("resource_id: 363"));
        assert_eq!(
            scenario::compile("ring-station", &result.source, &partial).unwrap(),
            bytes
        );
    }
}

#[test]
fn sound_argument_names_and_legacy_spellings_keep_native_order_and_bytes() {
    let definitions = definitions();
    let legacy = source(
        "native::play_sound_simple(sound_id: u16(189), raw_velocity: (memory::i32[0x810])++);
        native::play_sound(sound_id: s32(384), raw_velocity: (1 - 3), channel: u16(90), note: 4);
        native::play_sound(385, pan_offset: s16(-2), channel: (80 + 10), binding_index: u16(1));
        native::play_sound(sound_id: -1, raw_velocity: 0, channel: 0, note: 4);",
    );
    let canonical = legacy
        .replace("raw_velocity:", "pan_offset:")
        .replace("channel:", "volume:")
        .replace("note:", "binding_index:");
    let bytes = scenario::compile("canonical-sound", &canonical, &definitions).unwrap();
    assert_eq!(
        scenario::compile("legacy-sound", &legacy, &definitions).unwrap(),
        bytes
    );
    let formatted_legacy = symphonia_script_compiler::format("legacy-sound", &legacy).unwrap();
    assert_eq!(
        scenario::compile("formatted-legacy", &formatted_legacy, &definitions).unwrap(),
        bytes
    );
    for context in [
        scenario::Context::Field,
        scenario::Context::Battle,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "pan_offset: (memory::i32[0x0810])++",
            "pan_offset: (1 - 3)",
            "volume: u16(90)",
            "binding_index: 4",
            "pan_offset: s16(-2)",
            "volume: (80 + 10)",
            "binding_index: u16(1)",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        assert!(
            !result.source.contains("raw_velocity:")
                && !result.source.contains("channel:")
                && !result.source.contains("note:")
        );
        let formatted =
            symphonia_script_compiler::format("canonical-sound", &result.source).unwrap();
        assert_eq!(
            scenario::compile("canonical-sound", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn sound_argument_aliases_do_not_reorder_or_escape_their_native_signature() {
    for body in [
        "native::play_sound_simple(sound_id: 30, volume: 0);",
        "native::play_sound(sound_id: 30, volume: 0, pan_offset: 90, binding_index: 0);",
        "native::play_sound(sound_id: 30, raw_velocity: 0, note: 90, channel: 0);",
        "native::set_actor_ambient_sound(actor_id: 400, sound_id: 30, channel: 127, radius: 800);",
        "native::play_sound(sound_id: 30, raw_velocity: 0, channel: 90, note: 0, note: 1);",
    ] {
        let error = scenario::compile("bad-sound-label", &source(body), &definitions())
            .unwrap_err()
            .to_string();
        assert!(error.contains("unexpected parameter"), "{body}\n{error}");
    }
}

#[test]
fn location_caption_overlay_key_is_named_only_at_its_field_constructor() {
    let definitions = definitions();
    let text = source(
        "native::create_overlay(999989, -1179648, 320, 240, -1, -1, 0, 255, 255, 255, 255, 1, (25 * 8 + 15)); native::create_overlay((1000000 - 11), -1179647, 320, 240, -1, -1, 0, 255, 255, 255, 255, 1, 240); native::create_overlay(123, -1179648, 320, 240, -1, -1, 0, 255, 255, 255, 255, 1, 999989); native::despawn_actor(999989); eval 999989;",
    );
    let bytes = scenario::compile("caption-overlay", &text, &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(29),
        scenario::Context::Battle,
        scenario::Context::World,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let named = matches!(
            context,
            scenario::Context::Field | scenario::Context::FieldScene(_)
        );
        assert_eq!(
            result
                .source
                .matches("scenario::OverlayActorId::LocationCaption")
                .count(),
            if named { 2 } else { 0 }
        );
        if named {
            assert!(
                result
                    .source
                    .contains("encoded(scenario::OverlayActorId::LocationCaption, (1000000 - 11))")
            );
        }
        for raw in [
            "resource: -1179648",
            "actor_id: 123",
            "depth_or_caption_hold: 999989",
            "eval 999989;",
        ] {
            assert!(result.source.contains(raw), "{raw}\n{}", result.source);
        }
        let formatted =
            symphonia_script_compiler::format("caption-overlay", &result.source).unwrap();
        assert_eq!(
            scenario::compile("caption-overlay", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn location_caption_overlay_uses_optional_selected_definitions() {
    let bytes = scenario::compile("caption-overlay", &source("native::create_overlay(999989, -1179648, 320, 240, -1, -1, 0, 255, 255, 255, 255, 1, 215);"), &definitions()).unwrap();
    for mode in ["renamed", "moved", "missing"] {
        let mut modules = resonance_script_content::modules();
        let library = modules.get_mut("std::scenario").unwrap();
        let start = library.find("pub enum OverlayActorId ").unwrap();
        let end = start + library[start..].find('}').unwrap() + 1;
        let block = library[start..end].to_owned();
        library.replace_range(
            start..end,
            &match mode {
                "missing" => String::new(),
                "moved" => block.replace("LocationCaption = 999989", "CustomCaption = 999988"),
                _ => block.replace("LocationCaption = 999989", "CustomCaption = 999989"),
            },
        );
        let selected = Definitions::from_sources(&modules).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &selected, scenario::Context::Field).unwrap();
        assert_eq!(
            result
                .source
                .contains("scenario::OverlayActorId::CustomCaption"),
            mode == "renamed"
        );
        if mode != "renamed" {
            assert!(result.source.contains("actor_id: 999989"));
        }
        assert_eq!(
            scenario::compile("caption-overlay", &result.source, &selected).unwrap(),
            bytes
        );
    }
}

#[test]
fn fanfare_machinery_and_alarm_cues_keep_scalar_and_program_namespaces_separate() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "fanfare-machinery-alarm",
        &source(
            "native::play_sound_simple(s32(80), 0);
        native::play_sound(u16(248), 0, 100, 1);
        native::play_sound_simple((300 - 1), 0);
        native::set_actor_ambient_sound(101, 248, 100, 1200);
        native::configure_actor_animation(710, 0, 80, 0, 8);
        native::configure_camera_auxiliary(1, 0, 0, 80, 248, 299);
        native::play_sound_simple(516, 0); native::play_sound_simple(6, 0);
        native::play_sound_simple(32828, 0); native::play_sound_simple(32851, 0);
        eval 80; eval 248; eval 299;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::Battle] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(effects::EffectId::SuccessFanfare)",
            "u16(effects::EffectId::MachineryMotionVariant0248)",
            "encoded(effects::EffectId::AlarmSignal, (300 - 1))",
            "resource_index: 80",
            "red: 80",
            "green: 248",
            "blue: 299",
            "sound_id: 516",
            "sound_id: effects::EffectId::ReceivedItem",
            "sound_id: 32828",
            "sound_id: 32851",
            "eval 80;",
            "eval 248;",
            "eval 299;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        assert_eq!(
            result
                .source
                .matches("effects::EffectId::MachineryMotionVariant0248")
                .count(),
            if context == scenario::Context::Battle {
                1
            } else {
                2
            }
        );
        for name in ["SuccessFanfare", "AlarmSignal"] {
            assert_eq!(
                result
                    .source
                    .matches(&format!("effects::EffectId::{name}"))
                    .count(),
                1
            );
        }
        let formatted =
            symphonia_script_compiler::format("fanfare-machinery-alarm", &result.source).unwrap();
        assert_eq!(
            scenario::compile("fanfare-machinery-alarm", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn sound_cue_names_follow_selected_definitions_and_keep_missing_ids_numeric() {
    // All these catalogues use the same rename/remove contract. Keep distinct
    // host, scalar, width and program-ID cases in the behavioral tests below.
    let cases: &[(&str, &str, &[(&str, i32)], &[i32])] = &[
        (
            "fanfare-machinery-alarm",
            "native::play_sound_simple(80, 0); native::play_sound_simple(248, 0); native::play_sound_simple(299, 0);",
            &[
                ("SuccessFanfare", 80),
                ("MachineryMotionVariant0248", 248),
                ("AlarmSignal", 299),
            ],
            &[],
        ),
        (
            "signal-chef",
            "native::play_sound_simple(246, 0); native::play_sound_simple(373, 0);",
            &[("MechanismSignal", 246), ("WonderChefPoof", 373)],
            &[],
        ),
        (
            "impact-weapon",
            "native::play_sound_simple(71, 0); native::play_sound(195, 0, 100, 255);",
            &[("BodyImpact", 71), ("WeaponReady", 195)],
            &[],
        ),
        (
            "machinery-drive",
            "native::play_sound(405, 0, 100, 1); native::set_actor_ambient_sound(101, 405, 100, 1200);",
            &[("MachineryDrive", 405)],
            &[],
        ),
        (
            "rumble-waterfall",
            "native::play_sound(181, 0, 90, 0); native::set_actor_ambient_sound(200, 394, 100, 5000);",
            &[("LowRumble", 181), ("WaterfallAmbience", 394)],
            &[],
        ),
        (
            "ambience",
            "native::play_sound_simple(384, 0); native::play_sound(385, 0, 70, 4);
             native::set_actor_ambient_sound(400, 386, 100, 800);
             native::play_sound_simple(62, 0); native::play_sound_simple(32935, 0);",
            &[
                ("FireAmbience", 384),
                ("SurfAmbience", 385),
                ("FlowingWaterAmbience", 386),
            ],
            &[62, 32935],
        ),
        (
            "mechanism",
            "native::play_sound_simple(189, 0); native::play_sound_simple(239, 0);
             native::play_sound_simple(306, 0); native::play_sound_simple(263, 0);
             native::play_sound_simple(320, 0); native::play_sound_simple(156, 0);
             native::play_sound_simple(187, 0); native::play_sound_simple(210, 0);
             native::play_sound_simple(366, 0); native::play_sound_simple(367, 0);
             native::play_sound_simple(282, 0); native::play_sound_simple(283, 0);
             native::play_sound_simple(275, 0);",
            &[
                ("ElectronicBeep", 189),
                ("MechanicalSlide", 239),
                ("ElevatorDepartureBeep", 306),
                ("RingModeChange", 320),
                ("DoorKnock", 156),
                ("ElectricalDischarge", 187),
                ("StoneFloorCollapse", 210),
                ("TeleportPadActivation", 366),
                ("TeleportDissolve", 367),
                ("JumpTakeoff", 282),
                ("LandingImpact", 283),
                ("MagicalFade", 275),
            ],
            &[],
        ),
    ];
    let definitions = definitions();
    for &(case, body, cues, numeric) in cases {
        let bytes = scenario::compile(case, &source(body), &definitions).unwrap();
        for remove in [false, true] {
            let mut modules = resonance_script_content::modules();
            let effects = modules.get_mut("std::effects").unwrap();
            for (name, value) in cues {
                let old = format!("{name} = {value},");
                assert!(
                    effects.contains(&old),
                    "{case}: missing test declaration {name}"
                );
                *effects = effects.replace(
                    &old,
                    &if remove {
                        String::new()
                    } else {
                        format!("Mod{name} = {value},")
                    },
                );
            }
            let selected = Definitions::from_sources(&modules).unwrap();
            let result =
                scenario::decompile_with_context(&bytes, &selected, scenario::Context::Field)
                    .unwrap();
            for (name, value) in cues {
                assert_eq!(
                    result
                        .source
                        .contains(&format!("effects::EffectId::Mod{name}")),
                    !remove,
                    "{case}: {name}"
                );
                let identities: Vec<_> = result
                    .identities
                    .iter()
                    .filter(|id| id.domain == "effects::EffectId" && id.value == *value)
                    .collect();
                assert!(!identities.is_empty(), "{case}: missing cue {value}");
                assert!(
                    identities.iter().all(|id| id.symbolic != remove),
                    "{case}: cue {value}"
                );
                if remove {
                    assert!(
                        result.source.contains(&format!("sound_id: {value}")),
                        "{case}: {value}"
                    );
                }
            }
            for value in numeric {
                assert!(
                    result.source.contains(&format!("sound_id: {value}")),
                    "{case}: scalar {value}"
                );
            }
            if case == "mechanism" {
                assert!(result.source.contains("effects::EffectId::RingRadar"));
            }
            assert_eq!(
                scenario::compile(case, &result.source, &selected).unwrap(),
                bytes,
                "{case}"
            );
        }
    }
}

#[test]
fn signal_and_chef_cues_preserve_widths_expressions_and_independent_operations() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "signal-chef",
        &source(
            "native::play_sound(s32(246), 0, 100, 255);
        native::play_sound_simple(u16(373), 0);
        native::set_actor_ambient_sound(101, (240 + 6), 100, 1200);
        native::play_actor_sound(101, (370 + 3), 100, 1200);
        native::move_actor(101, 246, 373, 0, 60);
        native::play_sound_simple(597, 0); native::play_sound_simple(263, 0);
        native::play_sound_simple(32805, 0); native::play_sound_simple(289, 0);
        eval 246; eval 373;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::Battle] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(effects::EffectId::MechanismSignal)",
            "u16(effects::EffectId::WonderChefPoof)",
            "x: 246",
            "y: 373",
            "sound_id: 597",
            "sound_id: effects::EffectId::RingRadar",
            "sound_id: 32805",
            "eval 246;",
            "eval 373;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        for name in ["MechanismSignal", "WonderChefPoof"] {
            assert_eq!(
                result
                    .source
                    .matches(&format!("effects::EffectId::{name}"))
                    .count(),
                if context == scenario::Context::Battle {
                    1
                } else {
                    2
                }
            );
        }
        let formatted = symphonia_script_compiler::format("signal-chef", &result.source).unwrap();
        assert_eq!(
            scenario::compile("signal-chef", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn impact_and_weapon_cues_keep_encoding_and_do_not_type_motion_or_sample_numbers() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "impact-weapon",
        &source(
            "native::play_sound_simple(s32(71), 0);
        native::play_sound(u16(195), 0, 100, 255);
        native::set_actor_ambient_sound(101, (70 + 1), 100, 1200);
        native::play_actor_sound(101, (190 + 5), 100, 1200);
        native::move_actor(101, 71, 195, 0, 60);
        native::play_sound_simple(144, 0); native::play_sound_simple(759, 0);
        native::play_sound_simple(94, 0); native::play_sound_simple(356, 0);
        eval 71; eval 195;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::Battle] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(effects::EffectId::BodyImpact)",
            "u16(effects::EffectId::WeaponReady)",
            "x: 71",
            "y: 195",
            "sound_id: 144",
            "sound_id: 759",
            "sound_id: effects::EffectId::SilentPlaceholder0094",
            "eval 71;",
            "eval 195;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        for name in ["BodyImpact", "WeaponReady"] {
            assert_eq!(
                result
                    .source
                    .matches(&format!("effects::EffectId::{name}"))
                    .count(),
                if context == scenario::Context::Battle {
                    1
                } else {
                    2
                }
            );
        }
        let formatted = symphonia_script_compiler::format("impact-weapon", &result.source).unwrap();
        assert_eq!(
            scenario::compile("impact-weapon", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn machinery_drive_names_the_cue_without_replacing_motion_or_program_ids() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "machinery-drive",
        &source(
            "native::play_sound(s32(405), 0, 100, 1);
        native::set_actor_ambient_sound(101, u16(405), 100, 1200);
        native::play_actor_sound(101, (400 + 5), 100, 1200);
        native::set_actor_ambient_sound(101, -1, 100, 1200);
        native::move_actor(101, 405, 0, -150, 60);
        native::play_sound_simple(538, 0);
        native::play_sound_simple(220, 0);
        native::play_sound_simple(406, 0);
        eval 405;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [scenario::Context::Field, scenario::Context::Battle] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(effects::EffectId::MachineryDrive)",
            "x: 405",
            "sound_id: 538",
            "sound_id: 220",
            "sound_id: 406",
            "eval 405;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        assert_eq!(
            result
                .source
                .matches("effects::EffectId::MachineryDrive")
                .count(),
            if context == scenario::Context::Battle {
                1
            } else {
                3
            }
        );
        assert_eq!(
            scenario::compile("machinery-drive", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn rumble_and_waterfall_cues_keep_sound_hosts_and_separate_motion_operations() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "rumble-waterfall",
        &source(
            "native::play_sound(s32(181), 0, 90, 0);
        native::set_actor_ambient_sound(200, u16(181), 127, 800);
        native::set_actor_ambient_sound(201, (390 + 4), 100, 5000);
        native::play_sound(394, 0, 110, 1);
        native::set_actor_ambient_sound(200, -1, 0, 0);
        native::shake_camera(181, 394, 0);
        native::rumble_controller(0, 181, 394);
        native::play_sound_simple(639, 0); native::play_sound_simple(768, 0);
        native::play_sound_simple(295, 0); eval 394;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(324),
        scenario::Context::Battle,
        scenario::Context::World,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "s32(effects::EffectId::LowRumble)",
            "sound_id: effects::EffectId::WaterfallAmbience",
            "amplitude: 181",
            "decay_ticks: 394",
            "duration_ticks: 181",
            "brake_on_stop: 394",
            "volume: 90",
            "binding_index: 0",
            "volume: 110",
            "binding_index: 1",
            "sound_id: 639",
            "sound_id: 768",
            "sound_id: 295",
            "eval 394;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        let uses = result
            .identities
            .iter()
            .filter(|id| id.domain == "effects::EffectId" && matches!(id.value, 181 | 394))
            .collect::<Vec<_>>();
        assert_eq!(
            uses.len(),
            if context == scenario::Context::Battle {
                2
            } else {
                4
            }
        );
        assert!(uses.iter().all(|id| id.symbolic));
        if context != scenario::Context::Battle {
            assert!(result.source.contains("u16(effects::EffectId::LowRumble)"));
            assert!(
                result
                    .source
                    .contains("encoded(effects::EffectId::WaterfallAmbience, (390 + 4))")
            );
        }
        let formatted =
            symphonia_script_compiler::format("rumble-waterfall", &result.source).unwrap();
        assert_eq!(
            scenario::compile("rumble-waterfall", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn ambience_cues_preserve_actor_bindings_slots_widths_and_unrelated_scalars() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "ambience",
        &source(
            "native::play_sound_simple(u16(384), 0);
        native::play_sound(s32(385), 0, 70, 4);
        native::play_sound((380 + 6), 0, 50, 3);
        native::set_actor_ambient_sound(400, u16(384), 384, 386);
        native::set_actor_ambient_sound(401, s32(385), 127, 2500);
        native::set_actor_ambient_sound(402, (380 + 6), 90, 4000);
        native::play_actor_sound(403, 386, 100, 800);
        native::set_actor_ambient_sound(400, -1, 0, 0);
        native::play_sound_simple(62, 0); native::play_sound_simple(834, 0);
        native::play_sound_simple(32935, 0); eval 384;",
        ),
        &definitions,
    )
    .unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(11),
        scenario::Context::Battle,
        scenario::Context::Skit,
        scenario::Context::World,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "u16(effects::EffectId::FireAmbience)",
            "s32(effects::EffectId::SurfAmbience)",
            "encoded(effects::EffectId::FlowingWaterAmbience, (380 + 6))",
            "volume: 70",
            "binding_index: 4",
            "volume: 50",
            "binding_index: 3",
            "volume: 384",
            "radius: 386",
            "sound_id: 62",
            "sound_id: 834",
            "sound_id: 32935",
            "eval 384;",
        ] {
            assert!(
                result.source.contains(expected),
                "{expected}\n{}",
                result.source
            );
        }
        let names = result
            .identities
            .iter()
            .filter(|id| id.domain == "effects::EffectId" && matches!(id.value, 384..=386))
            .collect::<Vec<_>>();
        assert_eq!(
            names.len(),
            if context == scenario::Context::Battle {
                3
            } else {
                7
            }
        );
        assert!(names.iter().all(|id| id.symbolic));
        if context == scenario::Context::Battle {
            assert!(
                names
                    .iter()
                    .all(|id| matches!(id.native.as_str(), "play_sound" | "play_sound_simple"))
            );
        }
        let formatted = symphonia_script_compiler::format("ambience", &result.source).unwrap();
        assert_eq!(
            scenario::compile("ambience", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn mechanism_cue_names_keep_distinct_cue_program_and_sample_namespaces() {
    let definitions = definitions();
    let text = source(
        "native::play_sound_simple(u16(189), 0);
        native::play_sound(s32(239), 0, 255, 255);
        native::play_sound_simple((300 + 6), 0);
        native::play_sound_simple(33, 0);
        native::play_sound_simple(130, 0);
        native::play_sound_simple(263, 0);
        native::play_sound_simple(u16(320), 0);
        native::play_sound(s32(320), 0, 255, 255);
        native::play_sound_simple((300 + 20), 0);
        native::play_sound_simple(32929, 0);
        native::play_sound_simple(740, 0);
        native::play_sound_simple(28, 0);
        native::play_sound_simple(u16(156), 0);
        native::play_sound(s32(187), 0, 255, 255);
        native::play_sound_simple((200 + 10), 0);
        native::play_sound_simple(u16(366), 0);
        native::play_sound(s32(367), 0, 255, 255);
        native::play_sound_simple(u16(282), 0);
        native::play_sound(s32(283), 0, 255, 255);
        native::play_sound_simple((280 + 2), 0);
        native::play_sound_simple(460, 0);
        native::play_sound_simple(32900, 0);
        native::play_sound_simple(32901, 0);
        native::play_sound_simple(u16(275), 0);
        native::play_sound(s32(275), 0, 90, 255);
        native::play_sound_simple((270 + 5), 0);
        native::play_sound_simple(811, 0);
        native::play_sound_simple(32932, 0); eval 275;
        native::play_sound_simple(9999, 0); eval 189;",
    );
    let bytes = scenario::compile("mechanism-cues", &text, &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::Battle,
        scenario::Context::Skit,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "sound_id: u16(effects::EffectId::ElectronicBeep)",
            "sound_id: s32(effects::EffectId::MechanicalSlide)",
            "encoded(effects::EffectId::ElevatorDepartureBeep, (300 + 6))",
            "sound_id: effects::EffectId::MenuOpen",
            "sound_id: effects::EffectId::BattleEntry",
            "sound_id: effects::EffectId::RingRadar",
            "sound_id: u16(effects::EffectId::RingModeChange)",
            "sound_id: s32(effects::EffectId::RingModeChange)",
            "encoded(effects::EffectId::RingModeChange, (300 + 20))",
            "sound_id: 32929",
            "sound_id: 740",
            "sound_id: effects::EffectId::OpenChest",
            "sound_id: u16(effects::EffectId::DoorKnock)",
            "sound_id: s32(effects::EffectId::ElectricalDischarge)",
            "encoded(effects::EffectId::StoneFloorCollapse, (200 + 10))",
            "sound_id: u16(effects::EffectId::TeleportPadActivation)",
            "sound_id: s32(effects::EffectId::TeleportDissolve)",
            "sound_id: u16(effects::EffectId::JumpTakeoff)",
            "sound_id: s32(effects::EffectId::LandingImpact)",
            "encoded(effects::EffectId::JumpTakeoff, (280 + 2))",
            "sound_id: effects::EffectId::YmirBirdCall",
            "sound_id: 32900",
            "sound_id: 32901",
            "sound_id: u16(effects::EffectId::MagicalFade)",
            "sound_id: s32(effects::EffectId::MagicalFade)",
            "encoded(effects::EffectId::MagicalFade, (270 + 5))",
            "volume: 90",
            "sound_id: 811",
            "sound_id: 32932",
            "eval 275;",
            "sound_id: 9999",
            "eval 189;",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "effects::EffectId" && id.symbolic)
                .count(),
            22
        );
        let formatted =
            symphonia_script_compiler::format("mechanism-cues", &result.source).unwrap();
        assert_eq!(
            scenario::compile("mechanism-cues", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn reward_fanfare_names_keep_cue_ids_widths_and_reused_notice_contexts() {
    let definitions = definitions();
    let text = source(
        "native::play_sound_simple(u16(36), 0);
        native::play_sound(s32(37), 0, 255, 255);
        native::learn_title(characters::TitleId::LloydEternalSwordsman);
        native::play_sound_simple((35 + 1), 0);
        native::play_sound_simple(9999, 0);
        native::play_sound_simple(459, 0); eval 36;",
    );
    let bytes = scenario::compile("reward-cues", &text, &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::Battle,
        scenario::Context::Skit,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "sound_id: u16(effects::EffectId::AcquisitionFanfare)",
            "sound_id: s32(effects::EffectId::LearningFanfare)",
            "encoded(effects::EffectId::AcquisitionFanfare, (35 + 1))",
            "sound_id: 9999",
            "sound_id: effects::EffectId::YmirBoarImpact",
            "eval 36;",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "effects::EffectId"
                    && id.symbolic
                    && matches!(id.value, 36 | 37))
                .count(),
            3
        );
        let formatted = symphonia_script_compiler::format("reward-cues", &result.source).unwrap();
        assert_eq!(
            scenario::compile("reward-cues", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn fanfare_cue_names_follow_actual_definitions_without_substituting_program_ids() {
    let definitions = definitions();
    let text = source(
        "native::play_sound_simple(36, 0); native::play_sound_simple(37, 0);
        native::play_sound_simple(459, 0); native::play_sound_simple(711, 0);",
    );
    let bytes = scenario::compile("cue-names", &text, &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::effects").unwrap();
    *module = module
        .replace("AcquisitionFanfare = 36,", "ModAcquisition = 36,")
        .replace("LearningFanfare = 37,", "ModLearning = 37,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert!(result.source.contains("EffectId::ModAcquisition"));
    assert!(result.source.contains("EffectId::ModLearning"));
    assert!(
        result
            .source
            .contains("sound_id: effects::EffectId::YmirBoarImpact")
    );
    assert!(result.source.contains("sound_id: 711"));
    assert_eq!(
        scenario::compile("cue-names", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::effects").unwrap();
    *module = module
        .replace("ModAcquisition = 36,", "")
        .replace("YmirBoarImpact = 459,", "")
        .replace("ModLearning = 37,", "ModLearning = 459,");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert!(result.source.contains("sound_id: 36"));
    assert!(result.source.contains("sound_id: 37"));
    // A mod can explicitly declare a cue at 459; the decompiler follows that
    // declaration without confusing the native macro's number with cue36.
    assert!(
        result
            .source
            .contains("sound_id: effects::EffectId::ModLearning")
    );
    assert_eq!(
        scenario::compile("cue-names", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn animation_flags_name_default_playback_and_distinguish_resume_from_empty_flags() {
    let definitions = definitions();
    let body = "native::configure_actor_animation(1, -1, 12, 1, u16(1));
        native::configure_actor_animation(1, -1, 12, 1, 0);
        native::set_actor_animation_flags(1, s32(1));
        native::set_actor_animation_flags(1, s16(0));
        native::set_actor_animation_flags(1, 2);
        native::configure_actor_animation(1, -1, 12, 1, (0 + 1));
        native::set_actor_animation_flags(1, 9);
        native::set_actor_animation_flags(1, 65536);
        native::set_actor_animation_flags(1, -1);
        native::configure_actor_animation(1, -1, 12, 1, 20); eval 1;";
    let bytes = scenario::compile("animation-flags", &source(body), &definitions).unwrap();
    for context in [scenario::Context::Field, scenario::Context::Battle] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for expected in [
            "flags: u16(scenario::AnimationFlag::Default)",
            "flags: scenario::AnimationFlag::None",
            "flags: s32(scenario::AnimationFlag::Default)",
            "flags: s16(scenario::AnimationControl::Resume)",
            "flags: scenario::AnimationFlag::Paused",
            "encoded(scenario::AnimationFlag::Default, (0 + 1))",
            "flags(scenario::AnimationFlag::Default, scenario::AnimationFlag::Once)",
            "flags: 65536",
            "flags: -1",
            "flags: 20",
            "eval 1;",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "scenario::AnimationControl")
                .count(),
            1
        );
        let formatted =
            symphonia_script_compiler::format("animation-flags", &result.source).unwrap();
        assert_eq!(
            scenario::compile("animation-flags", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn animation_playback_and_resume_names_use_supplied_declarations() {
    let definitions = definitions();
    let body = "native::configure_actor_animation(1, -1, 12, 1, 1);
        native::set_actor_animation_flags(1, 1);
        native::set_actor_animation_flags(1, 0);";
    let bytes = scenario::compile("animation-flag-names", &source(body), &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    assert_eq!(module.matches("Default = 1,").count(), 1);
    let resume = "pub enum AnimationControl {\n    Resume = 0,\n}";
    assert!(module.contains(resume));
    *module = module
        .replace("Default = 1,", "ModDefault = 1,")
        .replace(resume, "pub enum AnimationControl {\n    ModResume = 0,\n}");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    assert_eq!(
        result.source.matches("AnimationFlag::ModDefault").count(),
        2
    );
    assert!(result.source.contains("AnimationControl::ModResume"));
    assert_eq!(
        scenario::compile("animation-flag-names", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module
        .replace("ModDefault = 1,", "")
        .replace("ModResume = 0,", "ModResume = 3,");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &missing).unwrap();
    assert_eq!(result.source.matches("flags: 1").count(), 2);
    assert!(result.source.contains("flags: 0"));
    assert!(!result.source.contains("AnimationFlag::None"));
    assert_eq!(
        scenario::compile("animation-flag-names", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn automatic_animation_keeps_ignored_flags_numeric_and_preserves_their_encodings() {
    let definitions = definitions();
    let mut body = String::new();
    for flags in [
        "u16(8)", "s32(60)", "s16(16)", "20", "(4 + 16)", "1", "0", "-1",
    ] {
        body.push_str(&format!(
            "native::configure_actor_animation(1, 0, 0, 0, {flags});"
        ));
    }
    body.push_str(
        "native::configure_actor_animation(1, 0, 0, 0, (memory::i32[0x0814] += 1));
        native::configure_actor_animation(1, -1, 12, 0, 8);
        native::configure_actor_animation(1, memory::i32[0x0810], 12, 0, 8);",
    );
    let bytes = scenario::compile("automatic-flags", &source(&body), &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(13),
        scenario::Context::World,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "scenario::AnimationFlag")
                .count(),
            2
        );
        assert_eq!(result.source.matches("AnimationFlag::Once").count(), 2);
        for expected in [
            "flags: u16(8)",
            "flags: s32(60)",
            "flags: s16(16)",
            "flags: 20",
            "flags: (4 + 16)",
            "flags: 1",
            "flags: 0",
            "flags: -1",
            "flags: (memory::i32[0x0814] += 1)",
        ] {
            assert!(result.source.contains(expected), "{}", result.source);
        }
        assert_eq!(
            result.source.contains("AnimationSource::Automatic"),
            context != scenario::Context::Unspecified
        );
        let formatted =
            symphonia_script_compiler::format("automatic-flags", &result.source).unwrap();
        assert_eq!(
            scenario::compile("automatic-flags", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    // Battle resource0 resolves a resource directly. Its flags are consumed.
    let battle =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Battle).unwrap();
    assert_eq!(
        battle
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::AnimationFlag")
            .count(),
        10
    );
    assert!(
        battle
            .source
            .contains("flags: u16(scenario::AnimationFlag::Once)")
    );
    assert!(!battle.source.contains("AnimationSource::Automatic"));
    assert_eq!(
        scenario::compile("automatic-flags", &battle.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn supplied_flag_names_do_not_type_ignored_automatic_animation_operands() {
    let definitions = definitions();
    let body = "native::configure_actor_animation(1, 0, 0, 0, 8);
        native::configure_actor_animation(1, -1, 12, 0, 8);";
    let bytes = scenario::compile("automatic-flag-names", &source(body), &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("Once = 8,", "ModOnce = 8,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &custom, scenario::Context::Field).unwrap();
    assert_eq!(result.source.matches("AnimationFlag::ModOnce").count(), 1);
    assert!(result.source.contains("flags: 8"));
    assert_eq!(
        scenario::compile("automatic-flag-names", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("ModOnce = 8,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &missing, scenario::Context::Field).unwrap();
    assert!(!result.source.contains("AnimationFlag::"));
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::AnimationFlag" && !id.symbolic)
            .count(),
        1
    );
    assert_eq!(
        scenario::compile("automatic-flag-names", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn model_animation_slots_require_the_current_model_selector() {
    let definitions = definitions();
    let body = "native::configure_actor_animation(1, -1, u16(12), 0, 8);
        native::configure_actor_animation(1, -1, s16(0), 0, 8);
        native::configure_actor_animation(1, -1, (8 + 4), 0, 8);
        native::configure_actor_animation(1, 65536, u16(12), 0, 8);
        native::configure_actor_animation(1, memory::i32[0x0810], s32(12), 0, 8);
        native::configure_actor_animation(1, 0, s16(12), 0, 8);
        native::configure_actor_animation(1, -2, 12, 0, 8);";
    let bytes = scenario::compile("animation-selectors", &source(body), &definitions).unwrap();
    for (context, count) in [
        (scenario::Context::Field, 3),
        (scenario::Context::FieldScene(13), 3),
        (scenario::Context::Battle, 0),
        (scenario::Context::World, 0),
        (scenario::Context::Skit, 0),
        (scenario::Context::Unspecified, 0),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert_eq!(
            result
                .identities
                .iter()
                .filter(|id| id.domain == "scenario::AnimationSlot")
                .count(),
            count
        );
        assert_eq!(
            result.source.contains("AnimationSource::Automatic"),
            !matches!(
                context,
                scenario::Context::Battle | scenario::Context::Unspecified
            )
        );
        if count != 0 {
            for expected in [
                "resource_index: u16(scenario::AnimationSlot::Idle)",
                "resource_index: s16(scenario::AnimationSlot::Default)",
                "encoded(scenario::AnimationSlot::Idle, (8 + 4))",
                "resource_index: u16(12)",
                "resource_index: s32(12)",
                "resource_index: s16(12)",
                "resource_index: 12",
            ] {
                assert!(result.source.contains(expected), "{}", result.source);
            }
        }
        let formatted =
            symphonia_script_compiler::format("animation-selectors", &result.source).unwrap();
        assert_eq!(
            scenario::compile("animation-selectors", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn animation_slot_definitions_only_name_operands_in_the_model_table_branch() {
    let definitions = definitions();
    let body = "native::configure_actor_animation(1, -1, 12, 0, 8);
        native::configure_actor_animation(1, memory::i32[0x0810], 12, 0, 8);";
    let bytes = scenario::compile("animation-slot-names", &source(body), &definitions).unwrap();
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("Idle = 12,", "ModIdle = 12,");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &custom, scenario::Context::Field).unwrap();
    assert_eq!(result.source.matches("AnimationSlot::ModIdle").count(), 1);
    assert!(result.source.contains("resource_index: 12"));
    assert_eq!(
        scenario::compile("animation-slot-names", &result.source, &custom).unwrap(),
        bytes
    );
    let module = modules.get_mut("std::scenario").unwrap();
    *module = module.replace("ModIdle = 12,", "");
    let missing = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &missing, scenario::Context::Field).unwrap();
    assert!(!result.source.contains("AnimationSlot::"));
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::AnimationSlot" && !id.symbolic)
            .count(),
        1
    );
    assert_eq!(
        scenario::compile("animation-slot-names", &result.source, &missing).unwrap(),
        bytes
    );
}

#[test]
fn operand_context_distinguishes_resources_grid_coordinates_and_update_sentinels() {
    let definitions = definitions();
    let mut body = String::from(
        "native::configure_dialogue(0, 0, -2, 4, 0, 0, 0, 0);
        native::configure_dialogue(0, 0, 0, 4, 0, 0, 0, 0);
        native::configure_dialogue(0, 0, -1, 201, 0, 0, 0, 0);
        native::configure_actor_animation(1, 0, 0, 0, 0);
        native::configure_actor_animation(1, -1, 12, 0, 8);
        native::configure_actor_animation(1, -1, 0, 0, 8);
        eval native::resolve_script_resource(65536);
        eval native::resolve_script_resource(131072);
        native::audio_command(-3);
        eval native::set_equipped_title(1, -1);
        native::configure_ex_gem(1, 0, -1, -1);",
    );
    for attachment in -18..=-10 {
        body.push_str(&format!(
            "native::configure_dialogue(0, 0, {attachment}, 1, 0, 0, 0, 0);"
        ));
    }
    let bytes = scenario::compile("contexts", &source(&body), &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for name in [
        "scenario::DialogueGrid::Center",
        "scenario::AnimationSlot::Idle",
        "scenario::AnimationSlot::Default",
        "resources::FieldAnimationId::LloydMotionE073",
        "resources::FieldModelId::Yggdrasill",
        "scenario::MusicControl::Resume",
        "scenario::TitleSelection::Query",
        "scenario::ExGemUpdate::Keep",
    ] {
        assert!(
            result.source.contains(name),
            "missing {name}: {}",
            result.source
        );
    }
    assert!(!result.source.contains("characters::CharacterId::Raine"));
    assert!(!result.identities.iter().any(|id| id.value == 201));
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.native == "configure_dialogue"
                && id.parameter == "actor_id"
                && id.domain != "scenario::DialogueGrid")
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::AnimationSlot" && id.value == 0)
            .count(),
        1
    );
    assert_eq!(
        scenario::compile("contexts", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn missing_identity_definitions_are_an_error_instead_of_a_numeric_decompile() {
    let text = source("native::set_event_bit(104);");
    let bytes = scenario::compile("numeric", &text, &Definitions::default()).unwrap();
    let error = scenario::decompile(&bytes, &Definitions::default())
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::story::StoryFlag definitions"),
        "{error}"
    );
}

#[test]
fn a_known_global_requires_its_scoped_value_definitions() {
    let bytes = scenario::compile(
        "scoped",
        &source("eval (memory::i32[0x004c] = 1000);"),
        &definitions(),
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let story = sources.get_mut("std::story").unwrap();
    let start = story.find("pub enum ColetteAngelStage").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "");
    let partial = Definitions::from_sources(&sources).unwrap();
    let error = scenario::decompile(&bytes, &partial)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::story::ColetteAngelStage definitions"),
        "{error}"
    );
}

#[test]
fn world_transfers_name_landmarks_without_retyping_field_coordinates() {
    let definitions = definitions();
    let text = source(
        "native::change_field(3000, s32(2), 0, 0, 2);
        native::change_field((3000 + 1), (260 + 7), 0, 0, 2);
        native::change_field(5, 2, 0, 0, 2);
        native::change_field(memory::i32[0x0810], 2, 0, 0, 2);
        native::play_world_cinematic(514, 3000, 19, 0, 0, 2);
        native::play_world_cinematic(514, 5, 19, 0, 0, 2);
        native::play_world_cinematic(514, memory::i32[0x0810], 19, 0, 0, 2);",
    );
    let bytes = scenario::compile("world-landmarks", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        result
            .source
            .contains("scenario::WorldTransferId::Standard")
    );
    assert!(
        result
            .source
            .contains("encoded(scenario::WorldTransferId::Alternate, (3000 + 1))")
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::WorldTransferId")
            .map(|id| (id.native.as_str(), id.value, id.symbolic))
            .collect::<Vec<_>>(),
        [
            ("change_field", 3000, true),
            ("change_field", 3001, true),
            ("play_world_cinematic", 3000, true)
        ]
    );
    assert!(
        result
            .source
            .contains("s32(locations::WorldLocationId::Iselia)")
    );
    assert!(
        result
            .source
            .contains("encoded(locations::WorldLocationId::FoojiMountains, (260 + 7))")
    );
    assert!(
        result
            .source
            .contains("locations::WorldLocationId::ThodaGeyser")
    );
    let landmarks: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.domain == "locations::WorldLocationId")
        .map(|id| {
            (
                id.native.as_str(),
                id.parameter.as_str(),
                id.value,
                id.symbolic,
            )
        })
        .collect();
    assert_eq!(
        landmarks,
        [
            ("change_field", "x", 2, true),
            ("change_field", "x", 267, true),
            ("play_world_cinematic", "x_or_landmark", 19, true),
        ]
    );
    let formatted = symphonia_script_compiler::format("world-landmarks", &result.source).unwrap();
    assert_eq!(
        scenario::compile("world-landmarks", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn world_transfer_names_follow_supplied_definitions_and_keep_unknown_encodings_numeric() {
    let bytes = scenario::compile(
        "world-transfer",
        &source(
            "native::change_field(s32(3000), 2, 0, 0, 2);
        native::change_field(3002, 0, 0, 0, 0);
        eval 3000;",
        ),
        &definitions(),
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let scenario = sources.get_mut("std::scenario").unwrap();
    assert!(scenario.contains("Standard = 3000"));
    *scenario = scenario.replace("Standard = 3000", "StandardForMod = 3000");
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &renamed).unwrap();
    assert!(
        result
            .source
            .contains("s32(scenario::WorldTransferId::StandardForMod)")
    );
    assert!(result.source.contains("eval 3000;"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "scenario::WorldTransferId" && id.value == 3002 && !id.symbolic)
    );
    assert_eq!(
        scenario::compile("world-transfer", &result.source, &renamed).unwrap(),
        bytes
    );

    let scenario = sources.get_mut("std::scenario").unwrap();
    *scenario = scenario.replace("StandardForMod = 3000", "StandardForMod = 3003");
    let moved = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &moved).unwrap();
    assert!(!result.source.contains("WorldTransferId::Standard"));
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "scenario::WorldTransferId" && id.value == 3000 && !id.symbolic)
    );
    assert_eq!(
        scenario::compile("world-transfer", &result.source, &moved).unwrap(),
        bytes
    );

    let scenario = sources.get_mut("std::scenario").unwrap();
    let start = scenario.find("pub enum WorldTransferId").unwrap();
    let end = start + scenario[start..].find('}').unwrap() + 1;
    scenario.replace_range(start..end, "");
    let partial = Definitions::from_sources(&sources).unwrap();
    let error = scenario::decompile(&bytes, &partial)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::scenario::WorldTransferId definitions"),
        "{error}"
    );
}

#[test]
fn shared_overlay_resources_use_image_names_without_retyping_animation_sources() {
    let bytes = scenario::compile(
        "overlays",
        &source(
            "eval native::resolve_script_resource(s32(68611));
        eval native::resolve_script_resource((68600 + 14));
        native::configure_actor_animation(1, 68611, 12, 10, 0);
        eval native::resolve_script_resource(68612);
        eval 68611;",
        ),
        &definitions(),
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let resources = sources.get_mut("std::resources").unwrap();
    assert!(resources.contains("RadialBurst = 68611"));
    *resources = resources.replace("RadialBurst = 68611", "RadialBurstForMod = 68611");
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &renamed, scenario::Context::Field).unwrap();
    assert!(
        result
            .source
            .contains("packed_resource: resources::FieldOverlayId::RadialBurstForMod")
    );
    assert!(
        result
            .source
            .contains("encoded(resources::FieldOverlayId::OrnateRing, (68600 + 14))")
    );
    assert!(result.source.contains("resource_id: 68611"));
    assert!(result.source.contains("eval 68611;"));
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "resources::FieldOverlayId")
            .count(),
        2
    );
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "resources::FieldModelId"
                && id.native == "configure_actor_animation"
                && id.value == 68611
                && !id.symbolic)
    );
    assert_eq!(
        scenario::compile("overlays", &result.source, &renamed).unwrap(),
        bytes
    );

    let resources = sources.get_mut("std::resources").unwrap();
    *resources = resources.replace("RadialBurstForMod = 68611", "RadialBurstForMod = 68612");
    let moved = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &moved, scenario::Context::Field).unwrap();
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "resources::FieldOverlayId"
                && id.value == 68611
                && !id.symbolic)
    );
    assert!(
        result
            .source
            .contains("packed_resource: resources::FieldOverlayId::RadialBurstForMod")
    );
    assert_eq!(
        scenario::compile("overlays", &result.source, &moved).unwrap(),
        bytes
    );

    let resources = sources.get_mut("std::resources").unwrap();
    let start = resources.find("pub enum FieldOverlayId").unwrap();
    let end = start + resources[start..].find('}').unwrap() + 1;
    resources.replace_range(start..end, "");
    let partial = Definitions::from_sources(&sources).unwrap();
    let error = scenario::decompile_with_context(&bytes, &partial, scenario::Context::Field)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("missing std::resources::FieldOverlayId definitions"),
        "{error}"
    );
}

#[test]
fn actor_audio_names_keep_emitter_controls_cues_volumes_and_widths_separate() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "actor-audio",
        &source(
            "native::set_actor_ambient_sound(999999, u16(289), s32(255), 2000);
        native::set_actor_ambient_sound(4, s32(-1), 255, 289);
        native::set_actor_ambient_sound(100, (0 - 1), 255, 289);
        native::set_actor_ambient_sound(100, 65535, 255, 289);
        native::set_actor_ambient_sound(100, 65537, 255, 289);
        native::set_actor_ambient_sound(100, (280 + 9), 300, 289);
        native::play_actor_sound(999999, 289, 255, 289);
        native::play_actor_sound(100, -1, 255, 289);
        native::play_actor_sound(100, 65535, 255, 289);
        eval 289; eval 255;",
        ),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for expected in [
        "actor_id: scenario::ActorSelector::Controlled",
        "actor_id: characters::CharacterId::Raine",
        "sound_id: u16(effects::EffectId::RingBubble)",
        "sound_id: s32(scenario::AmbientSoundControl::Disable)",
        "encoded(scenario::AmbientSoundControl::Disable, (0 - 1))",
        "encoded(effects::EffectId::RingBubble, (280 + 9))",
        "volume: s32(scenario::SpatialSoundVolume::Default)",
        "sound_id: 65535",
        "sound_id: 65537",
        "sound_id: -1",
        "volume: 300",
        "radius: 289",
        "eval 289;",
        "eval 255;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    let count = |domain| {
        result
            .identities
            .iter()
            .filter(|id| id.domain == domain)
            .count()
    };
    assert_eq!(count("scenario::AmbientSoundControl"), 2);
    assert_eq!(count("scenario::SpatialSoundVolume"), 6);
    assert_eq!(count("effects::EffectId"), 7);
    assert_eq!(count("scenario::ActorSelector"), 2);
    assert_eq!(count("characters::CharacterId"), 1);
    assert!(result.identities.iter().all(|id| {
        id.domain != "scenario::AmbientSoundControl" || id.native == "set_actor_ambient_sound"
    }));
    let formatted = symphonia_script_compiler::format("actor-audio", &result.source).unwrap();
    assert_eq!(
        scenario::compile("actor-audio", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn actor_audio_definitions_are_editable_and_have_no_builtin_fallbacks() {
    let bytes = scenario::compile(
        "actor-audio",
        &source(
            "native::set_actor_ambient_sound(100, 289, 255, 2000);
        native::set_actor_ambient_sound(100, -1, 255, 2000);",
        ),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let scenario_module = modules.get_mut("std::scenario").unwrap();
    *scenario_module = scenario_module
        .replace("Disable = -1", "DisableForMod = -1")
        .replace("Default = 255", "DefaultForMod = 255");
    let effects = modules.get_mut("std::effects").unwrap();
    *effects = effects.replace("RingBubble = 289", "BubbleForMod = 289");
    let custom = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &custom).unwrap();
    for symbol in [
        "AmbientSoundControl::DisableForMod",
        "SpatialSoundVolume::DefaultForMod",
        "EffectId::BubbleForMod",
    ] {
        assert!(result.source.contains(symbol), "{}", result.source);
    }
    assert_eq!(
        scenario::compile("actor-audio", &result.source, &custom).unwrap(),
        bytes
    );
    let scenario_module = modules.get_mut("std::scenario").unwrap();
    *scenario_module = scenario_module
        .replace("DisableForMod = -1", "DisableForMod = -2")
        .replace("DefaultForMod = 255", "DefaultForMod = 254");
    let effects = modules.get_mut("std::effects").unwrap();
    *effects = effects.replace("BubbleForMod = 289", "BubbleForMod = 290");
    let moved = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &moved).unwrap();
    assert!(result.identities.iter().all(|id| !id.symbolic));
    assert_eq!(
        scenario::compile("actor-audio", &result.source, &moved).unwrap(),
        bytes
    );
    for (module, domain) in [
        ("scenario", "AmbientSoundControl"),
        ("scenario", "SpatialSoundVolume"),
        ("effects", "EffectId"),
    ] {
        let mut partial = modules.clone();
        let source = partial.get_mut(&format!("std::{module}")).unwrap();
        let start = source.find(&format!("pub enum {domain}")).unwrap();
        let end = start + source[start..].find('}').unwrap() + 1;
        source.replace_range(start..end, "");
        let partial = Definitions::from_sources(&partial).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::{module}::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn actor_audio_in_battle_keeps_ignored_arguments_numeric_without_requiring_domains() {
    let bytes = scenario::compile(
        "actor-audio",
        &source(
            "native::set_actor_ambient_sound(999999, 289, 255, 2000);
        native::set_actor_ambient_sound(4, -1, 255, 289);
        native::play_actor_sound(999999, 289, 255, 2000);",
        ),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    modules.remove("std::effects");
    let source = modules.get_mut("std::scenario").unwrap();
    for domain in ["AmbientSoundControl", "SpatialSoundVolume"] {
        let start = source.find(&format!("pub enum {domain}")).unwrap();
        let end = start + source[start..].find('}').unwrap() + 1;
        source.replace_range(start..end, "");
    }
    let partial = Definitions::from_sources(&modules).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &partial, scenario::Context::Battle).unwrap();
    assert!(result.identities.is_empty());
    assert!(result.source.contains("sound_id: -1"));
    assert!(result.source.contains("volume: 255"));
    assert_eq!(
        scenario::compile("actor-audio", &result.source, &partial).unwrap(),
        bytes
    );
}

#[test]
fn sound_default_selectors_follow_consuming_branches_and_preserve_encoding() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "sound-defaults",
        &source(
            "native::play_sound(30, s16(255), u16(255), s32(255));
        native::play_sound(-1, 0, 255, 0);
        native::play_sound(-1, 0, 255, 255);
        native::play_sound(memory::i32[0x0810], 0, 255, 4);
        native::play_sound(30, 0, (250 + 5), (250 + 5));
        native::play_sound(30, 0, -1, 65535);
        native::play_sound(30, 0, 511, 511);
        native::play_sound(30, 0, 200, 4);
        native::play_sound(30, 0, -80, 255);
        native::play_sound(30, 0, 255, memory::i32[0x0810]);
        native::play_sound(memory::i32[0x0810], 0, 255, 255);
        native::play_sound_simple(30, 255);
        native::configure_sound(255, 100, s16(255));
        native::configure_sound(0, 101, 255);
        native::configure_sound(0, 102, 255);
        native::configure_sound(0, 255, 255);
        native::configure_sound(0, memory::i32[0x0810], 255);
        eval 255;",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    let compact = result
        .source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "pan_offset: 255",
        "volume: u16(scenario::SoundVolume::FieldDefault)",
        "binding_index: s32(scenario::SoundBinding::Unbound)",
        "encoded(scenario::SoundVolume::FieldDefault, (250 + 5))",
        "encoded(scenario::SoundBinding::Unbound, (250 + 5))",
        "volume: -1, binding_index: 65535",
        "volume: 511, binding_index: 511",
        "volume: 200, binding_index: 4",
        "volume: -80, binding_index: scenario::SoundBinding::Unbound",
        "binding_index: 255, operation: scenario::SoundSlotOperation::Volume, value: scenario::SoundVolume::FieldDefault",
        "operation: scenario::SoundSlotOperation::Pan, value: 255",
        "operation: scenario::SoundSlotOperation::Stop, value: 255",
        "operation: 255, value: 255",
        "eval 255;",
    ] {
        assert!(compact.contains(expected), "{expected}\n{}", result.source);
    }
    for (domain, count) in [("scenario::SoundVolume", 6), ("scenario::SoundBinding", 5)] {
        let identities: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.domain == domain)
            .collect();
        assert_eq!(identities.len(), count, "{domain}: {}", result.source);
        assert!(identities.iter().all(|id| id.value == 255 && id.symbolic));
    }
    let formatted = symphonia_script_compiler::format("sound-defaults", &result.source).unwrap();
    assert_eq!(
        scenario::compile("sound-defaults", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn sound_default_selectors_use_optional_selected_definitions() {
    let bytes = scenario::compile(
        "sound-defaults",
        &source("native::play_sound(30, 0, 255, 255); native::configure_sound(0, 100, 255);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let library = modules.get_mut("std::scenario").unwrap();
    *library = library
        .replace("FieldDefault = 255", "LevelForMod = 255")
        .replace("Unbound = 255", "TransientForMod = 255");
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert_eq!(
        result
            .source
            .matches("scenario::SoundVolume::LevelForMod")
            .count(),
        2
    );
    assert_eq!(
        result
            .source
            .matches("scenario::SoundBinding::TransientForMod")
            .count(),
        1
    );
    assert_eq!(
        scenario::compile("sound-defaults", &result.source, &selected).unwrap(),
        bytes
    );

    for remove in [false, true] {
        let mut legacy = modules.clone();
        let library = legacy.get_mut("std::scenario").unwrap();
        for domain in ["SoundVolume", "SoundBinding"] {
            let start = library.find(&format!("pub enum {domain} ")).unwrap();
            let end = start + library[start..].find('}').unwrap() + 1;
            let replacement = if remove {
                String::new()
            } else {
                library[start..end].replace("= 255", "= 254")
            };
            library.replace_range(start..end, &replacement);
        }
        let selected = Definitions::from_sources(&legacy).unwrap();
        let result = scenario::decompile(&bytes, &selected).unwrap();
        assert!(!result.source.contains("SoundVolume::"));
        assert!(!result.source.contains("SoundBinding::"));
        assert_eq!(
            scenario::compile("sound-defaults", &result.source, &selected).unwrap(),
            bytes
        );
    }
}

#[test]
fn sound_control_names_require_the_original_slot_branch_and_keep_encoding() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "sound-controls",
        &source(
            "native::play_sound(-1, 0, 255, 0);
        native::play_sound(s16(-1), 0, 255, 4);
        native::play_sound((0 - 1), 0, 255, (1 + 2));
        native::play_sound(-1, 0, 255, 255);
        native::play_sound(-1, 0, 255, memory::i32[0x0810]);
        native::play_sound_simple(-1, 0);
        native::audio_command(-1);
        native::configure_sound(0, s16(100), 75);
        native::configure_sound(0, 101, 0);
        native::configure_sound(4, (100 + 2), 0);
        native::configure_sound(0, 255, 0);
        eval -1;",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "s16(scenario::SoundControl::StopSlot)",
        "encoded(scenario::SoundControl::StopSlot, (0 - 1))",
        "scenario::MusicControl::Stop",
        "s16(scenario::SoundSlotOperation::Volume)",
        "scenario::SoundSlotOperation::Pan",
        "encoded(scenario::SoundSlotOperation::Stop, (100 + 2))",
        "operation: 255",
        "eval -1;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::SoundControl")
            .count(),
        3
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "effects::EffectId" && id.value == -1 && !id.symbolic)
            .count(),
        3
    );
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "scenario::SoundSlotOperation"
                && id.value == 255
                && !id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("sound-controls", &result.source).unwrap();
    assert_eq!(
        scenario::compile("sound-controls", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn sound_control_definitions_can_be_renamed_moved_or_missing_without_fallbacks() {
    let bytes = scenario::compile(
        "sound-controls",
        &source(
            "native::play_sound(-1, 0, 255, 0);
        native::configure_sound(0, 102, 0);",
        ),
        &definitions(),
    )
    .unwrap();
    let mut sources = resonance_script_content::modules();
    let scenario = sources.get_mut("std::scenario").unwrap();
    assert!(scenario.contains("StopSlot = -1"));
    *scenario = scenario.replace("StopSlot = -1", "StopForMod = -1");
    let renamed = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &renamed).unwrap();
    assert!(result.source.contains("scenario::SoundControl::StopForMod"));
    assert_eq!(
        scenario::compile("sound-controls", &result.source, &renamed).unwrap(),
        bytes
    );

    let scenario = sources.get_mut("std::scenario").unwrap();
    *scenario = scenario.replace("StopForMod = -1", "StopForMod = -2");
    let start = scenario.find("pub enum SoundSlotOperation").unwrap();
    let end = start + scenario[start..].find('}').unwrap() + 1;
    let operation = scenario[start..end].replace("Stop = 102", "StopForMod = 103");
    scenario.replace_range(start..end, &operation);
    let moved = Definitions::from_sources(&sources).unwrap();
    let result = scenario::decompile(&bytes, &moved).unwrap();
    assert!(!result.source.contains("SoundControl::StopForMod"));
    assert!(!result.source.contains("SoundSlotOperation::Stop"));
    for (domain, value) in [
        ("scenario::SoundControl", -1),
        ("scenario::SoundSlotOperation", 102),
    ] {
        assert!(
            result
                .identities
                .iter()
                .any(|id| id.domain == domain && id.value == value && !id.symbolic)
        );
    }
    assert_eq!(
        scenario::compile("sound-controls", &result.source, &moved).unwrap(),
        bytes
    );

    for domain in ["SoundControl", "SoundSlotOperation"] {
        let mut partial_sources = sources.clone();
        let scenario = partial_sources.get_mut("std::scenario").unwrap();
        let start = scenario.find(&format!("pub enum {domain}")).unwrap();
        let end = start + scenario[start..].find('}').unwrap() + 1;
        scenario.replace_range(start..end, "");
        let partial = Definitions::from_sources(&partial_sources).unwrap();
        let error = scenario::decompile(&bytes, &partial)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("missing std::scenario::{domain} definitions")),
            "{error}"
        );
    }
}

#[test]
fn domain_metadata_resolves_selectors_catalogues_bitmasks_and_packed_titles() {
    let definitions = definitions();
    let text = source("native::yield_command(0, 1);
        eval native::set_actor_property(999999, 10, 1);
        eval native::set_camera_property(10, 20);
        native::configure_dialogue(0, s16(flags(scenario::DialogueFlag::AutoSide, scenario::DialogueFlag::Persistent)), -1, 1, 0, 0, u16(4), u16(5));
        native::learn_title(257); native::learn_technique(1, 1);
        native::learn_recipe(0); native::configure_ex_gem(1, 0, 1, 1);
        eval native::set_equipped_title(2, 257);
        eval native::load_portrait(851968);
        native::set_portrait_talking(1, 714523);")
        .replace("use std::story;", "use std::story; use std::scenario;");
    let bytes = scenario::compile("typed", &text, &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    for symbol in [
        "scenario::WaitCondition::Ticks",
        "scenario::ActorSelector::Controlled",
        "scenario::ActorProperty::DisableGrounding",
        "scenario::CameraProperty::PositionMinY",
        "characters::TitleId::ColetteFledglingChosen",
        "techniques::TechniqueId::DemonFangLloyd",
        "recipes::RecipeId::Sandwich",
        "characters::ExSkillId::Strong",
        "skits::PortraitId::LloydNeutral",
        "skits::PortraitTalkMode::Exclusive",
        "flags(scenario::DialogueFlag::Persistent, scenario::DialogueFlag::AutoSide)",
    ] {
        assert!(
            result.source.contains(symbol),
            "missing {symbol}: {}",
            result.source
        );
    }
    assert_eq!(
        scenario::compile("typed", &result.source, &definitions).unwrap(),
        bytes
    );
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.domain == "characters::TitleId" && id.value == 257 && id.symbolic)
    );
}

#[test]
fn equipped_title_character_index_does_not_acquire_the_controlled_actor_alias() {
    let definitions = definitions();
    let text = source(
        "eval native::set_equipped_title(999999, -1);
        eval native::set_equipped_title(2, 257);
        if (native::set_equipped_title(2, -1) == 257) { eval 1; }
        eval native::get_actor_property(999999, 10);",
    );
    let bytes = scenario::compile("title-record-index", &text, &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::FieldScene(373),
        scenario::Context::Battle,
        scenario::Context::World,
        scenario::Context::Skit,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(
            result.source.contains("character_id: 999999"),
            "{}",
            result.source
        );
        assert!(
            !result
                .source
                .contains("character_id: scenario::ActorSelector::Controlled")
        );
        assert!(
            result
                .source
                .contains("actor_id: scenario::ActorSelector::Controlled")
        );
        assert!(result.source.contains("characters::CharacterId::Colette"));
        assert!(
            result
                .source
                .contains("title_id: characters::TitleId::ColetteFledglingChosen")
        );
        assert!(
            result
                .source
                .contains("== characters::TitleId::ColetteFledglingChosen")
        );
        let index = result
            .identities
            .iter()
            .find(|identity| {
                identity.native == "set_equipped_title"
                    && identity.parameter == "character_id"
                    && identity.value == 999999
            })
            .unwrap();
        assert_eq!(index.domain, "characters::CharacterId");
        assert!(!index.symbolic);
        assert_eq!(
            scenario::compile("title-record-index", &result.source, &definitions).unwrap(),
            bytes
        );
    }
    // An explicitly supplied character definition can still name the raw
    // record index; the decompiler must not substitute an actor selector.
    let mut modules = resonance_script_content::modules();
    let module = modules.get_mut("std::characters").unwrap();
    assert_eq!(module.matches("pub enum CharacterId {").count(), 1);
    *module = module.replace(
        "pub enum CharacterId {",
        "pub enum CharacterId { ExplicitRecordIndex = 999999,",
    );
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert!(
        result
            .source
            .contains("character_id: characters::CharacterId::ExplicitRecordIndex")
    );
    assert_eq!(
        scenario::compile("title-record-index", &result.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn interpreter_depth_alias_is_a_readable_argument_without_changing_instructions() {
    let definitions = definitions();
    let text = source(
        "native::configure_actor_attachment(2, 2,
        native::find_actor_body_part(2, 2), stack_depth(), 0, 0, 40);",
    );
    let bytes = scenario::compile("alias", &text, &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Field).unwrap();
    assert!(result.source.contains("stack_depth()"));
    assert!(result.source.contains("scenario::BodyPartId::Spine1"));
    assert_eq!(result.intrinsic_instructions, 0);
    assert_eq!(
        scenario::compile("alias", &result.source, &definitions).unwrap(),
        bytes
    );
}

fn source(body: &str) -> String {
    format!(
        "script scenario;\nuse std::story;\nuse std::items;\nuse std::characters;\nuse std::monsters;\ncode_base = 4;\ndata {{ word(4); pc(entry); offset(auxiliary); word(0); }}\npub task entry() {{ {body} end; }}\nauxiliary:\ndata {{ hex(\"00112233AB\"); }}\n"
    )
}

#[test]
fn original_vm_source_lifts_expressions_calls_and_generated_constants() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x810] = (native::get_item_count(items::ItemId::MemoryGem) + 1));
        native::set_event_bit(story::StoryFlag::ManualMovementLearned);
        eval native::remove_party_member(characters::CharacterId::Lloyd);
        eval native::configure_monster_knowledge(monsters::MonsterId::Zombie, 1);
        native::configure_dialogue(0, 64, -1, 6, 0, 0, u16(4), u16(5));
        eval ++(memory::i16[0x820]); eval (memory::i8[0x830])--;
        eval -(s32(3)); eval (s16(-1) + u16(2));
        eval (memory::i32[0x840][2] += 5);",
    );
    let bytes = scenario::compile("test", &text, &definitions).unwrap();
    let decompiled = scenario::decompile(&bytes, &definitions).unwrap();
    for name in [
        "items::ItemId::MemoryGem",
        "story::StoryFlag::ManualMovementLearned",
        "characters::CharacterId::Lloyd",
        "monsters::MonsterId::Zombie",
    ] {
        assert!(
            decompiled.source.contains(name),
            "missing {name}: {}",
            decompiled.source
        );
    }
    assert!(decompiled.source.contains("memory::i32[0x0810] ="));
    assert!(decompiled.source.contains("native::configure_dialogue(\n"));
    assert_eq!(decompiled.intrinsic_instructions, 0);
    assert_eq!(
        scenario::compile("test", &decompiled.source, &definitions).unwrap(),
        bytes
    );
    let formatted = symphonia_script_compiler::format("test", &decompiled.source).unwrap();
    assert_eq!(
        symphonia_script_compiler::format("test", &formatted).unwrap(),
        formatted
    );
    assert_eq!(
        scenario::compile("test", &formatted, &definitions).unwrap(),
        bytes
    );
    let mut sources = resonance_script_content::modules();
    sources.insert("test".into(), decompiled.source);
    let compilation = symphonia_script_compiler::compile("test", &sources, &[]).unwrap();
    assert_eq!(compilation.kind, ScriptKind::Scenario);
    assert_eq!(compilation.scenario_bytes.unwrap(), bytes);
    assert!(compilation.sources.contains_key("std::items"));
}

#[test]
fn edits_relocate_branch_registry_default_and_auxiliary_offsets() {
    let text = "script scenario; use std::scenario;
        events {
            entry entry;
            auxiliary auxiliary;
            on scenario::EventKind::Spawned(123) => event;
        }
        task entry() { if (1) { eval 2; } call event; end; }
        task event() { eval 9; return; }
        auxiliary: data { hex(\"1234AB\"); }";
    let definitions = definitions();
    let original = scenario::compile("test", text, &definitions).unwrap();
    let edited = scenario::compile(
        "test",
        &text.replace("eval 2;", "eval 200000; eval 4;"),
        &definitions,
    )
    .unwrap();
    let before = binary::analyze(&original).unwrap();
    let after = binary::analyze(&edited).unwrap();
    assert_eq!(before.header.code_base_words, 10);
    assert_eq!(before.header.registry_count, 1);
    assert_eq!(
        after.header.auxiliary_offset(),
        before.header.auxiliary_offset() + 8
    );
    assert_eq!(after.records[0].pc, before.records[0].pc + 4);
    assert_eq!(
        Program::decode(&edited).unwrap().event(2, 123),
        Some(after.records[0].pc)
    );
    assert_eq!(
        &edited[after.header.auxiliary_offset()..],
        &[0x12, 0x34, 0xab]
    );
    let extra = scenario::compile(
        "extra-event",
        &text.replace(
            "on scenario::EventKind::Spawned(123) => event;",
            "on scenario::EventKind::Spawned(123) => event; on 1(2000) => entry;",
        ),
        &definitions,
    )
    .unwrap();
    let extra_header = binary::parse_header(&extra).unwrap();
    assert_eq!(extra_header.code_base_words, 16);
    assert_eq!(extra_header.registry_count, 2);
    assert_eq!(&extra[32..], &original[20..]);
    for invalid in [
        "events {}",
        "events { entry entry; entry entry; }",
        "events { entry entry; auxiliary aux; auxiliary aux; }",
        "code_base = 4; events { entry entry; }",
        "data { word(0); } events { entry entry; }",
        "events { entry entry; on -1(2) => entry; }",
        "events { entry entry; on 1(4294967296) => entry; }",
        "events { entry missing; } task entry() { end; }",
    ] {
        assert!(
            scenario::compile(
                "invalid",
                &format!("script scenario; {invalid}"),
                &definitions
            )
            .is_err()
        );
    }
}

#[test]
fn literal_widths_overlay_headers_and_stack_edges_match() {
    let definitions = definitions();
    let blobs = [
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\npush.s32 1\npush.s16 2\ncalc 0x31\ncalc 0\nend\n.byte 171\n",
        ".code_base 0\n.word 0\n.word 4\n.word 0\n.word 1\npush.s8 0\nend\n.word 0\n.word 0\n.word 0\n.word 4\n",
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\npush.s8 1\njump target\ntarget:\ncalc 0\narg\npush.s8 2\ncalc 0\nend\n",
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nproc 0\nend\n",
    ];
    for listing in blobs {
        let bytes = binary::assemble(listing).unwrap();
        let result = scenario::decompile(&bytes, &definitions).unwrap();
        assert_eq!(
            scenario::compile("test", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn symbols_are_resolved_from_current_generated_sources_and_invalid_inputs_fail() {
    let sources = BTreeMap::from([(
        "std::story".into(),
        "script library; pub enum StoryFlag { Custom = 777 }".into(),
    )]);
    let definitions = Definitions::from_sources(&sources).unwrap();
    let text = source("native::set_event_bit(story::StoryFlag::Custom);");
    let bytes = scenario::compile("mod", &text, &definitions).unwrap();
    assert_eq!(
        Program::decode(&bytes).unwrap().instruction(0),
        Some((Op::Push(777), 2))
    );
    assert!(scenario::compile("mod", &text.replace("Custom", "Missing"), &definitions).is_err());
    assert!(scenario::compile("mod", &text.replace("use std::story;", ""), &definitions).is_err());
    assert!(scenario::compile("mod", &source("eval s8(128);"), &definitions).is_err());
    assert!(scenario::compile("mod", &source("native::set_event_bit();"), &definitions).is_err());
    assert!(scenario::compile("mod", &source("goto missing;"), &definitions).is_err());
    for invalid in [
        "script library; pub enum StoryFlag { Duplicate = 1, Duplicate = 2 }",
        "script library; pub enum StoryFlag { First = 1, Second = 1 }",
        "script model; pub enum StoryFlag { First = 1 }",
    ] {
        let sources = BTreeMap::from([("std::story".into(), invalid.into())]);
        assert!(Definitions::from_sources(&sources).is_err());
    }
}

#[test]
fn structured_loops_story_globals_dead_code_and_duplicate_registry_keys_match() {
    let definitions = definitions();
    let text = source(
        "while (globals[story::GlobalId::Story] < story::StoryStage::IseliaExile) {
        if (native::test_event_bit(bit: story::StoryFlag::ManualMovementLearned)) {
            eval ++(globals[story::GlobalId::Story]);
        }
    }",
    );
    let bytes = scenario::compile("test", &text, &definitions).unwrap();
    let decompiled = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(
        decompiled.source.contains("while ("),
        "{}",
        decompiled.source
    );
    assert!(decompiled.source.contains("if ("));
    assert!(
        decompiled
            .source
            .contains("globals[story::GlobalId::Story]")
    );
    assert!(decompiled.source.contains("story::StoryStage::IseliaExile"));
    assert_eq!(
        scenario::compile("test", &decompiled.source, &definitions).unwrap(),
        bytes
    );

    let bytes = binary::assemble(".code_base 16\n.word 16\n.word 0\n.word 0\n.word 2\n.word 0\n.word 2\n.word 0\n.word 500\n.word 0\n.word 1\n.word 0\n.word 2\n.word 0\n.word 500\n.word 0\n.word 2\nend\nend\nend\npush.s8 12\ncalc 0\nend\n").unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    assert!(result.source.contains("events {"));
    assert!(!result.source.contains("code_base ="));
    assert!(
        result
            .source
            .contains("on scenario::EventKind::Spawned(500) => spawned_500;")
    );
    assert!(
        result
            .source
            .contains("on scenario::EventKind::Spawned(500) => spawned_500_r1;")
    );
    assert!(
        result.source.contains("eval 12;"),
        "disabled code should remain readable"
    );
    assert_eq!(Program::decode(&bytes).unwrap().event(2, 500), Some(1));
    assert_eq!(
        scenario::compile("test", &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn cli_decompiles_and_recompiles_editable_sym_atomically() -> Result<(), Box<dyn std::error::Error>>
{
    let work = tempfile::tempdir()?;
    let input = work.path().join("input.ssb");
    let target = work.path().join("output.ssb");
    let sym = work.path().join("source.sym");
    let bytes = scenario::compile("test", &source("eval 7;"), &definitions())?;
    fs::write(&input, &bytes)?;
    for (operation, input, output) in [("decompile", &input, &sym), ("compile", &sym, &target)] {
        symphonia_script_tools::run(
            [
                "scenario".into(),
                operation.into(),
                input.display().to_string(),
                output.display().to_string(),
            ],
            &[],
            &mut Vec::new(),
        )?;
    }
    assert_eq!(fs::read(&target)?, bytes);
    fs::write(&sym, "script scenario; invalid")?;
    assert!(
        symphonia_script_tools::run(
            [
                "scenario".into(),
                "compile".into(),
                sym.display().to_string(),
                target.display().to_string()
            ],
            &[],
            &mut Vec::new()
        )
        .is_err()
    );
    assert_eq!(fs::read(&target)?, bytes);
    // The known library root wins over parent names and equivalent path spellings.
    let assets = work.path().join("fields");
    for (path, source) in resonance_script_content::definitions() {
        let path = assets.join("scripts").join(path);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, source)?;
    }
    let input = assets.join("skits/example/scenario.ssb");
    fs::create_dir_all(input.parent().unwrap())?;
    fs::write(
        &input,
        scenario::compile(
            "skit",
            &source("eval native::set_actor_property(1, 63, 851968);"),
            &definitions(),
        )?,
    )?;
    fs::create_dir_all(work.path().join("alias"))?;
    symphonia_script_tools::run(
        [
            "scenario".into(),
            "decompile".into(),
            input.display().to_string(),
            sym.display().to_string(),
            "--assets".into(),
            work.path().join("alias/../fields").display().to_string(),
        ],
        &[],
        &mut Vec::new(),
    )?;
    assert!(fs::read_to_string(&sym)?.contains("skits::PortraitId::"));
    Ok(())
}

#[test]
fn battle_rule_selectors_name_the_service_without_retyping_values() {
    let definitions = definitions();
    let mut body = String::new();
    for rule in 0..=5 {
        body.push_str(&format!(
            "eval native::configure_battle_rules({rule},u16({rule}));\n"
        ));
    }
    body.push_str("eval native::configure_battle_rules(6,-1); eval native::configure_battle_rules(-1,0); eval native::configure_battle_rules(memory::i32[0x810],3); eval native::configure_battle_rules((1 + 2),s16(-25)); eval 3;");
    let bytes = scenario::compile("battle-rules", &source(&body), &definitions).unwrap();
    for context in [
        scenario::Context::Field,
        scenario::Context::Battle,
        scenario::Context::Unspecified,
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        for rule in symphonia_constants::BattleRule::ALL {
            assert!(
                result.source.contains(&format!(
                    "rule: scenario::BattleRule::{}, value: u16({})",
                    rule.name(),
                    rule.raw()
                )),
                "{}",
                result.source
            );
        }
        for expected in [
            "rule: 6, value: -1",
            "rule: -1, value: 0",
            "rule: memory::i32[0x0810], value: 3",
            "encoded(scenario::BattleRule::AttackPercent, (1 + 2))",
            "value: s16(-25)",
            "eval 3;",
        ] {
            assert!(
                result.source.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        assert_eq!(result.identities.len(), 9);
        assert_eq!(
            result.identities.iter().filter(|id| !id.symbolic).count(),
            2
        );
        assert!(
            result
                .identities
                .iter()
                .all(|id| id.native == "configure_battle_rules"
                    && id.parameter == "rule"
                    && id.domain == "scenario::BattleRule")
        );
        let formatted = symphonia_script_compiler::format("battle-rules", &result.source).unwrap();
        assert_eq!(
            scenario::compile("battle-rules", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn equipment_slots_type_queries_and_removals_without_retyping_item_results() {
    let definitions = definitions();
    let mut body = String::new();
    for slot in 0..=5 {
        body.push_str(&format!("if (native::get_equipped_item(999999,u16({slot})) == 3) {{ native::unequip_item(1,s16({slot})); }}\n"));
    }
    body.push_str("eval native::get_equipped_item(1,-1); native::unequip_item(1,6); eval native::get_equipped_item(1,memory::i16[0x810]); native::unequip_item(1,(2 + 3)); native::equip_item(1,3); eval 5;");
    let bytes = scenario::compile("equipment-slots", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for slot in symphonia_constants::EquipmentSlot::ALL {
        for width in ["u16", "s16"] {
            assert!(
                result.source.contains(&format!(
                    "slot: {width}(items::EquipmentSlot::{})",
                    slot.name()
                )),
                "{}",
                result.source
            );
        }
    }
    for expected in [
        "items::ItemId::OrangeGel",
        "slot: -1",
        "slot: 6",
        "slot: memory::i16[0x0810]",
        "encoded(items::EquipmentSlot::Arm, (2 + 3))",
        "item_id: items::ItemId::OrangeGel",
        "eval 5;",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    let slots: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.domain == "items::EquipmentSlot")
        .collect();
    assert_eq!(slots.len(), 15);
    assert_eq!(slots.iter().filter(|id| !id.symbolic).count(), 2);
    assert!(slots.iter().all(|id| id.parameter == "slot"
        && matches!(id.native.as_str(), "get_equipped_item" | "unequip_item")));
    let formatted = symphonia_script_compiler::format("equipment-slots", &result.source).unwrap();
    assert_eq!(
        scenario::compile("equipment-slots", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn collection_and_snapshot_operations_keep_their_separate_identity_domains() {
    let definitions = definitions();
    let body = "eval native::configure_figurine(u16(0),s16(50)); eval native::configure_figurine(1,0); eval native::configure_figurine((1 + 1),287); eval native::configure_figurine(-1,288); eval native::configure_figurine(memory::i32[0x810],50); native::snapshot_party(s16(0)); native::snapshot_party((0 + 1)); native::snapshot_party(2); native::snapshot_party(memory::i16[0x814]); eval 1;";
    let bytes = scenario::compile("collection-operations", &source(body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "operation: u16(figurines::FigurineOperation::SetOwned)",
        "figurine_id: s16(figurines::FigurineId::FrankBrunel)",
        "operation: figurines::FigurineOperation::ClearOwned",
        "figurine_id: figurines::FigurineId::LloydIrving",
        "encoded(figurines::FigurineOperation::QueryOwned, (1 + 1))",
        "operation: -1, figurine_id: 288",
        "operation: memory::i32[0x0810]",
        "operation: s16(scenario::PartySnapshotOperation::Capture)",
        "encoded(scenario::PartySnapshotOperation::Clear, (0 + 1))",
        "operation: 2",
        "operation: memory::i16[0x0814]",
        "eval 1;",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "figurines::FigurineOperation")
            .count(),
        4
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::PartySnapshotOperation")
            .count(),
        3
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "figurines::FigurineId")
            .count(),
        5
    );
    let formatted =
        symphonia_script_compiler::format("collection-operations", &result.source).unwrap();
    assert_eq!(
        scenario::compile("collection-operations", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn rendering_and_screen_copy_commands_do_not_retype_field_specific_values() {
    let definitions = definitions();
    let mut body = String::new();
    for command in 0..=7 {
        body.push_str(&format!(
            "native::configure_rendering(u16({command}),{command});\n"
        ));
    }
    body.push_str("native::configure_rendering(128,0); native::configure_rendering(128,u16(1)); native::configure_rendering(128,-1); native::configure_rendering(129,native::random_mod(2)); native::configure_rendering(-1,1); native::configure_rendering(130,1); native::configure_rendering(memory::i32[0x810],1); native::configure_rendering((120 + 8),2);");
    for command in 0..=5 {
        body.push_str(&format!(
            "eval native::configure_screen_copy(s16({command}),native::random_mod(2));\n"
        ));
    }
    body.push_str("eval native::configure_screen_copy(6,0); eval native::configure_screen_copy(memory::i16[0x814],1); eval 128;");
    let bytes = scenario::compile("render-control", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for command in symphonia_constants::RenderingCommand::ALL {
        assert!(
            result
                .source
                .contains(&format!("scenario::RenderingCommand::{}", command.name())),
            "{}",
            result.source
        );
    }
    for command in symphonia_constants::ScreenCopyCommand::ALL {
        assert!(
            result.source.contains(&format!(
                "command: s16(scenario::ScreenCopyCommand::{})",
                command.name()
            )),
            "{}",
            result.source
        );
    }
    for expected in [
        "value: scenario::RenderingToggle::Disabled",
        "value: u16(scenario::RenderingToggle::Enabled)",
        "value: -1",
        "value: 2",
        "value: 1",
        "command: -1",
        "command: 130",
        "command: 6",
        "depth_hundredths: native::random_mod(",
        "command: memory::i32[0x0810], value: 1",
        "encoded(scenario::RenderingCommand::Enabled, (120 + 8))",
        "eval 128;",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::RenderingToggle")
            .count(),
        4
    );
    assert!(
        !result
            .identities
            .iter()
            .any(|id| id.parameter == "depth_hundredths")
    );
    let formatted = symphonia_script_compiler::format("render-control", &result.source).unwrap();
    assert_eq!(
        scenario::compile("render-control", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn voice_archive_references_name_the_bank_without_changing_member_bits_or_encoding() {
    let definitions = definitions();
    let mut body = String::new();
    for archive in symphonia_constants::VoiceArchiveReference::ALL {
        body.push_str(&format!(
            "native::preload_voice_bank({},0);\n",
            archive.raw() | 65535
        ));
    }
    body.push_str("native::preload_voice_bank(s32(655446),u16(0)); native::preload_voice_bank((655360 + 86),1); native::preload_voice_bank(655446,2); native::preload_voice_bank(86,0); native::preload_voice_bank(0x00130056,0); native::preload_voice_bank(memory::i32[0x810],memory::i16[0x814]); eval 655446;");
    let bytes = scenario::compile("voice-archives", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for archive in symphonia_constants::VoiceArchiveReference::ALL {
        assert!(
            result.source.contains(&format!(
                "voice_archive(resources::VoiceArchiveReference::{}, 65535)",
                archive.name()
            )),
            "{}",
            result.source
        );
    }
    for expected in [
        "voice_reference: voice_archive(resources::VoiceArchiveReference::EventScenes001Through009, 86)",
        "bank_slot: u16(scenario::VoiceBankSlot::Primary)",
        "encoded(voice_archive(resources::VoiceArchiveReference::EventScenes001Through009, 86), (655360 + 86))",
        "bank_slot: scenario::VoiceBankSlot::Secondary",
        "bank_slot: 2",
        "voice_reference: 86",
        "voice_reference: 1245270",
        "voice_reference: memory::i32[0x0810]",
        "eval 655446;",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    let refs: Vec<_> = result
        .identities
        .iter()
        .filter(|id| id.domain == "resources::VoiceArchiveReference")
        .collect();
    assert_eq!(refs.len(), 22);
    assert_eq!(refs.iter().filter(|id| !id.symbolic).count(), 2);
    let formatted = symphonia_script_compiler::format("voice-archives", &result.source).unwrap();
    assert_eq!(
        scenario::compile("voice-archives", &formatted, &definitions).unwrap(),
        bytes
    );
    let helper = source(
        "eval voice_archive(resources::VoiceArchiveReference::EventScenes001Through009,86);",
    )
    .replace("script scenario;", "script scenario; use std::resources;");
    assert_eq!(
        scenario::compile("voice-helper", &helper, &definitions).unwrap(),
        scenario::compile("voice-literal", &source("eval 655446;"), &definitions).unwrap()
    );
    for invalid in [
        "voice_archive(1,0)",
        "voice_archive(-1,0)",
        "voice_archive(0x100000000,0)",
        "voice_archive(0xA0000,-1)",
        "voice_archive(0xA0000,65536)",
    ] {
        assert!(
            scenario::compile(
                "invalid-voice",
                &source(&format!("eval {invalid};")),
                &definitions
            )
            .is_err(),
            "{invalid}"
        );
    }
    let signed = scenario::compile(
        "voice-signed",
        &source("eval voice_archive(0xffff0000,65535);"),
        &definitions,
    )
    .unwrap();
    assert_eq!(
        signed,
        scenario::compile("voice-signed-literal", &source("eval -1;"), &definitions).unwrap()
    );
}

#[test]
fn message_voice_controls_name_archives_and_sentinels_without_typing_stream_pointers() {
    let definitions = definitions();
    let source = "script scenario; use std::scenario;
        code_base = 4; data { word(4); pc(entry); offset(auxiliary); word(0); }
        task entry() { end; }
        auxiliary: data { relative32(line, auxiliary); }
        line: data {
            message_control(9, s32(655400)); message_control(9,(655360 + 41));
            message_control(9,s16(-1)); message_control(9,u16(0));
            message_control(9,1245270); message_control(9,memory::i32[0x810]);
            message_control(9,voice_archive(0x80000000,66));
            message_control(5,655400); byte(0);
        }";
    let bytes = scenario::compile("message-voice", source, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "voice_archive(resources::VoiceArchiveReference::EventScenes001Through009, 40)",
        "encoded(voice_archive(resources::VoiceArchiveReference::EventScenes001Through009, 41), (655360 + 41))",
        "s16(scenario::VoiceControl::Stop)",
        "u16(scenario::VoiceControl::None)",
        "scenario::MessageControl::Voice, 1245270",
        "scenario::MessageControl::Voice, memory::i32[0x0810]",
        "scenario::MessageControl::Voice, -2147483582",
        "scenario::MessageControl::Number, 655400",
    ] {
        assert!(
            result.source.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert!(result.identities.is_empty());
    assert_eq!(result.message_identities.len(), 5);
    assert_eq!(
        result
            .message_identities
            .iter()
            .filter(|id| !id.symbolic)
            .count(),
        1
    );
    assert!(
        !result
            .message_identities
            .iter()
            .any(|id| id.value == -2147483582)
    );
    let formatted = symphonia_script_compiler::format("message-voice", &result.source).unwrap();
    assert_eq!(
        scenario::compile("message-voice", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn camera_track_commands_name_only_playback_options_and_command_specific_ids() {
    let definitions = definitions();
    let mut body = String::from(
        "native::play_camera_track(s32(-1179648),u16(0),s16(1)); native::play_camera_track(1,-1,257); native::play_camera_track(1,memory::i32[0x810],native::random_mod(2));",
    );
    for command in 0..=14 {
        body.push_str(&format!(
            "eval native::configure_camera_track(u16({command}),1);"
        ));
    }
    body.push_str("eval native::configure_camera_track((4 + 1),u16(0)); eval native::configure_camera_track(5,255); eval native::configure_camera_track(11,999999); eval native::configure_camera_track(12,999999); eval native::configure_camera_track(memory::i32[0x814],1); eval native::configure_camera_track(15,native::random_mod(2)); eval 1;");
    let bytes = scenario::compile("camera-commands", &source(&body), &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::FieldScene(372))
            .unwrap();
    for command in symphonia_constants::CameraTrackCommand::ALL {
        assert!(
            result
                .source
                .contains(&format!("scenario::CameraTrackCommand::{}", command.name())),
            "{}",
            result.source
        );
    }
    let compact = result
        .source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "resource_handle: -1179648",
        "start_mode: u16(scenario::CameraTrackStart::Playing)",
        "repeat_mode: s16(scenario::CameraTrackRepeat::Loop)",
        "start_mode: -1, repeat_mode: 257",
        "start_mode: memory::i32[0x0810], repeat_mode: native::random_mod(",
        "encoded(scenario::CameraTrackCommand::SetRepeat, (4 + 1))",
        "value: u16(scenario::CameraTrackRepeat::Once)",
        "value: scenario::CameraTrackRepeat::Loop",
        "value: characters::CharacterId::Lloyd",
        "value: scenario::ActorSelector::Controlled",
        "scenario::CameraTrackCommand::SetTargetOffsetX, value: 999999",
        "operation: memory::i32[0x0814], value: 1",
        "operation: 15, value: native::random_mod(",
        "value: 255",
        "eval 1;",
    ] {
        assert!(
            compact.contains(expected),
            "missing {expected}: {}",
            result.source
        );
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::CameraTrackCommand")
            .count(),
        20
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "scenario::CameraTrackRepeat")
            .count(),
        5
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "characters::CharacterId")
            .count(),
        1
    );
    let formatted = symphonia_script_compiler::format("camera-commands", &result.source).unwrap();
    assert_eq!(
        scenario::compile("camera-commands", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn battle_camera_control_slots_and_relative_facing_keep_their_native_contexts() {
    let definitions = definitions();
    let mut body = String::new();
    for command in 0..=11 {
        body.push_str(&format!(
            "eval native::configure_battle_camera(u16({command}),{command},0,0,0,0,0);"
        ));
    }
    body.push_str("eval native::configure_battle_camera((3 + 1),0,0,0,(-2147483648 | 60),0,0); eval native::configure_battle_camera(memory::i32[0x810],1,0,0,0,0,0); native::move_actor_relative(999999,90,s16(5),(-2147483648 | 20),u16(1)); native::move_actor_relative(1,0,1,1,0); native::move_actor_relative(123,0,1,1,2); eval native::configure_battle_control(s16(1),u16(2)); eval native::configure_battle_control(4,-1); eval native::configure_battle_control(0,8); eval 2;");
    let bytes = scenario::compile("battle-camera-ids", &source(&body), &definitions).unwrap();
    for context in [
        scenario::Context::Battle,
        scenario::Context::FieldScene(372),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        let compact = result
            .source
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for expected in [
            "party_slot: s16(battle::BattleControlSlot::Slot1)",
            "mode: u16(battle::BattleControlMode::Auto)",
            "party_slot: battle::BattleControlSlot::Slot4, mode: battle::BattleControlMode::Query",
            "party_slot: 0, mode: 8",
            "heading_degrees: 90, distance: s16(5), speed_or_duration: (-2147483648 | 20)",
            "command: memory::i32[0x0810]",
            "eval 2;",
        ] {
            assert!(
                compact.contains(expected),
                "missing {expected}: {}",
                result.source
            );
        }
        if context == scenario::Context::Battle {
            for command in symphonia_constants::BattleCameraCommand::ALL {
                assert!(
                    compact.contains(&format!("battle::BattleCameraCommand::{}", command.name())),
                    "{}",
                    result.source
                );
            }
            for expected in [
                "command: u16(0)",
                "command: u16(1)",
                "command: u16(11)",
                "encoded(battle::BattleCameraCommand::MoveBy, (3 + 1))",
                "actor_id: 999999",
                "facing: u16(1)",
                "actor_id: 1",
            ] {
                assert!(
                    compact.contains(expected),
                    "missing {expected}: {}",
                    result.source
                );
            }
            assert_eq!(
                result
                    .identities
                    .iter()
                    .filter(|id| id.domain == "battle::BattleCameraCommand")
                    .count(),
                13
            );
            assert!(!result.identities.iter().any(|id| matches!(
                id.domain.as_str(),
                "scenario::MovementFacing" | "characters::CharacterId" | "scenario::ActorSelector"
            )));
        } else {
            assert!(
                !result
                    .identities
                    .iter()
                    .any(|id| id.domain == "battle::BattleCameraCommand")
            );
            for expected in [
                "actor_id: scenario::ActorSelector::Controlled",
                "facing: u16(scenario::MovementFacing::KeepHeading)",
                "actor_id: characters::CharacterId::Lloyd",
                "facing: scenario::MovementFacing::FollowMovement",
                "facing: 2",
            ] {
                assert!(
                    compact.contains(expected),
                    "missing {expected}: {}",
                    result.source
                );
            }
            assert_eq!(
                result
                    .identities
                    .iter()
                    .filter(|id| id.domain == "scenario::MovementFacing")
                    .count(),
                3
            );
        }
        let formatted =
            symphonia_script_compiler::format("battle-camera-ids", &result.source).unwrap();
        assert_eq!(
            scenario::compile("battle-camera-ids", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn scene_model_names_apply_only_to_consumed_model_operands_in_the_matching_field() {
    use scenario::Context;
    let definitions = definitions();
    let bytes = scenario::compile(
        "scene-models",
        &source(
            r#"
        native::spawn_actor(500, -1179641, 0, 0, 0, s32(-1179641), 0, 0);
        native::create_scene_actor(501, 0, 0, 0, 0, (-1179640 - 1), 0, 0);
        native::spawn_interaction_actor(502, 0, 0, 0, 0, -1179640, 0, 0);
        native::spawn_collision_actor(503, 0, 0, 0, 0, -1179641, 0, 0);
        native::spawn_scenery_actor(504, 0, 0, 0, 0, -1179641, 0, 0);
        native::create_ring_station(505, 0, 0, 0, 0, -1179641);
        native::spawn_actor(-100, 0, 0, 0, 0, -1179641, 0, 0);
        native::spawn_actor(-1, 0, 0, 0, 0, -1179641, 0, 0);
        native::spawn_actor(999999, 0, 0, 0, 0, -1179641, 0, 0);
        native::configure_actor_animation(500, -1179641, 80, 0, 8);
        eval native::resolve_script_resource(-1179641);
    "#,
        ),
        &definitions,
    )
    .unwrap();
    for field in [520, 521] {
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let domain = format!("resources::Field{field:04}ModelId");
        let small = format!("{domain}::SmallFish");
        let large = format!("{domain}::LargeFish");
        assert_eq!(
            result.source.matches(&small).count(),
            5,
            "{}",
            result.source
        );
        assert_eq!(result.source.matches(&large).count(), 1);
        assert!(result.source.contains("x: -1179641"));
        assert!(result.source.contains(&format!("resource_id: {small}")));
        assert!(
            result
                .source
                .contains(&format!("encoded({small}, (-1179640 - 1))"))
        );
        let identities = result
            .identities
            .iter()
            .filter(|id| id.domain == domain)
            .collect::<Vec<_>>();
        assert_eq!(identities.len(), 6);
        assert!(identities.iter().all(|id| id.symbolic));
        // Emote/ignored branches, animation resources and a generic resolver
        // cannot acquire a model identity from an equal numeric value.
        assert!(identities.iter().all(|id| !matches!(
            id.native.as_str(),
            "configure_actor_animation" | "resolve_script_resource"
        )));
        let formatted = symphonia_script_compiler::format("scene-models", &result.source).unwrap();
        assert_eq!(
            scenario::compile("scene-models", &formatted, &definitions).unwrap(),
            bytes
        );
    }
    for context in [
        Context::Unspecified,
        Context::Field,
        Context::World,
        Context::Battle,
        Context::Skit,
        Context::FieldScene(518),
    ] {
        let result = scenario::decompile_with_context(&bytes, &definitions, context).unwrap();
        assert!(!result.source.contains("ModelId::SmallFish"));
        assert!(!result.source.contains("ModelId::LargeFish"));
        assert_eq!(
            scenario::compile("scene-models", &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
fn scene_model_names_follow_selected_definitions_and_explicit_model_enum_convention() {
    use scenario::Context;
    let mut sources = resonance_script_content::modules();
    let resources = sources.get_mut("std::resources").unwrap();
    let start = resources.find("pub enum Field0520ModelId").unwrap();
    let end = start + resources[start..].find('}').unwrap() + 1;
    resources.replace_range(
        start..end,
        "pub enum OtherField0520ModelId { WrongEnum = -1179641 }",
    );
    sources
        .get_mut("std::story")
        .unwrap()
        .push_str("\npub enum Field0520ModelId { WrongModule = -1179641 }\n");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let bytes = scenario::compile(
        "scene-models",
        &source("native::spawn_interaction_actor(500, 24, 0, 0, 0, -1179641, 0, 0);"),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(520)).unwrap();
    assert!(result.source.contains("resource: -1179641"));
    assert!(!result.source.contains("WrongEnum"));
    assert!(!result.source.contains("WrongModule"));
    sources
        .get_mut("std::resources")
        .unwrap()
        .push_str("\npub enum Field0520ModelId { ModFish = -1179641, ModLocalShape = 24 }\n");
    let definitions = Definitions::from_sources(&sources).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(520)).unwrap();
    assert!(
        result
            .source
            .contains("resources::Field0520ModelId::ModFish")
    );
    assert!(
        !result.source.contains("ModLocalShape"),
        "the equal-valued X coordinate stays scalar"
    );
    assert_eq!(
        scenario::compile("scene-models", &result.source, &definitions).unwrap(),
        bytes
    );
    let small_id = scenario::compile(
        "scene-models",
        &source("native::create_scene_actor(500, 0, 0, 0, 0, 24, 0, 0);"),
        &definitions,
    )
    .unwrap();
    let result =
        scenario::decompile_with_context(&small_id, &definitions, Context::FieldScene(520))
            .unwrap();
    assert!(
        result
            .source
            .contains("resources::Field0520ModelId::ModLocalShape")
    );
    assert_eq!(
        scenario::compile("scene-models", &result.source, &definitions).unwrap(),
        small_id
    );
}

#[test]
fn mana_leaf_route_names_shared_word_and_phases_without_naming_unprofiled_room_inputs() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x012C] = s32(1500));
        eval (memory::i32[0x0810] = memory::i32[0x012C]);
        goto captured; captured:
        eval (memory::i32[0x0810] >= 14000);
        eval (memory::i32[0x012C] = 0);
        eval (memory::i32[0x012C] = 7);
        eval (memory::i16[0x012C] == 1500);
        eval (memory::i32[0x0814] == 1500);
        eval (globals[story::GlobalId::Story] == 1000);",
    );
    let bytes = scenario::compile("mana-leaf", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "globals[story::GlobalId::ManaLeafHerbRoute] = s32(story::ManaLeafHerbStage::YmirWhiteSeedPlanted)",
        "memory::i32[0x0810] = globals[story::GlobalId::ManaLeafHerbRoute]",
        "memory::i32[0x0810] >= story::ManaLeafHerbStage::StorytellerMithosHistoryHeard",
        "globals[story::GlobalId::ManaLeafHerbRoute] = 0",
        "globals[story::GlobalId::ManaLeafHerbRoute] = 7",
        "memory::i16[0x012C] == 1500",
        "memory::i32[0x0814] == 1500",
        "Story] == story::StoryStage::DayOfProphecyClassInterrupted",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32"
                && id.domain == "story::ManaLeafHerbStage"
                && id.value == 14000
                && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("mana-leaf", &result.source).unwrap();
    assert_eq!(
        scenario::compile("mana-leaf", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn mana_leaf_route_uses_selected_word_and_phase_names_and_requires_the_value_domain() {
    let bytes = scenario::compile(
        "mana-leaf-mod",
        &source("eval (memory::i32[0x012C] == 1500);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story
        .replace("ManaLeafHerbRoute = 59", "ModHerbRoute = 59")
        .replace("YmirWhiteSeedPlanted = 1500", "ModWhiteFlowers = 1500");
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert!(result.source.contains(
        "globals[story::GlobalId::ModHerbRoute] == story::ManaLeafHerbStage::ModWhiteFlowers"
    ));
    assert_eq!(
        scenario::compile("mana-leaf-mod", &result.source, &selected).unwrap(),
        bytes
    );
    let story = modules.get_mut("std::story").unwrap();
    let start = story.find("pub enum ManaLeafHerbStage {").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "");
    let missing = Definitions::from_sources(&modules).unwrap();
    assert!(
        scenario::decompile(&bytes, &missing)
            .err()
            .unwrap()
            .to_string()
            .contains("ManaLeafHerbStage")
    );
}

#[test]
fn shadow_route_keeps_phase_timing_widths_snapshots_and_other_namespaces() {
    let definitions = definitions();
    let text = source(
        "eval (memory::i32[0x00F0] = s32(32400));
        eval (memory::i32[0x0810] = memory::i32[0x00F0]);
        goto captured; captured:
        eval (memory::i32[0x0810] == 33001);
        eval (memory::i32[0x00F0] = s32(41000));
        eval (memory::i32[0x00F0] == 50000);
        eval (memory::i16[0x00F0] == 1000);
        eval (memory::i32[0x0814] == 1000);
        eval (globals[story::GlobalId::Story] == 1000);
        eval native::test_event_bit(44);",
    );
    let bytes = scenario::compile("shadow-route", &text, &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "globals[story::GlobalId::ShadowPactRoute] = s32(story::ShadowPactStage::SecondLinkedBlockMoveStarted)",
        "memory::i32[0x0810] = globals[story::GlobalId::ShadowPactRoute]",
        "memory::i32[0x0810] == story::ShadowPactStage::LowerFloorAlternateEntryInput",
        "globals[story::GlobalId::ShadowPactRoute] = s32(story::ShadowPactStage::ShadowBattleRetryAvailable)",
        "ShadowPactRoute] == story::ShadowPactStage::ShadowPactRewardsComplete",
        "memory::i16[0x00F0] == 1000",
        "memory::i32[0x0814] == 1000",
        "Story] == story::StoryStage::DayOfProphecyClassInterrupted",
        "bit: story::StoryFlag::FlanoirVisited",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32"
                && id.domain == "story::ShadowPactStage"
                && id.value == 33001
                && id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("shadow-route", &result.source).unwrap();
    assert_eq!(
        scenario::compile("shadow-route", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn shadow_route_uses_selected_word_and_phase_names_and_requires_the_value_domain() {
    let bytes = scenario::compile(
        "shadow-mod",
        &source("eval (memory::i32[0x00F0] == 32400);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story
        .replace("ShadowPactRoute = 44", "ModShadowRoute = 44")
        .replace(
            "SecondLinkedBlockMoveStarted = 32400",
            "ModSecondBlockStart = 32400",
        );
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert!(result.source.contains(
        "globals[story::GlobalId::ModShadowRoute] == story::ShadowPactStage::ModSecondBlockStart"
    ));
    assert_eq!(
        scenario::compile("shadow-mod", &result.source, &selected).unwrap(),
        bytes
    );
    let story = modules.get_mut("std::story").unwrap();
    let start = story.find("pub enum ShadowPactStage {").unwrap();
    let end = start + story[start..].find('}').unwrap() + 1;
    story.replace_range(start..end, "");
    let missing = Definitions::from_sources(&modules).unwrap();
    assert!(
        scenario::decompile(&bytes, &missing)
            .err()
            .unwrap()
            .to_string()
            .contains("ShadowPactStage")
    );
}

#[test]
fn earth_and_ice_routes_keep_distinct_phases_snapshots_and_earlier_counter_values() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "earth-ice",
        &source(
            "eval (memory::i32[0x00E8] = s32(1000));
        eval (memory::i32[0x0810] = memory::i32[0x00E8]);
        eval (memory::i32[0x00EC] = 1000);
        goto captured; captured:
        eval (memory::i32[0x0810] == 1000);
        eval (memory::i32[0x00EC] == 23300);
        eval (memory::i32[0x00EC] == 23400);
        eval (memory::i32[0x00E8] = 0);
        eval (memory::i32[0x00E8])++;
        eval (memory::i32[0x00E8] == 20);
        eval (memory::i16[0x00E8] == 1000);
        eval (memory::i32[0x0814] == 1000); eval 1000;",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "globals[story::GlobalId::CelsiusPactRoute] = s32(story::CelsiusPactStage::PenguinistGlovesRequirementExplained)",
        "globals[story::GlobalId::GnomePactRoute] = story::GnomePactStage::MissingGnomeletteSearchRequested",
        "memory::i32[0x0810] == story::CelsiusPactStage::PenguinistGlovesRequirementExplained",
        "GnomePactRoute] == story::GnomePactStage::EscortPotionChoiceAvailable",
        "GnomePactRoute] == story::GnomePactStage::EscortPotionUnavailable",
        "CelsiusPactRoute] = 0",
        "(globals[story::GlobalId::CelsiusPactRoute])++",
        "CelsiusPactRoute] == 20",
        "memory::i16[0x00E8] == 1000",
        "memory::i32[0x0814] == 1000",
        "eval 1000;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .any(|id| id.native == "memory::snapshot32"
                && id.domain == "story::CelsiusPactStage"
                && id.value == 1000
                && id.symbolic)
    );
    assert!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "story::CelsiusPactStage" && [0, 20].contains(&id.value))
            .all(|id| !id.symbolic)
    );
    let formatted = symphonia_script_compiler::format("earth-ice", &result.source).unwrap();
    assert_eq!(
        scenario::compile("earth-ice", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn earth_and_ice_routes_follow_selected_word_and_phase_definitions() {
    let bytes = scenario::compile(
        "earth-ice-mod",
        &source("eval (memory::i32[0x00E8] == 41000); eval (memory::i32[0x00EC] == 41000);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story
        .replace("CelsiusPactRoute = 42", "ModIceRoute = 42")
        .replace("GnomePactRoute = 43", "ModEarthRoute = 43")
        .replace("CelsiusBattleRetryAvailable = 41000", "ModIceRetry = 41000")
        .replace("GnomeBattleRetryAvailable = 41000", "ModEarthRetry = 41000");
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert!(
        result.source.contains(
            "globals[story::GlobalId::ModIceRoute] == story::CelsiusPactStage::ModIceRetry"
        )
    );
    assert!(result.source.contains(
        "globals[story::GlobalId::ModEarthRoute] == story::GnomePactStage::ModEarthRetry"
    ));
    assert_eq!(
        scenario::compile("earth-ice-mod", &result.source, &selected).unwrap(),
        bytes
    );
    for name in ["CelsiusPactStage", "GnomePactStage"] {
        let mut missing_modules = modules.clone();
        let story = missing_modules.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {name} {{")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        let missing = Definitions::from_sources(&missing_modules).unwrap();
        assert!(
            scenario::decompile(&bytes, &missing)
                .err()
                .unwrap()
                .to_string()
                .contains(name)
        );
    }
}

#[test]
fn kate_and_side_event_states_keep_encoded_counters_deltas_and_snapshot_storage() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "kate-side",
        &source(
            "eval (memory::i32[0x013C] = (130000 + 1));
        eval (memory::i32[0x0810] = memory::i32[0x013C]);
        eval (memory::i32[0x0128] = s32(2000));
        goto captured; captured:
        eval (memory::i32[0x0810] == 80101);
        eval (memory::i32[0x013C] >= 2000);
        eval (memory::i32[0x013C])++;
        eval (memory::i32[0x013C] += 100);
        eval (memory::i32[0x013C] += 0);
        eval (memory::i8[0x0813] = 0);
        eval (memory::i32[0x0810] == 80101);
        eval (memory::i16[0x013C] == 0); eval 130001;",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "globals[story::GlobalId::SideEvent] = encoded(story::SideEventState::SybakLostChildCounterOne, (130000 + 1))",
        "memory::i32[0x0810] = globals[story::GlobalId::SideEvent]",
        "KateRescueRoute] = s32(story::KateRescueStage::ColiseumRescueRegistrationInput)",
        "memory::i32[0x0810] == story::SideEventState::ColiseumSingleBeginner",
        "SideEvent] >= story::SideEventState::MizuhoExitSceneLowerBound",
        "(globals[story::GlobalId::SideEvent])++",
        "SideEvent] += 100",
        "SideEvent] += 0",
        "memory::i32[0x0810] == 80101",
        "memory::i16[0x013C] == 0",
        "eval 130001;",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.native == "memory::snapshot32"
                && id.domain == "story::SideEventState"
                && id.value == 80101)
            .count(),
        1
    );
    let formatted = symphonia_script_compiler::format("kate-side", &result.source).unwrap();
    assert_eq!(
        scenario::compile("kate-side", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn kate_and_side_event_names_follow_selected_definitions_and_require_their_domains() {
    let bytes = scenario::compile(
        "kate-side-mod",
        &source("eval (memory::i32[0x0128] == 3000); eval (memory::i32[0x013C] == 80101);"),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story
        .replace("KateRescueRoute = 58", "ModKateRoute = 58")
        .replace("SideEvent = 63", "ModSideLatch = 63")
        .replace(
            "ColiseumRescueRegistrationAccepted = 3000",
            "ModRegistration = 3000",
        )
        .replace("ColiseumSingleBeginner = 80101", "ModSoloEntry = 80101");
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    assert!(result.source.contains(
        "globals[story::GlobalId::ModKateRoute] == story::KateRescueStage::ModRegistration"
    ));
    assert!(
        result.source.contains(
            "globals[story::GlobalId::ModSideLatch] == story::SideEventState::ModSoloEntry"
        )
    );
    assert_eq!(
        scenario::compile("kate-side-mod", &result.source, &selected).unwrap(),
        bytes
    );
    for name in ["KateRescueStage", "SideEventState"] {
        let mut missing_modules = modules.clone();
        let story = missing_modules.get_mut("std::story").unwrap();
        let start = story.find(&format!("pub enum {name} {{")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
        let missing = Definitions::from_sources(&missing_modules).unwrap();
        assert!(
            scenario::decompile(&bytes, &missing)
                .err()
                .unwrap()
                .to_string()
                .contains(name)
        );
    }
}

#[test]
fn named_global_counters_coordinates_and_indexed_status_keep_scalar_operations() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "named-storage",
        &source(
            "eval (memory::i32[0x0100] = (memory::i32[0x0100] + 1));
             eval (memory::i32[0x0100] == 3);
             eval (memory::i32[0x0104] = -525); eval (memory::i32[0x0108] = 225);
             eval (memory::i32[0x010C] = memory::i32[0x0104] + 150);
             eval (memory::i32[0x0110] = memory::i32[0x0108]);
             eval (memory::i32[0x0114] = memory::i32[0x010C]);
             eval (memory::i32[0x0118] = memory::i32[0x0110]);
             eval (memory::i32[0x011C] = memory::i32[0x0114]);
             eval (memory::i32[0x0120] = memory::i32[0x0118]);
             eval (memory::i32[0x0138] |= (1 << memory::i32[0x0828]));
             eval (memory::i32[0x0138] = 0);
             eval ((memory::i32[0x0054])[memory::i32[0x0828]] = s32(36000));
             eval ((memory::i32[0x0054])[memory::i32[0x0828]] > 1);
             eval (memory::i16[0x0100] == 3);
             eval (memory::i16[0x0134])++; eval (memory::i16[0x0136] = 15);",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "SeveredManaLinkCount] = (globals[story::GlobalId::SeveredManaLinkCount] + 1)",
        "SeveredManaLinkCount] == 3",
        "TowerOfManaLightPuzzle0X] = -525",
        "TowerOfManaLightPuzzle0Y] = 225",
        "TowerOfManaLightPuzzle1X] = (globals[story::GlobalId::TowerOfManaLightPuzzle0X] + 150)",
        "TowerOfManaLightPuzzle1Y] = globals[story::GlobalId::TowerOfManaLightPuzzle0Y]",
        "TowerOfManaLightPuzzle2X] = globals[story::GlobalId::TowerOfManaLightPuzzle1X]",
        "TowerOfManaLightPuzzle2Y] = globals[story::GlobalId::TowerOfManaLightPuzzle1Y]",
        "TowerOfManaLightPuzzle3X] = globals[story::GlobalId::TowerOfManaLightPuzzle2X]",
        "TowerOfManaLightPuzzle3Y] = globals[story::GlobalId::TowerOfManaLightPuzzle2Y]",
        "PalmacostaOrienteeringCheckpointMask] |= (1 << memory::i32[0x0828])",
        "PalmacostaOrienteeringCheckpointMask] = 0",
        "(globals[story::GlobalId::KatzExpeditionStatusBase])[memory::i32[0x0828]] = s32(36000)",
        "(globals[story::GlobalId::KatzExpeditionStatusBase])[memory::i32[0x0828]] > 1",
        "memory::i16[0x0100] == 3",
        "(memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCheckpointCount])++",
        "memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCurrentCheckpoint] = 15",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(result.identities.iter().all(|id| !matches!(
        id.native.as_str(),
        "memory::constant32" | "memory::snapshot32"
    )));
    let formatted = symphonia_script_compiler::format("named-storage", &result.source).unwrap();
    assert_eq!(
        scenario::compile("named-storage", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn named_global_storage_uses_selected_aliases_without_inventing_value_domains() {
    let names = [
        (5, "KatzExpeditionStatusBase"),
        (48, "SeveredManaLinkCount"),
        (49, "TowerOfManaLightPuzzle0X"),
        (50, "TowerOfManaLightPuzzle0Y"),
        (51, "TowerOfManaLightPuzzle1X"),
        (52, "TowerOfManaLightPuzzle1Y"),
        (53, "TowerOfManaLightPuzzle2X"),
        (54, "TowerOfManaLightPuzzle2Y"),
        (55, "TowerOfManaLightPuzzle3X"),
        (56, "TowerOfManaLightPuzzle3Y"),
        (62, "PalmacostaOrienteeringCheckpointMask"),
    ];
    let body = names
        .iter()
        .map(|(id, _)| format!("eval (memory::i32[0x{:04X}] == 3000);", 0x40 + 4 * id))
        .collect::<String>();
    let bytes = scenario::compile("named-storage-mod", &source(&body), &definitions()).unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    for (id, name) in names {
        *story = story.replace(&format!("{name} = {id}"), &format!("ModStorage{id} = {id}"));
    }
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    for (id, name) in names {
        assert!(
            result
                .source
                .contains(&format!("globals[story::GlobalId::ModStorage{id}] == 3000"))
        );
        assert!(!result.source.contains(&format!("story::GlobalId::{name}")));
    }
    assert_eq!(
        scenario::compile("named-storage-mod", &result.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn costume_snapshots_and_minigame_records_preserve_indices_reuse_and_scalar_updates() {
    let definitions = definitions();
    let mut body = String::new();
    for slot in 0..8 {
        body.push_str(&format!(
            "eval (memory::i32[0x{:04X}] = native::set_character_costume(1, -1));",
            0x15c + 4 * slot
        ));
    }
    body.push_str(
        "eval native::set_character_costume(1, memory::i32[0x015C]);
         eval (memory::i32[0x017C] = native::get_title(1, 0));
         eval native::set_equipped_title(1, memory::i32[0x017C]);
         if (memory::i32[0x0188] < memory::i32[0x0810]) {
             eval (memory::i32[0x0188] = memory::i32[0x0810]);
         }
         if ((memory::i32[0x018C])[memory::i32[0x0810]] > 0) {
             eval ((memory::i32[0x018C])[memory::i32[0x0810]] = 0);
         }
         eval (memory::i32[0x01A0] >= 5);
         eval (memory::i32[0x01A4] = memory::i32[0x01A4] + 1);
         eval ((memory::i32[0x01A8])[(memory::i32[0x0810] - 4)] = 100);
         eval (memory::i32[0x01B0] < memory::i32[0x0814]);
         eval (memory::i32[0x01B4] > memory::i32[0x0814]);
         eval (memory::i32[0x01A0] = native::load_portrait(0));
         eval (memory::i32[0x0814] = memory::i32[0x01A0]);
         eval (memory::i32[0x01A8] = 1);
         eval (memory::i16[0x018C] = 3);",
    );
    let bytes = scenario::compile("saved-costumes-records", &source(&body), &definitions).unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for slot in 0..8 {
        assert!(result.source.contains(&format!(
            "SavedCostume{slot}] = native::set_character_costume("
        )));
    }
    for expected in [
        "costume_or_minus_one: globals[story::GlobalId::SavedCostume0]",
        "SavedLloydEquippedTitle] = native::get_title(",
        "title_id: globals[story::GlobalId::SavedLloydEquippedTitle]",
        "SeaKatzHighestClearedLevel] < memory::i32[0x0810]",
        "(globals[story::GlobalId::SeaKatzBestTimeBase])[memory::i32[0x0810]] > 0",
        "UncleGameBestClearedStage] >= 5",
        "UncleGameCorrectSelections] = (globals[story::GlobalId::UncleGameCorrectSelections] + 1)",
        "(globals[story::GlobalId::EmotionalBalloonBestRoundsBase])[(memory::i32[0x0810] - 4)] = 100",
        "PalmacostaCafeteriaBestGaldReward] < memory::i32[0x0814]",
        "PalmacostaOrienteeringBestTimeTicks] > memory::i32[0x0814]",
        "UncleGameBestClearedStage] = native::load_portrait(",
        "memory::i32[0x0814] = globals[story::GlobalId::UncleGameBestClearedStage]",
        "EmotionalBalloonBestRoundsBase] = 1",
        "memory::i16[0x018C] = 3",
    ] {
        assert!(
            result.source.contains(expected),
            "{expected}\n{}",
            result.source
        );
    }
    assert!(
        result
            .identities
            .iter()
            .all(|id| !id.native.starts_with("memory::"))
    );
    let formatted =
        symphonia_script_compiler::format("saved-costumes-records", &result.source).unwrap();
    assert_eq!(
        scenario::compile("saved-costumes-records", &formatted, &definitions).unwrap(),
        bytes
    );
}

#[test]
fn costume_and_record_storage_resolves_all_selected_mod_names() {
    let names = [
        (71, "SavedCostume0"),
        (72, "SavedCostume1"),
        (73, "SavedCostume2"),
        (74, "SavedCostume3"),
        (75, "SavedCostume4"),
        (76, "SavedCostume5"),
        (77, "SavedCostume6"),
        (78, "SavedCostume7"),
        (79, "SavedLloydEquippedTitle"),
        (82, "SeaKatzHighestClearedLevel"),
        (83, "SeaKatzBestTimeBase"),
        (88, "UncleGameBestClearedStage"),
        (89, "UncleGameCorrectSelections"),
        (90, "EmotionalBalloonBestRoundsBase"),
        (92, "PalmacostaCafeteriaBestGaldReward"),
        (93, "PalmacostaOrienteeringBestTimeTicks"),
    ];
    let body = names
        .iter()
        .map(|(id, _)| format!("eval (memory::i32[0x{:04X}] == 3000);", 0x40 + 4 * id))
        .collect::<String>();
    let bytes = scenario::compile("saved-storage-mod", &source(&body), &definitions()).unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    for (id, name) in names {
        *story = story.replace(&format!("{name} = {id}"), &format!("ModSaved{id} = {id}"));
    }
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    for (id, name) in names {
        assert!(
            result
                .source
                .contains(&format!("globals[story::GlobalId::ModSaved{id}] == 3000"))
        );
        assert!(!result.source.contains(&format!("story::GlobalId::{name}")));
    }
    assert_eq!(
        scenario::compile("saved-storage-mod", &result.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn colette_wing_request_preserves_scalar_values_widths_and_selected_alias() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "wing-request",
        &source(
            "eval (memory::i32[0x0044] = 1); eval (memory::i32[0x0044] != 0);
             eval (memory::i32[0x0044] = 2); eval (memory::i32[0x0044] += 1);
             eval (memory::i32[0x0044] = 0); eval (memory::i16[0x0044] = 2);",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "ColetteWingsRequested] = 1",
        "ColetteWingsRequested] != 0",
        "ColetteWingsRequested] = 2",
        "ColetteWingsRequested] += 1",
        "ColetteWingsRequested] = 0",
        "memory::i16[0x0044] = 2",
    ] {
        assert!(result.source.contains(expected), "{}", result.source);
    }
    assert!(
        result
            .identities
            .iter()
            .all(|id| !id.native.starts_with("memory::"))
    );
    let formatted = symphonia_script_compiler::format("wing-request", &result.source).unwrap();
    assert_eq!(
        scenario::compile("wing-request", &formatted, &definitions).unwrap(),
        bytes
    );
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story.replace("ColetteWingsRequested = 1", "ModFlightRequest = 1");
    let selected = Definitions::from_sources(&modules).unwrap();
    let edited = scenario::decompile(&bytes, &selected).unwrap();
    assert!(
        edited
            .source
            .contains("globals[story::GlobalId::ModFlightRequest] = 2")
    );
    assert!(!edited.source.contains("ColetteWingsRequested"));
    assert_eq!(
        scenario::compile("wing-request-mod", &edited.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn narrow_address_names_preserve_signed_widths_indices_and_scalar_values() {
    let definitions = definitions();
    let bytes = scenario::compile(
        "narrow-addresses",
        &source(
            "eval (memory::i8[0x0150] = -1); eval (memory::i8[0x0154] = s32(300));
         eval (memory::i8[0x0158] = memory::i8[0x0150]);
         eval ((memory::i8[0x00A4])[memory::i32[0x0810]] |= 128);
         eval ((memory::i8[0x0180])[memory::i32[0x0810]] = 0);
         eval (memory::i16[0x0134])++; eval (memory::i16[0x0136] = 15);
         eval (memory::i16[0x0146] = 8);
         eval (memory::i16[0x0150] = 2); eval (memory::i8[0x0134] = 3);
         eval (memory::i32[0x0150] = 4); eval (memory::i32[0x0134] = 5);
         eval 336; eval 308;",
        ),
        &definitions,
    )
    .unwrap();
    let result = scenario::decompile(&bytes, &definitions).unwrap();
    for expected in [
        "memory::i8[story::ByteOffset::SavedFormation0] = -1",
        "memory::i8[story::ByteOffset::SavedFormation4] = s32(300)",
        "memory::i8[story::ByteOffset::SavedFieldLeader] = memory::i8[story::ByteOffset::SavedFormation0]",
        "(memory::i8[story::ByteOffset::KatzExpeditionResultBase])[memory::i32[0x0810]] |= 128",
        "(memory::i8[story::ByteOffset::SecondarySavedFormationBase])[memory::i32[0x0810]] = 0",
        "(memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCheckpointCount])++",
        "memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCurrentCheckpoint] = 15",
        "memory::i16[story::HalfwordOffset::HeimdallCompanionActor] = 8",
        "memory::i16[0x0150] = 2",
        "memory::i8[0x0134] = 3",
        "memory::i32[0x0150] = 4",
        "memory::i32[0x0134] = 5",
        "eval 336;",
        "eval 308;",
    ] {
        assert!(
            result.source.contains(expected),
            "{expected}\n{}",
            result.source
        );
    }
    assert!(
        result
            .identities
            .iter()
            .all(|id| !id.native.starts_with("memory::"))
    );
    let formatted = symphonia_script_compiler::format("narrow-addresses", &result.source).unwrap();
    assert_eq!(
        scenario::compile("narrow-addresses", &formatted, &definitions).unwrap(),
        bytes
    );
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    for domain in ["ByteOffset", "HalfwordOffset"] {
        let start = story.find(&format!("pub enum {domain} {{")).unwrap();
        let end = start + story[start..].find('}').unwrap() + 1;
        story.replace_range(start..end, "");
    }
    story.push_str("\npub enum ArbitraryAddress { SameByte = 336, SameHalf = 308 }\n");
    let selected = Definitions::from_sources(&modules).unwrap();
    let raw = scenario::decompile(&bytes, &selected).unwrap();
    assert!(raw.source.contains("memory::i8[0x0150] = -1"));
    assert!(raw.source.contains("(memory::i16[0x0134])++"));
    assert!(!raw.source.contains("ByteOffset::") && !raw.source.contains("HalfwordOffset::"));
    assert!(!raw.source.contains("ArbitraryAddress::"));
    assert_eq!(
        scenario::compile("narrow-without-aliases", &raw.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn narrow_offsets_follow_mod_definitions_and_keep_partial_write_snapshot_invalidation() {
    let bytes = scenario::compile(
        "narrow-mod",
        &source(
            "eval (memory::i8[0x0150] = 1); eval (memory::i16[0x0134] = 2);
         eval (memory::i32[0x0810] = memory::i32[0x0128]);
         goto before; before: eval (memory::i32[0x0810] == 3000);
         eval (memory::i8[0x0813] = 0);
         goto after; after: eval (memory::i32[0x0810] == 3000);",
        ),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    *story = story
        .replace("SavedFormation0 = 336", "ModPartyByte = 336")
        .replace(
            "PalmacostaOrienteeringCheckpointCount = 308",
            "ModCheckpointCount = 308",
        )
        .replace(
            "pub enum ByteOffset {",
            "pub enum ByteOffset { ModSnapshotHighByte = 2067,",
        );
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    for expected in [
        "memory::i8[story::ByteOffset::ModPartyByte] = 1",
        "memory::i16[story::HalfwordOffset::ModCheckpointCount] = 2",
        "memory::i8[story::ByteOffset::ModSnapshotHighByte] = 0",
        "memory::i32[0x0810] == story::KateRescueStage::ColiseumRescueRegistrationAccepted",
        "memory::i32[0x0810] == 3000",
    ] {
        assert!(
            result.source.contains(expected),
            "{expected}\n{}",
            result.source
        );
    }
    assert!(!result.source.contains("ByteOffset::SavedFormation0"));
    assert!(
        !result
            .source
            .contains("HalfwordOffset::PalmacostaOrienteeringCheckpointCount")
    );
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.native == "memory::snapshot32" && id.symbolic)
            .count(),
        1
    );
    assert_eq!(
        scenario::compile("narrow-mod", &result.source, &selected).unwrap(),
        bytes
    );
}

#[test]
fn portrait_input_ids_stay_named_while_returned_handle_comparisons_stay_scalar() {
    let definitions = definitions();
    let bytes = scenario::compile("portrait-handle", &source(
        "eval (native::load_portrait(851968) == 851968);
         eval (native::load_portrait(851968) == -131072);
         eval (memory::i32[0x019C] = native::load_portrait(851968));
         eval (memory::i32[0x019C] == 851968);
         eval (memory::i32[0x0308] = s32(0));
         eval (memory::i32[0x030C] = 1000); eval (memory::i32[0x0310] = 4000);
         eval native::configure_battle_camera(2, memory::i32[0x0308], memory::i32[0x030C], memory::i32[0x0310], 0, 0, 0);"), &definitions).unwrap();
    let result =
        scenario::decompile_with_context(&bytes, &definitions, scenario::Context::Battle).unwrap();
    for expected in [
        "native::load_portrait(resource_id: skits::PortraitId::LloydNeutral) == 851968",
        "native::load_portrait(resource_id: skits::PortraitId::LloydNeutral) == -131072",
        "TestSkitLloydNeutralHandle] = native::load_portrait(resource_id: skits::PortraitId::LloydNeutral)",
        "TestSkitLloydNeutralHandle] == 851968",
        "BattleCameraEyeX] = s32(0)",
        "BattleCameraEyeY] = 1000",
        "BattleCameraEyeZ] = 4000",
        "battle::BattleCameraCommand::SetPose",
    ] {
        assert!(
            result.source.contains(expected),
            "{expected}\n{}",
            result.source
        );
    }
    assert_eq!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "skits::PortraitId")
            .count(),
        3
    );
    assert!(
        result
            .identities
            .iter()
            .filter(|id| id.domain == "skits::PortraitId")
            .all(|id| id.native == "load_portrait" && id.parameter == "resource_id")
    );
    let formatted = symphonia_script_compiler::format("portrait-handle", &result.source).unwrap();
    assert_eq!(
        scenario::compile("portrait-handle", &formatted, &definitions).unwrap(),
        bytes
    );
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(registry.get(0x58).unwrap().result_type, None);
}

#[test]
fn portrait_handle_and_battle_eye_storage_follow_selected_definitions() {
    let bytes = scenario::compile(
        "handle-eye-mod",
        &source(
            "eval (memory::i32[0x019C] = native::load_portrait(851968));
         eval (memory::i32[0x0308] = 0); eval (memory::i32[0x030C] = 1000);
         eval (memory::i32[0x0310] = 4000);",
        ),
        &definitions(),
    )
    .unwrap();
    let mut modules = resonance_script_content::modules();
    let story = modules.get_mut("std::story").unwrap();
    for (id, name) in [
        (87, "TestSkitLloydNeutralHandle"),
        (178, "BattleCameraEyeX"),
        (179, "BattleCameraEyeY"),
        (180, "BattleCameraEyeZ"),
    ] {
        *story = story.replace(&format!("{name} = {id}"), &format!("ModStorage{id} = {id}"));
    }
    let skits = modules.get_mut("std::skits").unwrap();
    *skits = skits.replace("LloydNeutral = 851968", "ModNeutral = 851968");
    let selected = Definitions::from_sources(&modules).unwrap();
    let result = scenario::decompile(&bytes, &selected).unwrap();
    for id in [87, 178, 179, 180] {
        assert!(
            result
                .source
                .contains(&format!("story::GlobalId::ModStorage{id}"))
        );
    }
    assert!(result.source.contains("skits::PortraitId::ModNeutral"));
    assert!(
        !result.source.contains("TestSkitLloydNeutralHandle")
            && !result.source.contains("BattleCameraEye")
    );
    assert_eq!(
        scenario::compile("handle-eye-mod", &result.source, &selected).unwrap(),
        bytes
    );
}
