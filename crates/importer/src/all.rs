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
    let started = std::time::Instant::now();
    eprintln!("[cook] Writing asset library to {}", library.display());
    eprintln!("[cook] Descriptor");
    crate::cook_descriptor(library).context("library descriptor")?;
    eprintln!("[cook] Scripts");
    crate::cook_scripts(library).context("script definitions and behaviors")?;
    eprintln!("[cook] Game data and interface");
    let mut fields = crate::cook_game_ui(root, library).context("game and interface")?;

    eprintln!("[cook] Character models");
    crate::cook_character_models(root, library, None).context("character models and media")?;
    eprintln!("[cook] Character attachments");
    crate::cook_character_attachments(root, library, None).context("character attachments")?;
    eprintln!("[cook] Field wings");
    crate::cook_field_wings(root, library).context("field wings")?;
    eprintln!("[cook] Field models");
    crate::cook_field_models(root, library).context("shared field models")?;
    eprintln!("[cook] Native models");
    crate::cook_native_models(root, library).context("native shared and standalone models")?;
    eprintln!("[cook] Original models");
    crate::cook_original_models(root, library).context("original model variants")?;
    eprintln!("[cook] Native cameras");
    crate::cook_native_cameras(root, library).context("shared cameras")?;
    eprintln!("[cook] Mesh mappings");
    crate::cook_mesh_mappings(root, library).context("mesh mappings")?;
    eprintln!("[cook] Field overlay media");
    crate::cook_field_overlay_media(root, library).context("field overlays")?;
    eprintln!("[cook] Field settings");
    crate::cook_field_settings(root, library).context("field settings")?;
    eprintln!("[cook] Field core");
    crate::cook_field_core(root, library, None).context("field scripts and collision")?;

    eprintln!("[cook] Figurine media");
    crate::cook_figurine_media(root, library, None).context("figurines")?;
    eprintln!("[cook] Battle formations");
    crate::cook_battle_formations(root, library).context("battle formations")?;
    eprintln!("[cook] Battle recoil");
    crate::cook_battle_recoil(root, library).context("battle recoil")?;
    eprintln!("[cook] Battle defense");
    crate::cook_battle_defense(root, library).context("battle defense")?;
    eprintln!("[cook] Battle unison");
    crate::cook_battle_unison(root, library).context("battle unison")?;
    eprintln!("[cook] Battle projectiles");
    crate::cook_battle_projectiles(root, library).context("battle projectiles")?;
    eprintln!("[cook] Battle trails");
    crate::cook_battle_trails(root, library).context("battle trails")?;
    eprintln!("[cook] Battle victory groups");
    crate::cook_battle_victory_groups(root, library).context("battle victory groups")?;
    eprintln!("[cook] Battle effects");
    crate::cook_battle_effects(root, library).context("battle effects")?;
    eprintln!("[cook] Battle party models and rules");
    let costumes =
        crate::cook_battle_party_mappings(root, library).context("battle party mappings")?;
    eprintln!("[cook] Battle party rules");
    crate::cook_battle_party_rules(root, library).context("battle party rules")?;
    let costume_count = costumes.len();
    for (index, (character, costume)) in costumes.into_iter().enumerate() {
        eprintln!(
            "[battle party {}/{}] character {character}, costume {costume}",
            index + 1,
            costume_count
        );
        crate::cook_battle_party_model(root, library, character, costume)
            .with_context(|| format!("battle party {character}:{costume}"))?;
    }
    let equipment = crate::cook_battle_equipment_mappings(root, library)
        .context("battle equipment mappings")?;
    for (index, &id) in equipment.iter().enumerate() {
        eprintln!("[battle equipment {}/{}] {id}", index + 1, equipment.len());
        crate::cook_battle_equipment(root, library, id)
            .with_context(|| format!("battle equipment {id}"))?;
    }
    eprintln!("[cook] Battle stage mappings");
    crate::cook_battle_stage_mappings(root, library).context("battle arena mappings")?;
    for (index, entry) in ArenaId::ALL.iter().enumerate() {
        let id = entry.raw();
        eprintln!(
            "[battle arenas {}/{}] {} ({id})",
            index + 1,
            ArenaId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_stage(root, library, id)
            .with_context(|| format!("battle arena {id}"))?;
    }
    eprintln!("[cook] Battle scene mappings");
    crate::cook_battle_scene_mappings(root, library).context("battle scene mappings")?;
    for (index, entry) in BattleSceneId::ALL.iter().enumerate() {
        let id = entry.raw();
        eprintln!(
            "[battle scenes {}/{}] {} ({id})",
            index + 1,
            BattleSceneId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_simple_scene(root, library, id)
            .with_context(|| format!("battle scene {id}"))?;
    }
    eprintln!("[cook] Battle skill mappings");
    crate::cook_battle_skill_mappings(root, library).context("battle skill mappings")?;
    for (index, entry) in BattleSkillId::ALL.iter().enumerate() {
        let id = entry.raw();
        eprintln!(
            "[battle skills {}/{}] {} ({id})",
            index + 1,
            BattleSkillId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_skill_package(root, library, id)
            .with_context(|| format!("battle skill {id}"))?;
    }
    eprintln!("[cook] Battle event scripts");
    crate::cook_battle_event_scripts(root, library, None).context("battle event scripts")?;
    eprintln!("[cook] Battle event media");
    crate::cook_battle_event_media(root, library, None).context("battle event media")?;
    eprintln!("[cook] Battle enemy mappings");
    crate::cook_battle_enemy_mappings(root, library).context("battle enemy mappings")?;
    let enemies = MonsterId::ALL.iter().map(|id| id.raw()).collect::<Vec<_>>();
    eprintln!("[cook] Battle enemy media group");
    crate::cook_battle_enemy_media_group(root, library, &enemies).context("battle enemy media")?;
    eprintln!("[cook] Battle enemy effects group");
    crate::cook_battle_enemy_effects_group(root, library, &enemies)
        .context("battle enemy effects")?;
    eprintln!("[cook] Battle victories");
    crate::cook_battle_victories(root, library).context("battle victory performances")?;
    for (index, &id) in enemies.iter().enumerate() {
        eprintln!(
            "[enemy rules {}/{}] {} ({id})",
            index + 1,
            enemies.len(),
            MonsterId::try_from(id)?
                .resource_name()
                .unwrap_or("unnamed")
        );
        crate::cook_battle_enemy_rules(root, library, id)
            .with_context(|| format!("battle enemy {id} rules"))?;
    }

    // Shared motions can reuse the already published party, NPC and battle clips.
    eprintln!("[cook] Shared animations");
    crate::cook_shared_animations(root, library, None).context("shared animations")?;
    eprintln!("[cook] Original animations");
    crate::cook_original_animations(root, library).context("unassigned native animations")?;
    eprintln!("[cook] Shared animation bindings");
    crate::cook_shared_animation_bindings(root, library, None)
        .context("shared animation bindings")?;
    // Field props reuse figurine and enemy media published above.
    // These canonical props supply motions reused by lower-numbered fields.
    fields.sort_by_key(|id| (!matches!(id, 268 | 279 | 479), *id));
    for (index, &id) in fields.iter().enumerate() {
        eprintln!(
            "[fields {}/{}] {} ({id})",
            index + 1,
            fields.len(),
            crate::field_core::field_name(id as usize)
        );
        let (exe, archive) = crate::field_core::read(root, id)?;
        crate::field_scene::cook_from_source(root, library, id, &exe, &archive)
            .with_context(|| format!("field {id} scene"))?;
        crate::field_actors::cook_from_source(root, library, id, &exe, &archive)
            .with_context(|| format!("field {id} actors"))?;
    }

    eprintln!("[cook] Local animation rigs");
    crate::cook_local_animation_rigs(root, library).context("field-local animation rigs")?;

    // The strict field and enemy conversions run before lengthy audio/movie
    // rendering, so an unsupported native package fails without hours of work.
    eprintln!("[cook] Music");
    crate::cook_music(root, library, None).context("music")?;
    eprintln!("[cook] Speech");
    crate::cook_speech(root, library).context("speech")?;
    eprintln!("[cook] Audio catalogue");
    crate::cook_audio_catalogue(root, library).context("sound effects")?;
    eprintln!("[cook] Menu audio");
    crate::cook_menu_audio(root, library).context("menu audio")?;
    eprintln!("[cook] Mix audio");
    crate::cook_mix_audio(root, library).context("audio mix")?;
    eprintln!("[cook] Synthesis audio");
    crate::cook_synthesis_audio(root, library).context("audio synthesis")?;
    eprintln!("[cook] Battle audio");
    crate::cook_battle_audio(root, library).context("battle audio")?;
    eprintln!("[cook] Movies");
    crate::cook_movies(root, library, None).context("movies")?;
    eprintln!("[cook] Skits");
    crate::cook_skits(root, library).context("skits")?;
    eprintln!("[cook] Skit portraits");
    crate::cook_skit_portraits(root, library).context("skit portraits")?;
    eprintln!("[cook] Shared texture banks");
    crate::cook_shared_texture_banks(root, library).context("shared texture banks")?;
    for (index, &id) in fields.iter().enumerate() {
        eprintln!(
            "[field definitions {}/{}] {} ({id})",
            index + 1,
            fields.len(),
            crate::field_core::field_name(id as usize)
        );
        crate::cook_field_definition(root, library, id)
            .with_context(|| format!("field {id} definition"))?;
    }

    eprintln!("[cook] World terrain");
    crate::cook_world_terrain(root, library).context("world terrain")?;
    eprintln!("[cook] World movement");
    crate::cook_world_movement(root, library).context("world movement")?;
    eprintln!("[cook] World encounters");
    crate::cook_world_encounters(root, library).context("world encounters")?;
    eprintln!("[cook] World dust");
    crate::cook_world_dust(root, library).context("world dust")?;
    eprintln!("[cook] World skies");
    crate::cook_world_skies(root, library).context("world skies")?;
    eprintln!("[cook] World actors common");
    crate::cook_world_actors_common(root, library).context("common world actors")?;
    eprintln!("[cook] World actors party");
    crate::cook_world_actors_party(root, library).context("party world actors")?;
    eprintln!("[cook] World landmarks");
    crate::cook_world_landmarks(root, library).context("world landmarks")?;
    eprintln!("[cook] World markers");
    crate::cook_world_markers(root, library).context("world markers")?;
    eprintln!("[cook] World cinematics");
    crate::cook_world_cinematics(root, library, None).context("world cinematics")?;
    eprintln!("[cook] World script");
    crate::cook_world_script(root, library).context("world script")?;
    eprintln!("[cook] World definition");
    crate::cook_world_definition(root, library).context("world definition")?;
    eprintln!(
        "[cook] Complete in {:.1}s: {}",
        started.elapsed().as_secs_f64(),
        library.display()
    );
    Ok(())
}
