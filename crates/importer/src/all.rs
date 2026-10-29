//! One fresh library from the native extraction, in final-owner order.
use anyhow::{Context, Result, ensure};
use std::path::Path;
use symphonia_constants::{ArenaId, BattleSceneId, BattleSkillId, MonsterId};

pub fn cook_all(root: &Path, library: &Path) -> Result<()> {
    let executable = root.join("disc1/sys/main.dol");
    ensure!(
        executable.is_file(),
        "missing extracted GameCube input {}; extract disc 1 first with resonance-import extract --disc <disc-1-image> --output {}",
        executable.display(),
        root.join("disc1").display()
    );
    crate::cook_descriptor(library).context("library descriptor")?;
    crate::cook_scripts(library).context("script definitions and behaviors")?;
    let mut fields = crate::cook_game_ui(root, library).context("game and interface")?;

    crate::cook_character_models(root, library, None).context("character models and media")?;
    crate::cook_character_attachments(root, library, None).context("character attachments")?;
    crate::cook_field_wings(root, library).context("field wings")?;
    crate::cook_field_models(root, library).context("shared field models")?;
    crate::cook_native_models(root, library).context("native shared and standalone models")?;
    crate::cook_original_models(root, library).context("original model variants")?;
    crate::cook_native_cameras(root, library).context("shared cameras")?;
    crate::cook_mesh_mappings(root, library).context("mesh mappings")?;
    crate::cook_field_overlay_media(root, library).context("field overlays")?;
    crate::cook_field_settings(root, library).context("field settings")?;
    crate::cook_field_core(root, library, None).context("field scripts and collision")?;

    crate::cook_figurine_media(root, library, None).context("figurines")?;
    crate::cook_battle_formations(root, library).context("battle formations")?;
    crate::cook_battle_recoil(root, library).context("battle recoil")?;
    crate::cook_battle_defense(root, library).context("battle defense")?;
    crate::cook_battle_unison(root, library).context("battle unison")?;
    crate::cook_battle_projectiles(root, library).context("battle projectiles")?;
    crate::cook_battle_trails(root, library).context("battle trails")?;
    crate::cook_battle_victory_groups(root, library).context("battle victory groups")?;
    crate::cook_battle_effects(root, library).context("battle effects")?;
    let costumes =
        crate::cook_battle_party_mappings(root, library).context("battle party mappings")?;
    crate::cook_battle_party_rules(root, library).context("battle party rules")?;
    for (character, costume) in costumes {
        crate::cook_battle_party_model(root, library, character, costume)
            .with_context(|| format!("battle party {character}:{costume}"))?;
    }
    for id in
        crate::cook_battle_equipment_mappings(root, library).context("battle equipment mappings")?
    {
        crate::cook_battle_equipment(root, library, id)
            .with_context(|| format!("battle equipment {id}"))?;
    }
    crate::cook_battle_stage_mappings(root, library).context("battle arena mappings")?;
    for id in ArenaId::ALL {
        let id = id.raw();
        crate::cook_battle_stage(root, library, id)
            .with_context(|| format!("battle arena {id}"))?;
    }
    crate::cook_battle_scene_mappings(root, library).context("battle scene mappings")?;
    for id in BattleSceneId::ALL {
        let id = id.raw();
        crate::cook_battle_simple_scene(root, library, id)
            .with_context(|| format!("battle scene {id}"))?;
    }
    crate::cook_battle_skill_mappings(root, library).context("battle skill mappings")?;
    for id in BattleSkillId::ALL {
        let id = id.raw();
        crate::cook_battle_skill_package(root, library, id)
            .with_context(|| format!("battle skill {id}"))?;
    }
    crate::cook_battle_event_scripts(root, library, None).context("battle event scripts")?;
    crate::cook_battle_event_media(root, library, None).context("battle event media")?;
    crate::cook_battle_enemy_mappings(root, library).context("battle enemy mappings")?;
    let enemies = MonsterId::ALL.iter().map(|id| id.raw()).collect::<Vec<_>>();
    crate::cook_battle_enemy_media_group(root, library, &enemies).context("battle enemy media")?;
    crate::cook_battle_enemy_effects_group(root, library, &enemies)
        .context("battle enemy effects")?;
    crate::cook_battle_victories(root, library).context("battle victory performances")?;
    for id in enemies {
        crate::cook_battle_enemy_rules(root, library, id)
            .with_context(|| format!("battle enemy {id} rules"))?;
    }

    // Shared motions can reuse the already published party, NPC and battle clips.
    crate::cook_shared_animations(root, library, None).context("shared animations")?;
    crate::cook_original_animations(root, library).context("unassigned native animations")?;
    crate::cook_shared_animation_bindings(root, library, None)
        .context("shared animation bindings")?;
    // Field props reuse figurine and enemy media published above.
    // These canonical props supply motions reused by lower-numbered fields.
    fields.sort_by_key(|id| (!matches!(id, 268 | 279 | 479), *id));
    for &id in &fields {
        let (exe, archive) = crate::field_core::read(root, id)?;
        crate::field_scene::cook_from_source(root, library, id, &exe, &archive)
            .with_context(|| format!("field {id} scene"))?;
        crate::field_actors::cook_from_source(root, library, id, &exe, &archive)
            .with_context(|| format!("field {id} actors"))?;
    }

    crate::cook_local_animation_rigs(root, library).context("field-local animation rigs")?;

    // The strict field and enemy conversions run before lengthy audio/movie
    // rendering, so an unsupported native package fails without hours of work.
    crate::cook_music(root, library, None).context("music")?;
    crate::cook_speech(root, library).context("speech")?;
    crate::cook_audio_catalogue(root, library).context("sound effects")?;
    crate::cook_menu_audio(root, library).context("menu audio")?;
    crate::cook_mix_audio(root, library).context("audio mix")?;
    crate::cook_synthesis_audio(root, library).context("audio synthesis")?;
    crate::cook_battle_audio(root, library).context("battle audio")?;
    crate::cook_movies(root, library, None).context("movies")?;
    crate::cook_skits(root, library).context("skits")?;
    crate::cook_skit_portraits(root, library).context("skit portraits")?;
    crate::cook_shared_texture_banks(root, library).context("shared texture banks")?;
    for id in fields {
        crate::cook_field_definition(root, library, id)
            .with_context(|| format!("field {id} definition"))?;
    }

    crate::cook_world_terrain(root, library).context("world terrain")?;
    crate::cook_world_movement(root, library).context("world movement")?;
    crate::cook_world_encounters(root, library).context("world encounters")?;
    crate::cook_world_dust(root, library).context("world dust")?;
    crate::cook_world_skies(root, library).context("world skies")?;
    crate::cook_world_actors_common(root, library).context("common world actors")?;
    crate::cook_world_actors_party(root, library).context("party world actors")?;
    crate::cook_world_landmarks(root, library).context("world landmarks")?;
    crate::cook_world_markers(root, library).context("world markers")?;
    crate::cook_world_cinematics(root, library, None).context("world cinematics")?;
    crate::cook_world_script(root, library).context("world script")?;
    crate::cook_world_definition(root, library).context("world definition")?;
    Ok(())
}
