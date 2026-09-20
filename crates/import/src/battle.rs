//! Discover and cook battle resources independently of encounter admission.
mod actor_tables;
pub(crate) mod all;
pub(crate) use actor_tables::cook_all as cook_actor_tables;
mod contact_effects;
pub(crate) use contact_effects::cook_all as cook_contact_effects;
mod contact_sounds;
pub(crate) use contact_sounds::cook_all as cook_contact_sounds;
mod selection;
pub use selection::{CookSelection, Encounter};
mod action_inventory;
mod action_program;
pub(crate) mod animation_table;
pub(crate) use action_program::physical_bundle;
pub(crate) use actions::cook_elemental_spell_parameters;
pub(crate) use actions::cook_enemy_parameters;
pub(crate) use actions::cook_martial_parameters;
pub(crate) use actions::cook_normal_actions;
pub(crate) use actions::cook_ordinary_spell_parameters;
pub(crate) use actions::cook_recovery_parameters;
pub(crate) use actions::cook_stored_spell_parameters;
pub(crate) use actions::cook_summon_parameters;
mod actions;
mod archive_directories;
mod arte_inventory;
pub(crate) use archive_directories::cook as cook_archive_directories;
mod casting_programs;
mod casting_voices;
mod embedded;
pub(crate) use casting_programs::cook as cook_casting_programs;
pub(crate) use casting_voices::cook as cook_casting_voices;
pub(crate) mod audio;
mod effect_inventory;
mod effect_program;
mod effects;
mod enemy_inventory;
mod entrance;
mod formations;
pub(crate) use entrance::cook_all as cook_entrance;
mod inventory;
mod items;
mod motion;
pub(crate) use motion::cook_all as cook_motion;
mod placement;
mod pose;
pub(crate) use placement::cook_all as cook_placement;
mod projectile_modifiers;
mod ui;
mod ui_tables;
pub(crate) use ui_tables::cook_all as cook_ui_tables;
mod message_tables;
pub(crate) use message_tables::cook_all as cook_message_tables;
mod unison;
pub(crate) use unison::cook_parameters as cook_unison_parameters;
mod unison_tables;
pub(crate) use unison_tables::cook_all as cook_unison_tables;
mod unison_opener;
pub(crate) use unison_opener::cook_all as cook_unison_opener;
mod victory_group;
pub(crate) use victory_group::cook_all as cook_victory_groups;
mod visual;

#[cfg(test)]
use crate::{
    compression,
    read::{u16 as half, u32 as word},
};
use crate::{field_preload::Inventory, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::{
        BattleCatalog, DeathStyle, DefenseTraits, ElementAffinity, EnemyData, EnemySpawn,
        EnemyTraits, Formation, FormationPlacement, GeneratedEnemyLayout, GuardTraits, Movement,
        PartyTraits, ReactionTraits, StunMotion, StunTraits,
        actions::{AnimationCommand, CastingConclusion},
        effects::{EffectBank, EffectId},
    },
    field_preload::Role,
    monster::{MONSTER_COUNT, Monster},
};
#[cfg(test)]
use std::fs;
use std::{collections::BTreeSet, path::Path};

pub fn recover_inventory(extracted: &Path, output: &Path) -> Result<()> {
    inventory::write(extracted, output)
}

fn party_traits(
    settings: &[all::PartySettings],
    ids: &[u8],
) -> Result<std::collections::BTreeMap<u8, PartyTraits>> {
    ensure!(
        ids.iter().all(|id| (1..=9).contains(id)),
        "invalid party metadata owner"
    );
    ids.iter()
        .copied()
        .map(|id| {
            let metadata = &settings
                .iter()
                .find(|row| row.character == id)
                .context("missing party settings")?
                .settings;
            contact_effects::require_inert_guard_effect(metadata.model.guard_effect)?;
            Ok((
                id,
                PartyTraits {
                    family: resonance_content::battle::Family(metadata.combat.family),
                    immune_to_unison: metadata.combat.flags & 0x0400_0000 != 0,
                    death: death(metadata, false)?,
                    combo_limit: if id == 1 { 3 } else { 2 },
                    idle_delay: metadata.model.idle_delay,
                    idle_jitter: metadata.model.idle_jitter,
                    turn_divisor: Some(metadata.combat.turn_divisor),
                    normal_facing: Some(if metadata.combat.flags & 0x80 == 0 {
                        resonance_content::battle::NormalFacing::Target
                    } else {
                        resonance_content::battle::NormalFacing::Fixed
                    }),
                    finish_waits_for_animation: metadata.combat.flags & 0x20_0000 != 0,
                    reaction: reaction(metadata)?,
                    defense: defense(metadata),
                    stun: stun(metadata),
                    conditions: conditions(metadata)?,
                    guard_reduction_percent: metadata.combat.guard_reduction,
                    hit_radius_scale: metadata.model.model_scale,
                    effect_scale: metadata.model.effect_scale,
                    overlimit_scale: metadata.effects.overlimit_scale,
                    casting_base: metadata.casting.base_ticks,
                    casting_conclusion: Some(CastingConclusion {
                        animation: AnimationCommand::Play {
                            clip: 12,
                            blend: metadata.casting.resume_blend_ticks,
                            start: 4,
                            end: None,
                            layer: 8,
                            looping: metadata.casting.release_looping,
                            mirror: false,
                            resource: -1,
                            rate: metadata.casting.animation_rate,
                        },
                        loop_start: metadata.casting.resume_loop_start,
                    }),
                    effect_offset: metadata.model.effect_offset,
                    movement: movement(metadata),
                },
            ))
        })
        .collect()
}

pub fn cook(extracted: &Path, output: &Path, coefficients: &Path) -> Result<()> {
    cook_selected(extracted, output, coefficients, &CookSelection::default())
}

pub fn cook_selected(
    extracted: &Path,
    output: &Path,
    coefficients: &Path,
    selection: &CookSelection,
) -> Result<()> {
    selection.validate()?;
    let data = output.join("data");
    let entrance: resonance_content::battle::entrance::EntranceRecipe =
        crate::embedded::read(&data, "battle-entrance", "US_r_Top2Btl.rel")?;
    entrance.validate()?;
    let contact_effects = crate::embedded::read::<contact_effects::ContactEffectTables>(
        &data,
        "battle-contact-effects",
        "US_r_Top2Btl.rel",
    )?
    .bind();
    let actor_tables = crate::embedded::read::<actor_tables::ActorTables>(
        &data,
        "battle-actor-tables",
        "US_r_Top2Btl.rel",
    )?;
    let (party_layout, generated_layout) = crate::embedded::read::<placement::PlacementTables>(
        &data,
        "battle-placement",
        "US_r_Top2Btl.rel",
    )?
    .bind()?;
    let motion_tables =
        crate::embedded::read::<motion::MotionTables>(&data, "battle-motion", "US_r_Top2Btl.rel")?;
    let motion = motion_tables.bind()?;
    let disc = crate::disc_number(extracted)?;
    let sources = all::Sources::cooked(output, disc)?;
    let velocity_reset_scale = motion_tables.projectile_velocity_reset_scale()?;
    let formations = selected_formations(
        &formations::FormationTable::bind(output, disc, &sources)?,
        &selection.encounters,
        Some(&generated_layout),
    )?;
    let enemy_ids: Vec<_> = formations
        .iter()
        .flat_map(|f| f.enemies.iter().map(|e| e.monster))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let arenas: Vec<_> = formations
        .iter()
        .map(|f| f.arena)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let enemy_source = crate::cooked::Source::open(output, disc, &sources.enemy)?;
    let enemies: std::collections::BTreeMap<_, _> = enemy_ids
        .iter()
        .map(|&id| {
            Ok((
                id,
                published_enemy_data(&enemy_source, id)
                    .with_context(|| format!("monster {id} combat data"))?,
            ))
        })
        .collect::<Result<_>>()?;
    for formation in &formations {
        if let FormationPlacement::Generated { layout } = &formation.placement {
            let rows = formation
                .enemies
                .iter()
                .map(|spawn| {
                    enemies
                        .get(&spawn.monster)
                        .context("missing generated enemy data")?
                        .placement_row
                        .context("missing generated enemy placement row")
                })
                .collect::<Result<Vec<_>>>()?;
            layout.validate(&rows)?;
        }
    }
    let party_settings = crate::cooked::Source::open(output, disc, "US_r_Top2Btl.rel")?
        .embedded::<Vec<all::PartySettings>>("battle-party-settings", "US_r_Top2Btl.rel")?;
    let mut party = party_traits(&party_settings, &selection.party)?;
    let arte_catalogue = crate::arte::cooked(&data)?;
    let mut active_level_techniques = BTreeSet::new();
    for character in 1..=embedded::PARTY_COUNT {
        for &id in arte_catalogue.learned_by(character)? {
            if arte_catalogue.definition(usize::from(id))?.flags & 0x8000_0000 != 0 {
                active_level_techniques.insert(u16::from(id));
            }
        }
    }
    let mut shared_contact = None;
    let actions = actions::cook_selected(
        extracted,
        &data,
        &arte_catalogue,
        &motion_tables,
        selection,
        &enemy_ids,
        &mut || {
            if shared_contact.is_none() {
                shared_contact = Some(effects::bind_one(
                    output,
                    disc,
                    &sources,
                    EffectId {
                        bank: EffectBank::Techniques,
                        id: 1,
                    },
                    velocity_reset_scale,
                )?);
            }
            Ok(shared_contact.as_ref().unwrap().clone())
        },
    )?;
    if actions
        .techniques
        .iter()
        .any(|technique| (284..=293).contains(&technique.native_id))
    {
        visual::binding::require_null_party_motion(output, disc, &sources, 5, 13)?;
    }
    let unison = unison::cook(
        &unison::Inputs::bind(output, disc, &sources)?,
        &crate::embedded::read(&data, "battle-unison-tables", "US_r_Top2Btl.rel")?,
        &crate::embedded::read(&data, "battle-unison-opener", "US_r_Top2Btl.rel")?,
        &arte_catalogue,
    )?;
    let mut dependencies = selection::Dependencies::actions(&actions)?;
    dependencies.programs.extend(contact_effects.programs());
    dependencies.unison(&unison, &selection.party)?;
    let items = items::cook(&crate::item::cooked(&data)?, &actor_tables)?;
    dependencies.programs.extend(items.effects());
    if selection.party.contains(&1) && selection.party.contains(&2) {
        dependencies.programs.extend([0, 1, 2, 3, 5].map(|id| {
            resonance_content::battle::effects::EffectId {
                bank: resonance_content::battle::effects::EffectBank::Magic(101),
                id,
            }
        }));
    }
    let swords = [1, 6, 9]
        .into_iter()
        .filter(|id| selection.party.contains(id))
        .count();
    for (package, count, enabled) in [
        (102, 2, swords >= 2),
        (103, 2, swords > 0 && selection.party.contains(&5)),
        (104, 3, swords > 0 && selection.party.contains(&5)),
    ] {
        if enabled {
            dependencies.programs.extend((0..count).map(|id| {
                resonance_content::battle::effects::EffectId {
                    bank: resonance_content::battle::effects::EffectBank::Magic(package),
                    id,
                }
            }));
        }
    }
    for (package, effects, enabled) in [
        (
            112,
            [2, 3, 5],
            selection.party.contains(&7) && selection.party.contains(&2),
        ),
        (
            114,
            [2, 4, 5],
            [6, 9].iter().any(|id| selection.party.contains(id)) && selection.party.contains(&2),
        ),
    ] {
        if enabled {
            dependencies.programs.extend(effects.map(|id| {
                resonance_content::battle::effects::EffectId {
                    bank: resonance_content::battle::effects::EffectBank::Magic(package),
                    id,
                }
            }));
        }
    }
    dependencies
        .programs
        .insert(resonance_content::battle::effects::EffectId {
            bank: resonance_content::battle::effects::EffectBank::Common,
            id: 44,
        });
    dependencies.programs.extend((45..=47).map(|id| {
        resonance_content::battle::effects::EffectId {
            bank: resonance_content::battle::effects::EffectBank::Common,
            id,
        }
    }));
    dependencies.programs.extend(&selection.effects);
    let effects = effects::bind(
        output,
        disc,
        &sources,
        &dependencies.projectiles,
        velocity_reset_scale,
    )?;
    let projectile_modifiers =
        projectile_modifiers::bind(output, disc, &sources.enemy, &actions.enemies)?;
    dependencies.projectiles(&effects, &projectile_modifiers)?;
    let mut effect_actors: BTreeSet<_> = selection.effect_actors.iter().copied().collect();
    effect_actors.insert(resonance_content::battle::effects::EffectId {
        bank: resonance_content::battle::effects::EffectBank::Common,
        id: 19,
    });
    let effect_programs =
        effect_program::cook(extracted, output, &dependencies.programs, &effect_actors)?;
    let victory_groups = victory_group::cook(extracted)?;
    let (mut sounds, mut voices) = actions.audio_ids();
    for &character in &selection.party {
        let (opener_sounds, opener_voices) =
            unison.opener[usize::from(character - 1)].action.audio_ids();
        sounds.extend(opener_sounds);
        voices.extend(opener_voices);
        voices.insert(unison.combined_prelude.voices[usize::from(character - 1)]);
    }
    sounds.extend([73, 77, resonance_content::battle::entrance::SHATTER_SOUND]);
    for (kind, program) in &unison.pow {
        if !kind.selected(&selection.party) {
            continue;
        }
        for phase in &program.phases {
            let (phase_sounds, phase_voices) = phase.action.audio_ids();
            sounds.extend(phase_sounds);
            voices.extend(phase_voices);
        }
        sounds.insert(126);
    }
    for (kind, program) in &unison.thrusts {
        if !kind.selected(&selection.party) {
            continue;
        }
        voices.extend(kind.voices());
        for phase in &program.phases {
            let (phase_sounds, phase_voices) = phase.action.audio_ids();
            sounds.extend(phase_sounds);
            voices.extend(phase_voices);
        }
    }
    for (kind, program) in &unison.strikes {
        if !kind.selected(&selection.party) {
            continue;
        }
        if program.projectile_rule.sound != 0 {
            sounds.insert(program.projectile_rule.sound);
        }
        for phase in &program.phases {
            let (phase_sounds, phase_voices) = phase.action.audio_ids();
            sounds.extend(phase_sounds);
            voices.extend(phase_voices);
        }
    }
    for kind in resonance_content::battle::unison::CombinedPair::ALL {
        let Some(program) = unison
            .pair_program(kind)
            .filter(|_| kind.selected(&selection.party))
        else {
            continue;
        };
        voices.extend(program.voice.ids());
        for phase in &program.phases {
            let (phase_sounds, phase_voices) = phase.action.audio_ids();
            sounds.extend(phase_sounds);
            voices.extend(phase_voices);
        }
        sounds.extend(
            program
                .projectiles
                .iter()
                .flatten()
                .map(|p| p.rule.sound)
                .filter(|&id| id != 0),
        );
    }
    sounds.insert(117);
    sounds.extend(
        effect_programs
            .programs
            .iter()
            .flat_map(|p| &p.emissions)
            .filter_map(|e| match e.command {
                resonance_content::battle::effect_program::EffectCommand::Sound {
                    sound, ..
                } => Some(sound),
                _ => None,
            }),
    );
    voices.extend(victory_groups.voices());
    voices.extend(
        selection
            .party
            .iter()
            .map(|&character| unison.party[usize::from(character - 1)].overlimit_voice),
    );
    if selection.party.contains(&7) {
        voices.insert(806);
    }
    if selection.party.contains(&4) {
        voices.insert(377);
    }
    let characters = actions.party.iter().map(|a| a.character).collect();
    let audio = audio::cook(
        extracted,
        output,
        coefficients,
        &sounds,
        &voices,
        &characters,
        &enemy_ids,
    )?;
    let visuals = visual::cook(
        extracted,
        output,
        &arenas,
        &selection.party,
        &enemy_ids,
        &selection.weapons,
        &effect_programs,
    )?;
    for (&id, traits) in &mut party {
        traits.stun.head_bone = visuals
            .party
            .get(&id)
            .context("missing stun model")?
            .standard()?
            .head_bone;
    }
    visual::validate_martial(&arte_catalogue, &actions, &visuals)?;
    dependencies.validate(&effect_programs, &visuals)?;
    effect_programs.validate_models(&visuals.effect_models)?;
    crate::monsters::cook(extracted, output, &enemy_ids)?;
    let ui = ui::cook(extracted, output, &enemy_ids)?;

    let mut inventory = Inventory::new(output);
    let monsters: Vec<Monster> = enemy_ids
        .iter()
        .map(|id| inventory.json(&format!("monsters/{id:03}.json"), None, Role::Data))
        .collect::<Result<_>>()?;
    for scene in visuals.scenes().chain(
        monsters
            .iter()
            .flat_map(|m| m.preview.parts.iter().map(|p| &p.scene)),
    ) {
        inventory.add(&scene.mesh, None, Role::Mesh)?;
        for texture in &scene.textures {
            inventory.add(texture, None, Role::Texture)?;
        }
    }
    inventory.add(&visuals.toon_ramp, None, Role::Texture)?;
    inventory.add(&visuals.shadow_texture, None, Role::Texture)?;
    for texture in visuals.trail_textures() {
        inventory.add(&texture.path, None, Role::Texture)?;
    }
    for path in effect_programs.assets() {
        inventory.add(path, None, Role::Texture)?;
    }
    for path in ui.assets() {
        inventory.add(
            path,
            None,
            if path.ends_with(".ktx2") {
                Role::Texture
            } else {
                Role::Data
            },
        )?;
    }
    inventory.audio_assets(&audio.audio)?;
    let catalog = BattleCatalog {
        version: BattleCatalog::VERSION,
        formations,
        enemies,
        party,
        party_layout,
        entrance,
        motion,
        contact_effects,
        active_level_techniques,
        actions,
        effects,
        effect_programs,
        projectile_modifiers,
        visuals,
        audio,
        ui,
        items,
        unison,
        victory_groups,
        files: inventory.into_files(),
    };
    catalog.validate(&monsters)?;
    write_atomic(
        &output.join(BattleCatalog::PATH),
        &serde_json::to_vec_pretty(&catalog)?,
    )?;
    println!(
        "Cooked {} battle formations, {} dependencies",
        catalog.formations.len(),
        catalog.files.len()
    );
    Ok(())
}

fn selected_formations(
    table: &formations::FormationTable,
    encounters: &[Encounter],
    generated_layout: Option<&GeneratedEnemyLayout>,
) -> Result<Vec<Formation>> {
    encounters
        .iter()
        .map(
            |&Encounter {
                 formation: id,
                 arena,
             }| {
                formation(
                    table
                        .formations
                        .get(usize::from(id))
                        .context("missing selected formation")?,
                    id,
                    arena,
                    generated_layout,
                )
            },
        )
        .collect()
}

fn formation(
    row: &formations::Record,
    id: u16,
    arena: u16,
    generated_layout: Option<&GeneratedEnemyLayout>,
) -> Result<Formation> {
    let count = usize::from(row.actor_count);
    let unique = usize::from(row.resource_count);
    ensure!(
        (1..=8).contains(&count) && (1..=4).contains(&unique),
        "formation {id}: invalid actor count {count} or resource count {unique}"
    );
    const ESCAPE_DISABLED: u8 = 1;
    const GENERATED: u8 = 2;
    const EXPLICIT_ENCOUNTER: u8 = 4;
    const VICTORY_MUSIC_DISABLED: u8 = 0x10;
    const OPENING_VOICE_DISABLED: u8 = 0x40;
    const VICTORY_CELEBRATION_DISABLED: u8 = 0x80;
    // Authored only in formation25; no battle header consumer reads this bit.
    const UNUSED: u8 = 8;
    // The alternate result staging mode (0x20) still needs its full consumer.
    ensure!(
        matches!(
            row.flags
                & !(ESCAPE_DISABLED
                    | VICTORY_MUSIC_DISABLED
                    | OPENING_VOICE_DISABLED
                    | VICTORY_CELEBRATION_DISABLED
                    | UNUSED),
            GENERATED | EXPLICIT_ENCOUNTER
        ) && usize::from(row.hidden_names) < 1 << unique,
        "formation {id}: unsupported flags {:#04x}, hidden-name mask {:#04x} (actors {count}, resources {unique})",
        row.flags,
        row.hidden_names
    );
    let placement = if row.flags & GENERATED != 0 {
        FormationPlacement::Generated {
            layout: generated_layout
                .context("missing generated enemy layout")?
                .clone(),
        }
    } else {
        FormationPlacement::Explicit
    };
    ensure!(
        row.actors.iter().all(|actor| actor.attachments == [0; 2]),
        "formation {id}: attachment overrides are not implemented"
    );
    let enemies = row.actors[..count]
        .iter()
        .map(|actor| {
            let slot = usize::from(actor.resource);
            ensure!(slot < unique, "formation refers to a missing enemy slot");
            let monster = row.resources[slot];
            ensure!(
                usize::try_from(monster).is_ok_and(|id| id < MONSTER_COUNT),
                "formation has an unknown monster"
            );
            Ok(EnemySpawn {
                monster: monster as u8,
                variant: actor.variant,
                texture_variant: actor.appearance,
                name_visible: row.hidden_names & (1 << slot) == 0,
                position: actor.position.map(f32::from),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Formation {
        id,
        arena,
        placement,
        escape_allowed: row.flags & ESCAPE_DISABLED == 0,
        opening_voice: row.flags & OPENING_VOICE_DISABLED == 0,
        victory_music: row.flags & VICTORY_MUSIC_DISABLED == 0,
        victory_camera: row.flags & 0x20 == 0,
        victory_celebration: row.flags & (0x20 | VICTORY_CELEBRATION_DISABLED) == 0,
        enemies,
    })
}

fn published_enemy_data(source: &crate::cooked::Source<'_>, id: u8) -> Result<EnemyData> {
    let read = |name| {
        source
            .resolve(&format!("battle/all/enemy-{id}/{name}.json"))
            .map(|(_, bytes)| bytes)
    };
    let settings: all::ActorSettings = serde_json::from_slice(&read("header-4")?)?;
    let statistics = serde_json::from_slice(&read("header-6")?)?;
    let resources = serde_json::from_slice(&read("header-14")?)?;
    let variants = if settings.combat.variant_count == 0 {
        Vec::new()
    } else {
        serde_json::from_slice::<all::EnemyVariants>(&read("variants")?)?.variants
    };
    enemy_data(&settings, &statistics, &resources, &variants)
}

fn enemy_data(
    metadata: &all::ActorSettings,
    stats: &all::EnemyStatistics,
    resources: &all::EnemyResources,
    variant_rows: &[all::EnemyVariant],
) -> Result<EnemyData> {
    let count = usize::from(metadata.combat.variant_count);
    ensure!(count < 16, "invalid enemy variant count");
    ensure!(count == variant_rows.len(), "incomplete enemy variants");
    ensure!(
        count == 0 || resources.variant_offset >= 0x1e8,
        "missing enemy variant table"
    );
    ensure!(
        resources.motion_offsets.len() == 80,
        "incomplete enemy motion declarations"
    );
    let variants = std::iter::once(&stats.combat)
        .chain(variant_rows.iter().map(|row| &row.combat))
        .map(enemy_traits)
        .collect::<Result<_>>()?;
    let mut affinities = [ElementAffinity::Neutral; 8];
    for (affinity, &value) in affinities
        .iter_mut()
        .zip(&metadata.combat.element_affinities)
    {
        *affinity = value.try_into()?;
    }
    contact_effects::require_inert_guard_effect(metadata.model.guard_effect)?;
    ensure!(
        metadata.combat.guard_pressure_limit <= i16::MAX as u16,
        "negative battle statistic"
    );
    Ok(EnemyData {
        family: resonance_content::battle::Family(metadata.combat.family),
        immune_to_unison: metadata.combat.flags & 0x0400_0000 != 0,
        concealed_name: "？".repeat(
            stats
                .name_bytes
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated enemy name")?
                / 2,
        ),
        opening_warning: metadata.effects.opening_warning >= 3,
        death: death(metadata, true)?,
        target_policy: stats.target_policy.try_into()?,
        placement_row: Some(stats.placement_row.try_into()?),
        manual_target_excluded: metadata.model.body_flags & 0x200 != 0,
        flying: metadata.combat.flags & 1 != 0,
        idle_delay: metadata.model.idle_delay,
        idle_jitter: metadata.model.idle_jitter,
        finish_waits_for_animation: metadata.combat.flags & 0x20_0000 != 0,
        reaction: reaction(metadata)?,
        defense: defense(metadata),
        stun: StunTraits {
            motion: if resources.motion_offsets[21] == 0 {
                StunMotion::KeepCurrent
            } else {
                StunMotion::Loop
            },
            ..stun(metadata)
        },
        conditions: conditions(metadata)?,
        overlimit_rate: metadata.effects.overlimit_rate,
        overlimit_initial: metadata.effects.overlimit_initial,
        overlimit_bone: metadata.effects.overlimit_bone,
        overlimit_scale: metadata.effects.overlimit_scale,
        drop_chances: metadata.rewards.drop_chances,
        steal_chance: metadata.rewards.steal_chance,
        grade: metadata.rewards.grade,
        variants,
        guard: GuardTraits {
            reduction_percent: metadata.combat.guard_reduction,
            pressure_limit: metadata.combat.guard_pressure_limit,
        },
        hit_radius_scale: metadata.model.model_scale,
        effect_scale: metadata.model.effect_scale,
        ground_offset: metadata.model.ground_offset,
        effect_offset: metadata.model.effect_offset,
        movement: movement(metadata),
        physical_affinity: metadata.combat.physical_affinity.try_into()?,
        affinities,
    })
}

fn death(metadata: &all::ActorSettings, enemy: bool) -> Result<DeathStyle> {
    let flags = metadata.combat.flags;
    let clip = metadata.model.death_clip;
    if clip == 0 && flags & 0x40000 == 0 {
        ensure!(enemy, "unsupported disappearing party defeat");
        return Ok(DeathStyle::Fade);
    }
    ensure!(
        flags & 0x800 == 0,
        "motion-driven defeat is not implemented"
    );
    if enemy {
        ensure!(
            clip == 0,
            "custom enemy collapse shading is not implemented"
        );
        return Ok(DeathStyle::Corpse);
    }
    Ok(DeathStyle::Collapse {
        clip: if clip == 0 { 7 } else { clip },
    })
}

fn enemy_traits(stats: &all::CombatStats) -> Result<EnemyTraits> {
    let positive = |value: i16| u16::try_from(value).context("negative battle statistic");
    Ok(EnemyTraits {
        level: stats.level,
        thrust: positive(stats.thrust)?,
        intelligence: positive(stats.intelligence)?,
        accuracy: positive(stats.accuracy)?,
        evasion: positive(stats.evasion)?,
        luck: stats.luck,
    })
}

fn movement(metadata: &all::ActorSettings) -> Movement {
    Movement {
        walk_speed: metadata.combat.walk_speed,
        run_speed: metadata.combat.run_speed,
        body: Some(resonance_content::battle::BodyTraits {
            resting_height: metadata.model.ground_offset,
            freeze_height: metadata.model.body_flags & 0x4000 != 0,
            immovable: metadata.combat.flags & 0x1000_0000 != 0,
            excluded: metadata.model.body_flags & 0x100 != 0,
            ignores_boundary: metadata.combat.flags & 0x4000_0000 != 0,
            pairing: match metadata.model.body_flags & 0x21 {
                0 => resonance_content::battle::BodyPairing::Independent,
                1 => resonance_content::battle::BodyPairing::Primary,
                0x20 => resonance_content::battle::BodyPairing::Secondary,
                _ => resonance_content::battle::BodyPairing::Combined,
            },
        }),
        gravity: if metadata.combat.flags & 1 != 0 {
            0.
        } else {
            -1.
        },
    }
}

fn reaction(metadata: &all::ActorSettings) -> Result<ReactionTraits> {
    let flags = metadata.combat.flags;
    Ok(ReactionTraits {
        weight: metadata.combat.weight.try_into()?,
        restrain_launch: flags & 0x200 != 0,
        unlaunchable: flags & 0x0200_0000 != 0,
        no_knockback: flags & 0x400 != 0,
    })
}

fn defense(metadata: &all::ActorSettings) -> DefenseTraits {
    let flags = metadata.combat.flags;
    DefenseTraits {
        poise: metadata.combat.poise,
        stun_resistance: metadata.combat.stun_resistance,
        stagger_threshold: metadata.combat.stagger_threshold,
        stagger_ticks: metadata.combat.stagger_ticks,
        auto_guard_disabled: flags & 0x4000 != 0,
        fixed_one_damage: flags & 4 != 0,
        quarter_damage: metadata.model.body_flags & 0x3000 != 0,
    }
}

fn conditions(
    metadata: &all::ActorSettings,
) -> Result<resonance_content::battle::conditions::ConditionTraits> {
    use resonance_content::battle::conditions::{Condition, ConditionTraits};
    let [high, low] = metadata.combat.intrinsic_conditions;
    let intrinsic = u64::from(high) << 32 | u64::from(low);
    const CHANCE_RESISTANCE: u64 = 1 << 41;
    ensure!(
        intrinsic & !(Condition::MASK | CHANCE_RESISTANCE) == 0,
        "unsupported intrinsic battle conditions {intrinsic:#x}"
    );
    Ok(ConditionTraits {
        immunity: u64::from(metadata.combat.condition_immunity[0]) << 32
            | u64::from(metadata.combat.condition_immunity[1]),
        intrinsic: intrinsic & Condition::MASK,
        chance_resistance: intrinsic & CHANCE_RESISTANCE != 0,
        paralysis_face: metadata.appearance.paralysis_face,
    })
}

fn stun(metadata: &all::ActorSettings) -> StunTraits {
    StunTraits {
        motion: StunMotion::Loop,
        immune: metadata.combat.condition_immunity[1] & 4 != 0,
        half_duration: metadata.combat.condition_immunity[0] & 0x100 != 0,
        head_bone: u16::from(metadata.model.head_bone),
        offset: metadata.model.stun_offset,
        face: metadata.appearance.stun_face,
        idle_face: metadata.appearance.idle_face,
    }
}

#[cfg(test)]
mod tests;
