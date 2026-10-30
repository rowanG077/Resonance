//! Capture an entire encounter from its effective named library. Every dynamic
//! choice belongs to this candidate, including reserves and transferable gear.
use super::*;
use resonance_content::{
    battle_effect::document as effects,
    battle_model::document as models,
    battle_scene::document::{self as scenes, Kind},
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::JSON_LIMIT,
};

impl Assets {
    /// Bind every action-selected sound before the presentation worker captures
    /// its PCM. The same catalogue supplies both admission and playback paths.
    pub fn prepare_named(
        self,
        menus: &MenuData,
        options: PrepareOptions,
        catalogue: &resonance_content::battle_audio::document::Catalogue,
    ) -> Result<(Assets, Prepared, Selection)> {
        let mut selected = self.event_audio.clone();
        let prepared = self.prepare(menus, options, |sound| {
            catalogue.collect(&mut selected, self.files.diagnostics(), sound)
        })?;
        selected.music.insert(prepared.music);
        selected
            .music
            .insert(crate::battle::lifecycle::DEFEAT_MUSIC);
        if prepared.results.style.play_music {
            selected
                .music
                .insert(crate::battle::lifecycle::VICTORY_MUSIC);
        }
        Ok((self, prepared, selected))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "One preparation borrows the retained session and its candidate snapshot."
    )]
    pub fn capture(
        snapshot: &mut Snapshot,
        menus: &MenuData,
        session: &SessionData,
        party: &Party,
        setup: Setup,

        diagnostics: Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        Self::capture_inner(
            snapshot,
            menus,
            session,
            party,
            setup,
            diagnostics,
            cancelled,
            false,
        )
    }

    /// Observe one action without requiring its surrounding story event.
    #[cfg(feature = "test-support")]
    #[expect(
        clippy::too_many_arguments,
        reason = "One preparation borrows the retained session and its candidate snapshot."
    )]
    pub fn capture_fixture(
        snapshot: &mut Snapshot,
        menus: &MenuData,
        session: &SessionData,
        party: &Party,
        setup: Setup,

        diagnostics: Diagnostics,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        Self::capture_inner(
            snapshot,
            menus,
            session,
            party,
            setup,
            diagnostics,
            cancelled,
            true,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "One preparation borrows the retained session and its candidate snapshot."
    )]
    fn capture_inner(
        snapshot: &mut Snapshot,
        menus: &MenuData,
        session: &SessionData,
        party: &Party,
        setup: Setup,

        diagnostics: Diagnostics,
        cancelled: &impl Fn() -> bool,
        isolated_fixture: bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "battle preparation cancelled");
        // Battle menus, fatal-load slots and event dialogue share the field UI.
        // Capture their current dependencies into this encounter's own snapshot.
        resonance_content::preparation::ui(snapshot, &diagnostics, cancelled)?;
        let rules = resonance_content::battle_rules::capture(snapshot, cancelled)?;
        let mut enemies = super::super::enemies_from_formations(
            &rules.formations,
            menus.monsters()?,
            menus.items.len(),
            setup.formations()?,
        )?;
        let party_positions = party_positions(menus, party)?;
        ensure!(
            session.items.len() == menus.items.len(),
            "item definitions differ from the menu catalogue"
        );

        let mut event_scenes = Vec::new();
        if setup.route != [0; 5] {
            ensure!(
                setup.formation()? == 4
                    && setup.arena == 26
                    && setup.route == [2, 3, 600, 0, 3]
                    && setup.music == Some(91)
                    && setup.defeat == resonance_events::battle::DefeatPolicy::GameOver,
                "unsupported battle route overrides"
            );
            ensure!(
                party
                    .formation
                    .iter()
                    .take(4)
                    .copied()
                    .collect::<BTreeSet<_>>()
                    == BTreeSet::from([1, 2, 3, 9])
                    && party.formation.get(3) == Some(&9),
                "Vidarr event requires its initialized party"
            );
            for id in [2, 3] {
                event_scenes.push(Arc::new(
                    resonance_content::battle_event::document::capture(
                        snapshot,
                        id,
                        &diagnostics,
                        cancelled,
                    )?,
                ));
            }
        }

        let equipped: BTreeSet<_> = party
            .formation
            .iter()
            .take(party_positions.len())
            .flat_map(|&character| {
                let member = &party.members[usize::from(character - 1)];
                [member.equipment[0], member.equipment[5]]
            })
            .filter(|&item| item != 0)
            .collect();
        let mut candidates: BTreeSet<_> = party
            .items
            .iter()
            .filter_map(|(&item, &count)| (item != 0 && count > 0).then_some(item))
            .collect();
        candidates.extend(
            party
                .members
                .iter()
                .flat_map(|member| [member.equipment[0], member.equipment[5]])
                .filter(|&item| item != 0),
        );
        let selected_characters = party
            .formation
            .iter()
            .take(party_positions.len())
            .fold(0, |mask, &character| mask | (1 << (character - 1)));
        let mapped_equipment = models::equipment_mappings(snapshot)?
            .into_keys()
            .collect::<BTreeSet<_>>();
        let mut available_equipment = BTreeMap::new();
        let mut weapons = BTreeMap::new();
        for item in candidates {
            ensure!(!cancelled(), "battle preparation cancelled");
            let definition = session
                .items
                .get(usize::from(item))
                .with_context(|| format!("missing equipment item definition {item}"))?;
            if !equipped.contains(&item) && definition.allowed_characters & selected_characters == 0
            {
                continue;
            }
            if !mapped_equipment.contains(&u32::from(item)) {
                ensure!(
                    !matches!(definition.equipment_kind, Some(0 | 3)),
                    "missing equipment attachment {item}"
                );
                continue;
            }
            let (kind, weapon) =
                match models::capture_equipment(snapshot, item, &diagnostics, cancelled)? {
                    battle_model::Attachment::Nonvisual => continue,
                    battle_model::Attachment::Weapon(weapon) => (EquipmentKind::Weapon, weapon),
                    battle_model::Attachment::Shield(weapon) => (EquipmentKind::Shield, weapon),
                };
            available_equipment.insert(item, (kind, definition.allowed_characters));
            weapons.insert(item, weapon);
        }
        let mut party_actors = Vec::new();
        let mut victory_owners = Vec::new();
        for (&character, position) in party.formation.iter().zip(party_positions) {
            let character = Character::try_from(character)?;
            let member = &party.members[character.index()];
            let equipment = available_equipment
                .iter()
                .filter_map(|(&item, &(kind, allowed))| {
                    (member.equipment[0] == item
                        || member.equipment[5] == item
                        || allowed & (1 << character.index()) != 0)
                        .then_some((item, kind))
                })
                .collect();
            let (selection, source) = models::capture_party(
                snapshot,
                character as u8,
                member.costume,
                &diagnostics,
                cancelled,
            )?;
            if enemies.settings.celebrate {
                victory_owners.push((character as u8, member.costume, selection));
            }
            party_actors.push(PartyInput {
                character,
                position,
                source,
                equipment,
                motions: BTreeMap::new(),
            });
        }
        let mut enemy_definitions = BTreeMap::new();
        let mut enemy_models = BTreeMap::new();
        let mut enemy_effects = BTreeMap::new();
        // A formation can name one enemy more than once with different hidden
        // name settings. Refresh every instance from this candidate's one model.
        for resource in &mut enemies.resources {
            let id = resource.monster.id;
            let captured = battle_enemy::document::capture(snapshot, id, &diagnostics, cancelled)?;
            enemy_definitions.insert(id, captured.definition);
            if let Some(art) = captured.artwork {
                Arc::make_mut(&mut resource.monster).preview = art.preview;
                enemy_models.insert(id, (art.source, BTreeMap::new()));
            }
            if let Some((_, bank)) = captured.effects {
                enemy_effects.insert(id, Arc::new(bank));
            }
        }
        let has_host = enemies.spawns.first().is_some_and(|spawn| {
            let id = enemies.resources[spawn.resource].monster.id;
            enemy_definitions[&id].profile.traits.hosts_participants
        });
        if !has_host {
            for definition in enemy_definitions.values_mut() {
                let profile = &mut definition.profile;
                if profile.hosted_body.take().is_some() {
                    profile.traits.passable_for_allies = false;
                    profile.traits.suppress_hurt_motion = false;
                    profile.traits.push_obstacle_disabled = false;
                    profile.traits.item_target_excluded = false;
                }
            }
        }
        ensure!(
            isolated_fixture
                || enemy_definitions
                    .values()
                    .all(|enemy| !enemy.actions.policy.event_hp_hold || !event_scenes.is_empty()),
            "enemy requires a prepared battle event route"
        );
        let stage = resonance_content::battle_stage::document::capture(
            snapshot,
            setup.arena,
            &diagnostics,
            cancelled,
        )?;
        if let Some(reason) = &stage.unsupported_reason {
            anyhow::bail!(
                "battle stage {} presentation is not prepared: {reason}",
                setup.arena
            );
        }
        let victory = resonance_content::battle_victory::document::capture(
            snapshot,
            victory_owners
                .iter()
                .map(|(character, costume, path)| (*character, *costume, path)),
            &diagnostics,
            cancelled,
        )?;
        if setup.defeat == resonance_events::battle::DefeatPolicy::GameOver {
            resonance_content::game_over::document::capture(snapshot, &diagnostics, cancelled)?;
        }
        let ui =
            resonance_content::battle_ui::document::capture(snapshot, &diagnostics, cancelled)?;
        let common_effects = effect(snapshot, effects::COMMON, &diagnostics, cancelled)?;
        let technique_effects = effect(snapshot, effects::TECHNIQUES, &diagnostics, cancelled)?;
        let mut stage_effects = if stage.recurring_interval.is_some() {
            effect(
                snapshot,
                stage
                    .effects
                    .as_deref()
                    .context("recurring arena has no effect bank")?,
                &diagnostics,
                cancelled,
            )?
        } else {
            None
        };
        let tints = snapshot.read(battle_effect::TINTS_PATH, JSON_LIMIT);
        ensure!(!cancelled(), "battle preparation cancelled");
        diagnostics.attempt(battle_effect::TINTS_PATH, tints)?;

        // Select choreography from the rules already captured for this encounter.
        let catalogue = Arc::new(rules.techniques);
        let recoil = crate::battle::recoil::Parameters::from_table(rules.recoil);
        let unison = Arc::new(rules.unison);
        let (scene_ids, skill_ids) =
            choreography_ids(&catalogue, &unison, &party_actors, &enemy_definitions)?;
        let mut spell_scenes = BTreeMap::new();
        let mut skill_scenes = BTreeMap::new();
        for (kind, ids, target) in [
            (Kind::Technique, scene_ids, &mut spell_scenes),
            (Kind::Skill, skill_ids, &mut skill_scenes),
        ] {
            for id in ids {
                // The scene is required; independently invalid optional art
                // retains the bank reader's diagnostics within that scene.
                let (_, bank) = scenes::capture(snapshot, kind, id, &diagnostics, cancelled)?;
                target.insert(id, Arc::new(bank));
            }
        }
        for bank in stage_effects
            .iter_mut()
            .chain(enemy_effects.values_mut())
            .chain(spell_scenes.values_mut())
            .chain(skill_scenes.values_mut())
        {
            if let (Some(art), Some(common)) = (
                Arc::make_mut(bank).art.as_mut(),
                common_effects.as_ref().and_then(|bank| bank.art.as_ref()),
            ) {
                for (&slot, textures) in &common.textures {
                    art.textures.entry(slot).or_insert_with(|| textures.clone());
                }
            }
        }
        let files = Files::capture_snapshot(snapshot, cancelled, diagnostics.clone())?;
        for input in &mut party_actors {
            input.motions = diagnostics
                .attempt(
                    "party body motions",
                    (|| {
                        model::motions(
                            &files,
                            input
                                .source
                                .parts
                                .first()
                                .context("missing party body scene")?,
                            &input.source.body.skeleton,
                        )
                    })(),
                )?
                .unwrap_or_default();
        }
        for resource in &enemies.resources {
            let enemy = &resource.monster;
            if let Some((source, motions)) = enemy_models.get_mut(&enemy.id) {
                *motions = diagnostics
                    .attempt(
                        "enemy body motions",
                        (|| {
                            model::motions(
                                &files,
                                &enemy
                                    .preview
                                    .parts
                                    .first()
                                    .context("missing enemy body scene")?
                                    .scene,
                                &source.body.skeleton,
                            )
                        })(),
                    )?
                    .unwrap_or_default();
            }
        }
        let mut event_audio = Selection::default();
        for scene in &event_scenes {
            event_audio.sounds.extend(&scene.audio.sounds);
            event_audio.streams.extend(&scene.audio.streams);
            event_audio.voices.extend(&scene.audio.voices);
            event_audio.music.extend(&scene.audio.music);
        }
        let events = if event_scenes.is_empty() {
            None
        } else {
            Some(crate::battle::event::Plan::vidarr(
                crate::battle::event::Prepared::load(&files, event_scenes[0].clone())?,
                crate::battle::event::Prepared::load(&files, event_scenes[1].clone())?,
            ))
        };
        let game_over = if setup.defeat == resonance_events::battle::DefeatPolicy::GameOver {
            Some(crate::game_over::Assets::from_prepared(files.clone())?)
        } else {
            None
        };
        ensure!(!cancelled(), "battle preparation cancelled");
        Ok(Self {
            setup,
            events,
            event_audio,
            files,
            stage,
            ui,
            catalogue,
            unison,
            projectiles: rules.projectiles,
            recoil,
            victory,
            game_over,
            enemies,
            party: party.clone(),
            party_profiles: rules.party,
            defense_system: rules.defense,
            party_actors,
            weapons,
            enemy_definitions,
            enemy_models,
            common_effects,
            technique_effects,
            stage_effects,
            enemy_effects,
            spell_scenes,
            skill_scenes,
        })
    }
}

fn effect(
    snapshot: &mut Snapshot,
    path: &str,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<Arc<SourceBank>>> {
    let captured = effects::capture(snapshot, path, diagnostics, cancelled);
    ensure!(!cancelled(), "battle preparation cancelled");
    diagnostics.attempt(path, captured.map(Arc::new))
}
