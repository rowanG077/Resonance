use anyhow::{Context, Result, ensure};
use resonance_importer as cooker;
use std::{env, ffi::OsString, fmt::Display, path::PathBuf, str::FromStr};

fn number<T: FromStr>(args: &[OsString], index: usize) -> Result<T>
where
    T::Err: Display,
{
    let value = args
        .get(index)
        .context("missing selection")?
        .to_str()
        .context("selection is not UTF-8")?;
    value
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid selection {value:?}: {error}"))
}

fn selection<T: FromStr>(args: &[OsString]) -> Result<Option<T>>
where
    T::Err: Display,
{
    args.get(3).map(|_| number(args, 3)).transpose()
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3
            || args.len() == 4 && args[0] == "movies"
            || args.len() == 4 && args[0] == "character-attachments"
            || args.len() == 4 && args[0] == "character-models"
            || args.len() == 4 && args[0] == "music"
            || args.len() == 4 && args[0] == "effect"
            || args.len() == 4 && args[0] == "field-core"
            || args.len() == 4 && args[0] == "field-actors"
            || args.len() == 4 && args[0] == "field-local-models"
            || args.len() == 4 && args[0] == "field-definition"
            || args.len() == 4 && args[0] == "field-scene"
            || args.len() == 4 && args[0] == "shared-animations"
            || args.len() == 4 && args[0] == "shared-animations-field"
            || args.len() == 4 && args[0] == "shared-animation-bindings"
            || args.len() == 4 && args[0] == "ui-location-titles"
            || args.len() == 4 && args[0] == "skit-script"
            || args.len() == 4 && args[0] == "skit-portrait"
            || args.len() == 4 && args[0] == "figurine-media"
            || args.len() == 4 && args[0] == "battle-event-scripts"
            || args.len() == 4 && args[0] == "battle-event-media"
            || args.len() == 4 && args[0] == "battle-scene-parameters"
            || args.len() == 4 && args[0] == "battle-simple-scene"
            || args.len() == 4 && args[0] == "battle-skill"
            || args.len() == 4 && args[0] == "battle-victories"
            || args.len() == 4 && args[0] == "battle-enemy-media"
            || args.len() == 4 && args[0] == "battle-enemy-effects"
            || args.len() == 4 && args[0] == "battle-enemy-rules"
            || args.len() == 4 && args[0] == "battle-equipment"
            || args.len() == 4 && args[0] == "field-npc-model"
            || args.len() == 3 && args[0] == "field-models"
            || args.len() == 5 && args[0] == "field-model"
            || args.len() == 4 && args[0] == "world-cinematics"
            || args.len() == 4 && args[0] == "world-terrain-visual"
            || args.len() == 4 && args[0] == "world-terrain-tile"
            || args.len() == 5 && args[0] == "world-terrain-tile" && args[4] == "alternate"
            || args.len() == 5 && args[0] == "world-terrain-texture"
            || args.len() == 4 && args[0] == "battle-stage"
            || args.len() == 5 && args[0] == "battle-effect-texture"
            || args.len() == 5 && args[0] == "battle-party-model",
        "usage: resonance-importer <owner> <extraction-root> <output> [selection]"
    );
    let extracted = PathBuf::from(&args[1]);
    let library = PathBuf::from(&args[2]);
    match args[0].to_str() {
        Some("all") => cooker::cook_all(&extracted, &library),
        Some("descriptor") => cooker::cook_descriptor(&library),
        Some("extra-artwork") => cooker::cook_extra_artwork(&extracted, &library),
        Some("music-extras") => cooker::cook_extra_music(&extracted, &library),
        Some("font-styles") => cooker::cook_font_styles(&extracted, &library),
        Some("original-models") => cooker::cook_original_models(&extracted, &library),
        Some("original-animations") => cooker::cook_original_animations(&extracted, &library),
        Some("local-animation-rigs") => cooker::cook_local_animation_rigs(&extracted, &library),
        Some("unassigned-skits") => cooker::cook_unassigned_skits(&extracted, &library),
        Some("scripts") => cooker::cook_scripts(&library),
        Some("battle-victories") => {
            if let Some(selection) = args.get(3) {
                let ids = selection
                    .to_str()
                    .context("victory IDs are not UTF-8")?
                    .split(',')
                    .map(|id| id.parse::<u8>().context("invalid victory character"))
                    .collect::<Result<Vec<_>>>()?;
                cooker::cook_battle_victory_group(&extracted, &library, &ids)
            } else {
                cooker::cook_battle_victories(&extracted, &library)
            }
        }
        Some("battle-party-rules") => cooker::cook_battle_party_rules(&extracted, &library),
        Some("game-ui") => cooker::cook_game_ui(&extracted, &library).map(|_| ()),
        Some("field-overlay-media") => cooker::cook_field_overlay_media(&extracted, &library),
        Some("battle-equipment-mappings") => {
            cooker::cook_battle_equipment_mappings(&extracted, &library).map(|_| ())
        }
        Some("figurine-media") => {
            cooker::cook_figurine_media(&extracted, &library, selection(&args)?)
        }
        Some("manual") => cooker::cook_manual(&extracted, &library),
        Some("rename") => cooker::cook_rename(&extracted, &library),
        Some("options") => cooker::cook_options(&extracted, &library),
        Some("titles") => cooker::cook_titles(&extracted, &library),
        Some("ex-skills") => cooker::cook_ex_skills(&extracted, &library),
        Some("strategy") => cooker::cook_strategy(&extracted, &library),
        Some("cooking") => cooker::cook_cooking(&extracted, &library),
        Some("credits") => cooker::cook_credits_screen(&extracted, &library),
        Some("boot") => cooker::cook_boot(&extracted, &library),
        Some("ending") => cooker::cook_ending(&extracted, &library),
        Some("title-screen") => cooker::cook_title_screen(&extracted, &library),
        Some("input-prompts") => cooker::cook_input_prompts(&library),
        Some("ui-controls") => cooker::cook_ui_controls(&extracted, &library),
        Some("ui-emotions") => cooker::cook_ui_emotions(&extracted, &library),
        Some("ui-location-titles") => cooker::cook_ui_location_titles(
            &extracted,
            &library,
            args.get(3)
                .map(|value| value.to_str().context("title name is not UTF-8"))
                .transpose()?,
        ),
        Some("dialogue-font") => cooker::cook_dialogue_font(&extracted, &library),
        Some("dialogue-art") => cooker::cook_dialogue_art(&extracted, &library),
        Some("battle-font") => cooker::cook_battle_font(&extracted, &library),
        Some("items") => cooker::cook_items(&extracted, &library),
        Some("techniques") => cooker::cook_techniques(&extracted, &library),
        Some("characters") => cooker::cook_characters(&extracted, &library),
        Some("character-attachments") => cooker::cook_character_attachments(
            &extracted,
            &library,
            args.get(3)
                .map(|value| value.to_str().context("attachment name is not UTF-8"))
                .transpose()?,
        ),
        Some("character-models") => {
            cooker::cook_character_models(&extracted, &library, selection(&args)?)
        }
        Some("music") => cooker::cook_music(&extracted, &library, selection(&args)?),
        Some("speech") => cooker::cook_speech(&extracted, &library),
        Some("speech-resume") => {
            cooker::cook_speech_resume(&extracted, &library).context("resume shared speech")
        }
        Some("skit-script") => cooker::cook_skit_script(&extracted, &library, number(&args, 3)?),
        Some("skit-catalogue") => cooker::cook_skit_catalogue(&extracted, &library),
        Some("skits") => cooker::cook_skits(&extracted, &library),
        Some("skit-portrait") => {
            cooker::cook_skit_portrait(&extracted, &library, number(&args, 3)?)
        }
        Some("skit-portraits") => cooker::cook_skit_portraits(&extracted, &library),
        Some("audio") => cooker::cook_audio_catalogue(&extracted, &library),
        Some("effect") => {
            cooker::cook_effects(&extracted, &library, Some(number(&args, 3)?)).map(|_| ())
        }
        Some("menu-audio") => cooker::cook_menu_audio(&extracted, &library),
        Some("mix-audio") => cooker::cook_mix_audio(&extracted, &library),
        Some("synthesis-audio") => cooker::cook_synthesis_audio(&extracted, &library),
        Some("field-core") => cooker::cook_field_core(&extracted, &library, selection(&args)?),
        Some("field-actors") => cooker::cook_field_actors(&extracted, &library, number(&args, 3)?),
        Some("field-local-models") => {
            cooker::cook_field_local_models(&extracted, &library, number(&args, 3)?)
        }
        Some("field-definition") => {
            cooker::cook_field_definition(&extracted, &library, number(&args, 3)?)
        }
        Some("field-settings") => cooker::cook_field_settings(&extracted, &library),
        Some("field-wings") => cooker::cook_field_wings(&extracted, &library),
        Some("field-npc-model") => {
            cooker::cook_field_npc_model(&extracted, &library, number(&args, 3)?)
        }
        Some("field-models") => cooker::cook_field_models(&extracted, &library),
        Some("field-model") => {
            cooker::cook_field_model(&extracted, &library, number(&args, 3)?, number(&args, 4)?)
        }
        Some("field-scene") => cooker::cook_field_scene(&extracted, &library, number(&args, 3)?),
        Some("shared-animations") => {
            cooker::cook_shared_animations(&extracted, &library, selection(&args)?)
        }
        Some("shared-animations-field") => {
            cooker::cook_shared_animations_for_field(&extracted, &library, number(&args, 3)?)
        }
        Some("shared-animation-bindings") => {
            cooker::cook_shared_animation_bindings(&extracted, &library, selection(&args)?)
        }
        Some("battle-formations") => cooker::cook_battle_formations(&extracted, &library),
        Some("battle-recoil") => cooker::cook_battle_recoil(&extracted, &library),
        Some("battle-party-mappings") => {
            cooker::cook_battle_party_mappings(&extracted, &library).map(|_| ())
        }
        Some("battle-effect-tints") => cooker::cook_battle_effect_tints(&extracted, &library),
        Some("battle-effects") => cooker::cook_battle_effects(&extracted, &library),
        Some("battle-effect-atlases") => {
            cooker::cook_battle_effect_atlases(&extracted, &library).map(|_| ())
        }
        Some("battle-effect-models") => {
            cooker::cook_battle_effect_models(&extracted, &library).map(|_| ())
        }
        Some("battle-common-effects") => cooker::cook_common_effects(&extracted, &library),
        Some("battle-technique-effects") => cooker::cook_technique_effects(&extracted, &library),
        Some("battle-scene-mappings") => cooker::cook_battle_scene_mappings(&extracted, &library),
        Some("battle-skill-mappings") => cooker::cook_battle_skill_mappings(&extracted, &library),
        Some("battle-event-scripts") => {
            cooker::cook_battle_event_scripts(&extracted, &library, selection(&args)?)
        }
        Some("battle-event-media") => {
            cooker::cook_battle_event_media(&extracted, &library, selection(&args)?)
        }
        Some("battle-scene-parameters") => {
            cooker::cook_battle_scene_parameters(&extracted, &library, number(&args, 3)?)
        }
        Some("battle-simple-scene") => {
            cooker::cook_battle_simple_scene(&extracted, &library, number(&args, 3)?)
        }
        Some("battle-skill") => {
            cooker::cook_battle_skill_package(&extracted, &library, number(&args, 3)?)
        }
        Some("battle-effect-texture") => cooker::cook_battle_effect_texture(
            &extracted,
            &library,
            number(&args, 3)?,
            number(&args, 4)?,
        ),
        Some("battle-party-model") => cooker::cook_battle_party_model(
            &extracted,
            &library,
            number(&args, 3)?,
            number(&args, 4)?,
        ),
        Some("battle-projectiles") => cooker::cook_battle_projectiles(&extracted, &library),
        Some("battle-defense") => cooker::cook_battle_defense(&extracted, &library),
        Some("battle-unison") => cooker::cook_battle_unison(&extracted, &library),
        Some("battle-trails") => cooker::cook_battle_trails(&extracted, &library)
            .context("publish named battle trail materials"),
        Some("battle-equipment") => {
            cooker::cook_battle_equipment(&extracted, &library, number(&args, 3)?)
        }
        Some("battle-victory-groups") => cooker::cook_battle_victory_groups(&extracted, &library),
        Some("battle-audio") => cooker::cook_battle_audio(&extracted, &library),
        Some("battle-stage-mappings") => cooker::cook_battle_stage_mappings(&extracted, &library),
        Some("battle-stage") => cooker::cook_battle_stage(&extracted, &library, number(&args, 3)?),
        Some("battle-ui-portraits") => cooker::cook_battle_ui_portraits(&extracted, &library),
        Some("battle-ui-hud") => cooker::cook_battle_ui_hud(&extracted, &library),
        Some("battle-enemy-media") => {
            let ids = args[3]
                .to_str()
                .context("enemy IDs are not UTF-8")?
                .split(',')
                .map(|id| id.parse::<u8>().context("invalid enemy ID"))
                .collect::<Result<Vec<_>>>()?;
            cooker::cook_battle_enemy_media_group(&extracted, &library, &ids)
        }
        Some("battle-enemy-effects") => {
            let ids = args[3]
                .to_str()
                .context("enemy IDs are not UTF-8")?
                .split(',')
                .map(|id| id.parse::<u8>().context("invalid enemy ID"))
                .collect::<Result<Vec<_>>>()?;
            cooker::cook_battle_enemy_effects_group(&extracted, &library, &ids)
        }
        Some("battle-enemy-rules") => {
            cooker::cook_battle_enemy_rules(&extracted, &library, number(&args, 3)?)
        }
        Some("battle-enemy-mappings") => cooker::cook_battle_enemy_mappings(&extracted, &library),
        Some("menu-catalogues") => cooker::cook_menu_catalogues(&extracted, &library),
        Some("menu-art") => cooker::cook_menu_art(&extracted, &library),
        Some("menu-figurines") => cooker::cook_menu_figurines(&extracted, &library),
        Some("game-over") => cooker::cook_game_over_screen(&extracted, &library),
        Some("native-models") => cooker::cook_native_models(&extracted, &library),
        Some("native-cameras") => cooker::cook_native_cameras(&extracted, &library),
        Some("texture-banks") => cooker::cook_shared_texture_banks(&extracted, &library),
        Some("mesh-mappings") => {
            cooker::cook_mesh_mappings(&extracted, &library).context("publish model ID mappings")
        }
        Some("menu-world-map") => cooker::cook_menu_world_map(&extracted, &library),
        Some("menu-presentation") => cooker::cook_menu_presentation(&extracted, &library),
        Some("menu-synopsis") => cooker::cook_menu_synopsis(&extracted, &library),
        Some("menu-monster-book") => cooker::cook_menu_monster_book(&extracted, &library),
        Some("field-directory") => cooker::cook_field_directory(&extracted, &library).map(|_| ()),
        Some("world-movement") => cooker::cook_world_movement(&extracted, &library),
        Some("world-encounters") => cooker::cook_world_encounters(&extracted, &library),
        Some("world-dust") => cooker::cook_world_dust(&extracted, &library),
        Some("world-actors-common") => cooker::cook_world_actors_common(&extracted, &library),
        Some("world-actors-party") => cooker::cook_world_actors_party(&extracted, &library),
        Some("world-cinematics") => {
            cooker::cook_world_cinematics(&extracted, &library, selection(&args)?)
        }
        Some("world-definition") => cooker::cook_world_definition(&extracted, &library),
        Some("world-landmarks") => cooker::cook_world_landmarks(&extracted, &library),
        Some("world-markers") => cooker::cook_world_markers(&extracted, &library),
        Some("world-terrain-directory") => {
            cooker::cook_world_terrain_directory(&extracted, &library)
        }
        Some("world-terrain") => cooker::cook_world_terrain(&extracted, &library),
        Some("world-script") => cooker::cook_world_script(&extracted, &library),
        Some("world-skies") => cooker::cook_world_skies(&extracted, &library),
        Some("world-terrain-collision") => {
            cooker::cook_world_terrain_collision(&extracted, &library)
        }
        Some("world-terrain-texture") => {
            let number = |at: usize| -> Result<usize> {
                Ok(args[at]
                    .to_str()
                    .context("terrain texture argument is not UTF-8")?
                    .parse()
                    .context("invalid terrain texture argument")?)
            };
            cooker::cook_world_terrain_texture(&extracted, &library, number(3)?, number(4)?)
        }
        Some("world-terrain-visual") => {
            cooker::cook_world_terrain_visual(&extracted, &library, number(&args, 3)?)
        }
        Some("world-terrain-tile") => cooker::cook_world_terrain_tile(
            &extracted,
            &library,
            args[3].to_str().context("terrain owner is not UTF-8")?,
            args.len() == 5,
        ),
        Some("movies") => cooker::cook_movies(&extracted, &library, selection(&args)?),
        _ => anyhow::bail!("unknown asset owner: {:?}", args[0]),
    }
    .with_context(|| format!("cook asset owner {:?}", args[0]))
}
