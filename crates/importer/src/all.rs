//! One fresh library from the native extraction, in final-owner order.
use anyhow::{Context, Result, ensure};
use std::{path::Path, time::Instant};
use symphonia_constants::{ArenaId, BattleSceneId, BattleSkillId, MonsterId};

const PASS_COUNT: usize = 64;

struct PassTiming {
    number: usize,
    name: &'static str,
    started: Instant,
}

impl Drop for PassTiming {
    fn drop(&mut self) {
        eprintln!(
            "[asset type {}/{}] {} took {:.3} seconds",
            self.number,
            PASS_COUNT,
            self.name,
            self.started.elapsed().as_secs_f64()
        );
    }
}

pub fn cook_all(root: &Path, library: &Path) -> Result<()> {
    let executable = root.join("disc1/sys/main.dol");
    ensure!(
        executable.is_file(),
        "missing extracted GameCube input {}; extract disc 1 first with resonance-import extract --disc <disc-1-image> --output {}",
        executable.display(),
        root.join("disc1").display()
    );
    let started = std::time::Instant::now();
    eprintln!("Writing asset library to {}", library.display());
    let mut pass = 0;
    let mut timing = None;
    let mut heading = |name: &'static str| {
        drop(timing.take());
        pass += 1;
        eprintln!("\n[asset type {pass}/{PASS_COUNT}] Cooking {name}");
        timing = Some(PassTiming {
            number: pass,
            name,
            started: Instant::now(),
        });
    };
    heading("Descriptor");
    crate::cook_descriptor(library).context("library descriptor")?;
    heading("Scripts");
    crate::cook_scripts(library).context("script definitions and behaviors")?;
    heading("Game data and interface");
    let mut fields = crate::cook_game_ui(root, library).context("game and interface")?;

    heading("Character models");
    crate::cook_character_models(root, library, None).context("character models and media")?;
    heading("Character attachments");
    crate::cook_character_attachments(root, library, None).context("character attachments")?;
    heading("Field wings");
    crate::cook_field_wings(root, library).context("field wings")?;
    heading("Field models");
    crate::cook_field_models(root, library).context("shared field models")?;
    heading("Native models");
    crate::cook_native_models(root, library).context("native shared and standalone models")?;
    heading("Original models");
    crate::cook_original_models(root, library).context("original model variants")?;
    heading("Native cameras");
    crate::cook_native_cameras(root, library).context("shared cameras")?;
    heading("Mesh mappings");
    crate::cook_mesh_mappings(root, library).context("mesh mappings")?;
    heading("Field overlay media");
    crate::cook_field_overlay_media(root, library).context("field overlays")?;
    heading("Field settings");
    crate::cook_field_settings(root, library).context("field settings")?;
    heading("Field core");
    crate::cook_field_core(root, library, None).context("field scripts and collision")?;

    heading("Figurine media");
    crate::cook_figurine_media(root, library, None).context("figurines")?;
    heading("Battle formations");
    crate::cook_battle_formations(root, library).context("battle formations")?;
    heading("Battle recoil");
    crate::cook_battle_recoil(root, library).context("battle recoil")?;
    heading("Battle defense");
    crate::cook_battle_defense(root, library).context("battle defense")?;
    heading("Battle unison");
    crate::cook_battle_unison(root, library).context("battle unison")?;
    heading("Battle projectiles");
    crate::cook_battle_projectiles(root, library).context("battle projectiles")?;
    heading("Battle trails");
    crate::cook_battle_trails(root, library).context("battle trails")?;
    heading("Battle victory groups");
    crate::cook_battle_victory_groups(root, library).context("battle victory groups")?;
    heading("Battle effects");
    crate::cook_battle_effects(root, library).context("battle effects")?;
    heading("Battle party");
    let costumes =
        crate::cook_battle_party_mappings(root, library).context("battle party mappings")?;
    crate::cook_battle_party_rules(root, library).context("battle party rules")?;
    let characters = (1..=9u8).collect::<Vec<_>>();
    crate::parallel::for_each(&characters, |_, &selected| {
        for (index, &(character, costume)) in costumes.iter().enumerate() {
            if character != selected {
                continue;
            }
            eprintln!(
                "  [{}/{} jobs] Battle party: character {character}, costume {costume}",
                index + 1,
                costumes.len()
            );
            crate::cook_battle_party_model(root, library, character, costume)
                .with_context(|| format!("battle party {character}:{costume}"))?;
        }
        Ok(())
    })?;
    heading("Battle equipment");
    let equipment = crate::cook_battle_equipment_mappings(root, library)
        .context("battle equipment mappings")?;
    for (index, &id) in equipment.iter().enumerate() {
        eprintln!(
            "  [{}/{} jobs] Battle equipment: {id}",
            index + 1,
            equipment.len()
        );
        crate::cook_battle_equipment(root, library, id)
            .with_context(|| format!("battle equipment {id}"))?;
    }
    heading("Battle arenas");
    crate::cook_battle_stage_mappings(root, library).context("battle arena mappings")?;
    crate::parallel::for_each(&ArenaId::ALL, |index, entry| {
        let id = entry.raw();
        eprintln!(
            "  [{}/{} jobs] Battle arenas: {} ({id})",
            index + 1,
            ArenaId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_stage(root, library, id).with_context(|| format!("battle arena {id}"))
    })?;
    heading("Battle scenes");
    crate::cook_battle_scene_mappings(root, library).context("battle scene mappings")?;
    crate::parallel::for_each(&BattleSceneId::ALL, |index, entry| {
        let id = entry.raw();
        eprintln!(
            "  [{}/{} jobs] Battle scenes: {} ({id})",
            index + 1,
            BattleSceneId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_simple_scene(root, library, id)
            .with_context(|| format!("battle scene {id}"))
    })?;
    heading("Battle skills");
    crate::cook_battle_skill_mappings(root, library).context("battle skill mappings")?;
    crate::parallel::for_each(&BattleSkillId::ALL, |index, entry| {
        let id = entry.raw();
        eprintln!(
            "  [{}/{} jobs] Battle skills: {} ({id})",
            index + 1,
            BattleSkillId::ALL.len(),
            entry.resource_name().unwrap_or("unnamed")
        );
        crate::cook_battle_skill_package(root, library, id)
            .with_context(|| format!("battle skill {id}"))
    })?;
    heading("Battle event scripts");
    crate::cook_battle_event_scripts(root, library, None).context("battle event scripts")?;
    heading("Battle event media");
    crate::cook_battle_event_media(root, library, None).context("battle event media")?;
    heading("Battle enemy mappings");
    crate::cook_battle_enemy_mappings(root, library).context("battle enemy mappings")?;
    let enemies = MonsterId::ALL.iter().map(|id| id.raw()).collect::<Vec<_>>();
    heading("Battle enemy media group");
    crate::cook_battle_enemy_media_group(root, library, &enemies).context("battle enemy media")?;
    heading("Battle enemy effects group");
    crate::cook_battle_enemy_effects_group(root, library, &enemies)
        .context("battle enemy effects")?;
    heading("Battle victories");
    crate::cook_battle_victories(root, library).context("battle victory performances")?;
    heading("Enemy rules");
    crate::parallel::for_each(&enemies, |index, &id| {
        eprintln!(
            "  [{}/{} jobs] Enemy rules: {} ({id})",
            index + 1,
            enemies.len(),
            MonsterId::try_from(id)?
                .resource_name()
                .unwrap_or("unnamed")
        );
        crate::cook_battle_enemy_rules(root, library, id)
            .with_context(|| format!("battle enemy {id} rules"))
    })?;

    // Shared motions can reuse the already published party, NPC and battle clips.
    heading("Shared animations");
    crate::cook_shared_animations(root, library, None).context("shared animations")?;
    heading("Original animations");
    crate::cook_original_animations(root, library).context("unassigned native animations")?;
    heading("Shared animation bindings");
    crate::cook_shared_animation_bindings(root, library, None)
        .context("shared animation bindings")?;
    // Field props reuse figurine/enemy media and keep canonical actor publication ordered.
    heading("Field scenery and actors");
    fields.sort_by_key(|id| (!matches!(id, 268 | 279 | 479), *id));
    crate::cook_fields(root, library, &fields).context("field scenery and actors")?;

    heading("Local animation rigs");
    crate::cook_local_animation_rigs(root, library).context("field-local animation rigs")?;

    // The strict field and enemy conversions run before lengthy audio/movie
    // rendering, so an unsupported native package fails without hours of work.
    heading("Music");
    crate::cook_music(root, library, None).context("music")?;
    heading("Speech");
    crate::cook_speech(root, library).context("speech")?;
    heading("Audio catalogue");
    crate::cook_audio_catalogue(root, library).context("sound effects")?;
    heading("Menu audio");
    crate::cook_menu_audio(root, library).context("menu audio")?;
    heading("Mix audio");
    crate::cook_mix_audio(root, library).context("audio mix")?;
    heading("Synthesis audio");
    crate::cook_synthesis_audio(root, library).context("audio synthesis")?;
    heading("Battle audio");
    crate::cook_battle_audio(root, library).context("battle audio")?;
    heading("Movies");
    crate::cook_movies(root, library, None).context("movies")?;
    heading("Skits");
    crate::cook_skits(root, library).context("skits")?;
    heading("Skit portraits");
    crate::cook_skit_portraits(root, library).context("skit portraits")?;
    heading("Shared texture banks");
    crate::cook_shared_texture_banks(root, library).context("shared texture banks")?;
    heading("Field definitions");
    crate::parallel::for_each(&fields, |index, &id| {
        eprintln!(
            "  [{}/{} jobs] Field definitions: {} ({id})",
            index + 1,
            fields.len(),
            crate::field_core::field_name(id as usize)
        );
        crate::cook_field_definition(root, library, id)
            .with_context(|| format!("field {id} definition"))
    })?;

    heading("World terrain");
    crate::cook_world_terrain(root, library).context("world terrain")?;
    heading("World movement");
    crate::cook_world_movement(root, library).context("world movement")?;
    heading("World encounters");
    crate::cook_world_encounters(root, library).context("world encounters")?;
    heading("World dust");
    crate::cook_world_dust(root, library).context("world dust")?;
    heading("World skies");
    crate::cook_world_skies(root, library).context("world skies")?;
    heading("World actors common");
    crate::cook_world_actors_common(root, library).context("common world actors")?;
    heading("World actors party");
    crate::cook_world_actors_party(root, library).context("party world actors")?;
    heading("World landmarks");
    crate::cook_world_landmarks(root, library).context("world landmarks")?;
    heading("World markers");
    crate::cook_world_markers(root, library).context("world markers")?;
    heading("World cinematics");
    crate::cook_world_cinematics(root, library, None).context("world cinematics")?;
    heading("World script");
    crate::cook_world_script(root, library).context("world script")?;
    heading("World definition");
    crate::cook_world_definition(root, library).context("world definition")?;
    drop(heading);
    drop(timing);
    eprintln!(
        "Cook complete in {:.1}s: {}",
        started.elapsed().as_secs_f64(),
        library.display()
    );
    Ok(())
}
