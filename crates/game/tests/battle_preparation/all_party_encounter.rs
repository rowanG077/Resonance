use super::*;
use resonance_battle::TechniqueId;
use resonance_battle::{
    Activity, BattlePhase, ButtonInput, Character, ContactSource, ControlInput, Cue,
};
use resonance_content::{diagnostics::Diagnostics, menu_data::MenuData, session::SessionData};
use resonance_events::{battle::DefeatPolicy, party::Party};
use resonance_game::battle::encounter::{Assets, PrepareOptions, Prepared};

pub(super) struct ColdEncounter {
    files: Files,
    pub(super) menus: MenuData,
    pub(super) session: SessionData,
    story: i32,
}

impl ColdEncounter {
    pub(super) fn load() -> Result<Self> {
        let root = common::asset_root();

        let files = resonance_content::preparation::capture(
            &resonance_content::library::Library::open(&root, &[])?,
            &[],
            || false,
            resonance_content::diagnostics::Diagnostics::new(true),
        )?;
        let session = SessionData::load(&files)?;
        let menus = session.rules.as_ref().unwrap().as_ref().clone();
        Ok(Self {
            files,
            menus,
            session,
            story: 2500,
        })
    }

    pub(super) fn prepare(&mut self, party: &Party) -> Result<(Assets, Prepared)> {
        self.prepare_formation(party, 2)
    }

    fn prepare_formation(&mut self, party: &Party, formation: u16) -> Result<(Assets, Prepared)> {
        party.validate(&self.session)?;
        let (assets, prepared) = {
            let mut snapshot = (&self.files)
                .library()
                .context("named library is required")?
                .snapshot();
            let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
                &mut snapshot,
                &(|| false),
            )?;
            let inputs = resonance_game::battle::encounter::Assets::capture(
                &mut snapshot,
                &self.menus,
                &self.session,
                party,
                opening_setup(formation, DefeatPolicy::GameOver),
                (&self.files).diagnostics().clone(),
                &(|| false),
            )?;
            let (assets, prepared, _) = inputs.prepare_named(
                &self.menus,
                PrepareOptions {
                    story: self.story,
                    ..opening_options(0x2345)
                },
                &catalogue,
            )?;
            Ok::<_, anyhow::Error>((assets, prepared))
        }?;

        Ok((assets, prepared))
    }
}

#[test]
#[ignore = "requires cooked party costumes and victory motions; CPU only"]
fn all_costumes_prepare_bodies_actions_and_celebrations() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    let mut snapshot = fixture
        .files
        .library()
        .context("named library is required")?
        .snapshot();
    let mut available = std::collections::BTreeSet::new();
    for character in 1..=9_u8 {
        let mapping = snapshot.resolve(
            resonance_content::battle_model::document::PARTY,
            u32::from(character),
            Some("mappings.json"),
        )?;
        for (costume, _) in snapshot.mappings(&mapping)?.iter() {
            available.insert((character, u8::try_from(costume)?));
        }
    }
    for (character, titles) in fixture.menus.titles.iter().enumerate() {
        for costume in titles.iter().filter_map(|title| title.costume) {
            assert!(available.contains(&((character + 1) as u8, costume as u8)));
        }
    }
    for costume in 0..5 {
        for formation in [&[1, 2, 3, 4][..], &[5, 6, 7, 8], &[9]] {
            let mut party = Party::new(&fixture.session, Default::default())?;
            party.formation = formation
                .iter()
                .copied()
                .filter(|&character| available.contains(&(character, costume)))
                .collect();
            if party.formation.is_empty() {
                continue;
            }
            let formation = party.formation.clone();
            party.field_leader = formation[0];
            for &character in &formation {
                let member = &mut party.members[usize::from(character - 1)];
                member.costume = costume;
                if let Some(title) = fixture.menus.titles[usize::from(character - 1)]
                    .iter()
                    .position(|title| title.costume.is_some_and(|value| value as u8 == costume))
                {
                    member.title = (title + 1).try_into()?;
                }
            }
            let (assets, prepared) = fixture
                .prepare(&party)
                .with_context(|| format!("party {formation:?}, costume {costume}"))?;
            for (actor, &character) in formation.iter().enumerate() {
                let (_, body) = resonance_content::battle_model::document::capture_party(
                    &mut snapshot,
                    character,
                    costume,
                    &Diagnostics::new(true),
                    &|| false,
                )?;
                let rendered = prepared
                    .models
                    .iter()
                    .find(|model| {
                        model
                            .parts
                            .iter()
                            .any(|part| part.scene.mesh == body.parts[0].mesh)
                    })
                    .context("missing rendered party body")?;
                let appearance = body.parts[0].appearance.as_ref().unwrap();
                assert_eq!(
                    rendered
                        .texture_channels
                        .iter()
                        .map(|c| (c.texture, c.frames))
                        .collect::<Vec<_>>(),
                    [
                        (appearance.eyes.unwrap(), 16),
                        (appearance.mouth.unwrap(), 8)
                    ]
                );
                let head = &prepared.head_anchors[actor];
                assert_eq!(
                    body.body.skeleton.bones[usize::from(head.bone)].name,
                    "head"
                );
                let overlimit = prepared.feedback.actors[actor].overlimit.as_ref().unwrap();
                assert_eq!(
                    body.body.skeleton.bones[usize::from(overlimit.bone)].name,
                    "spine-01"
                );
                let character = Character::try_from(character)?;
                assert_eq!(
                    prepared
                        .results
                        .performances
                        .iter()
                        .filter(|performance| performance.character == character)
                        .count(),
                    5
                );
            }
            let mut core = prepared.core;
            core.set_diagnostics(Diagnostics::new(true));
            for _ in 0..180 {
                let mut frame = core.step(BattleInput::default())?;
                let clock = frame.clock;
                core.advance_models(&mut frame, clock, false)?;
            }
            assert!(!assets.files.diagnostics().has_errors());
            assert!(!core.is_diagnostic());
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires current boss models and action metadata; CPU only"]
fn changed_enemy_families_prepare_their_linked_models_and_actions() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    let party = Party::new(&fixture.session, Default::default())?;
    // Includes mounted knights and Undine's separately animated cape.
    for formation in [16, 83, 34, 43, 87, 88, 89, 93, 354, 98, 24] {
        fixture
            .prepare_formation(&party, formation)
            .with_context(|| format!("prepare changed enemy formation {formation}"))?;
    }
    Ok(())
}

#[test]
#[ignore = "requires current Volt body and carried-model publications; CPU only"]
fn volt_body_and_corona_animate_in_prepared_encounter() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    let mut party = Party::new(&fixture.session, Default::default())?;
    party.formation = vec![1];
    party.field_leader = 1;
    party.settings.battle_controls = [1; 4];
    let (assets, prepared) = fixture.prepare_formation(&party, 46)?;
    let owner = prepared
        .results
        .enemies
        .iter()
        .find(|enemy| enemy.reward.monster == 198)
        .context("missing Volt")?
        .actor;
    let mut core = prepared.core;
    core.set_diagnostics(Diagnostics::new(true));
    let mut body_poses = Vec::new();
    let mut corona_poses = Vec::new();
    let mut corona_frames = Vec::new();
    let mut lightning_rows = std::collections::BTreeSet::new();
    for _ in 0..360 {
        let mut frame = core.step(BattleInput::default())?;
        let clock = frame.clock;
        core.advance_models(&mut frame, clock, false)?;
        if core.phase() != BattlePhase::Combat || clock != resonance_battle::BattleClock::Running {
            continue;
        }
        let body = frame
            .models
            .iter()
            .find(|model| model.actor == owner)
            .context("missing Volt body pose")?;
        let corona = frame
            .weapons
            .iter()
            .find(|model| {
                model.owner == owner && model.slot == Some(resonance_battle::WeaponSlot(0))
            })
            .context("missing Volt corona pose")?;
        assert!(body.visible && corona.visible);
        assert_eq!(corona.clip, Some(0));
        body_poses.push(body.bones.clone());
        corona_poses.push(corona.bones.clone());
        corona_frames.push(corona.frame);
        let (texture, offset) = corona
            .texture_scroll
            .context("missing Volt lightning atlas")?;
        assert_eq!(texture, 0);
        assert_eq!(offset[0], 0.);
        lightning_rows.insert(offset[1].to_bits());
        if body_poses.len() == 20 {
            break;
        }
    }
    assert_eq!(body_poses.len(), 20, "Volt never entered combat");
    assert!(
        body_poses.windows(2).any(|poses| poses[0] != poses[1]),
        "Volt's body is frozen"
    );
    assert!(
        corona_poses.windows(2).any(|poses| poses[0] != poses[1]),
        "Volt's corona is frozen"
    );
    for frames in corona_frames.windows(2) {
        let advance = (frames[1] - frames[0]).rem_euclid(3.);
        assert!(
            (advance - 0.15).abs() < 0.0001,
            "incorrect corona rate: {frames:?}"
        );
    }
    assert_eq!(
        lightning_rows,
        [0_f32, 1. / 3., 2. / 3.].map(f32::to_bits).into(),
        "Volt's lightning atlas must cycle through all three images"
    );
    assert!(!assets.files.diagnostics().has_errors());
    assert!(!core.is_diagnostic());
    Ok(())
}

fn prepare(fixture: &mut ColdEncounter, formation: &[u8], controls: [u8; 4]) -> Result<Prepared> {
    let mut party = Party::new(&fixture.session, Default::default())?;
    party.formation = formation.to_vec();
    party.field_leader = formation[0];
    party.settings.battle_controls = controls;
    for member in &mut party.members {
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.recent_compound_ex_skills.clear();
    }
    let (_, prepared) = fixture.prepare(&party)?;
    assert_eq!(
        prepared.characters,
        formation
            .iter()
            .copied()
            .map(|id| Character::try_from(id).map_err(anyhow::Error::from))
            .collect::<Result<Vec<_>>>()?
    );
    Ok(prepared)
}

fn live(mut battle: Battle) -> Battle {
    battle.set_diagnostics(Diagnostics::new(true));
    battle
}

#[test]
#[ignore = "requires current party, encounter and audio publications; CPU only"]
fn mixed_parties_prepare_spells_and_audio() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    // Genis shares each roster so spell voice selection reaches all nine profiles.
    for formation in [[3, 1, 2, 4], [3, 5, 6, 7], [3, 8, 9, 1]] {
        let mut party = Party::new(&fixture.session, Default::default())?;
        party.formation = formation.to_vec();
        party.field_leader = formation[0];
        assert_eq!(party.members[1].costume, 0);
        // Retaliation skills are native gameplay; neither needs a spell or artwork binding.
        use resonance_content::menu_data::ex_effect;
        for (character, skill) in [(2, ex_effect::REFLECT_DAMAGE), (4, ex_effect::AID_REVENGE)] {
            if !formation.contains(&character) {
                continue;
            }
            let index = usize::from(character - 1);
            let catalogue = &fixture.menus.ex_skills.characters[index];
            let recipe = catalogue
                .compounds
                .iter()
                .find(|recipe| recipe.skill == skill)
                .context("missing retaliation recipe")?;
            let member = &mut party.members[index];
            member.ex_skills = [0; 4];
            member.ex_skills[..recipe.required.len()].copy_from_slice(&recipe.required);
            member.ex_gems = member.ex_skills.map(|skill| {
                catalogue
                    .levels
                    .iter()
                    .position(|level| level.contains(&skill))
                    .map_or(0, |level| level as u8 + 1)
            });
        }
        let (assets, prepared) = fixture.prepare(&party)?;
        for (index, &character) in prepared.characters.iter().enumerate() {
            let traits = prepared.core.actors()[index].equipment.reaction_ex;
            if character == Character::Colette {
                assert!(traits.reflect_damage);
            }
            if character == Character::Raine {
                assert!(traits.aid_revenge);
            }
        }
        let audio = resonance_content::battle_audio::document::Catalogue::capture(
            &mut assets
                .files
                .library()
                .context("named library required")?
                .snapshot(),
            &|| false,
        )?;
        assert!(
            audio
                .resolved
                .music
                .contains_key(&i16::try_from(prepared.music)?)
        );
        let actors = prepared.core.actors();
        for (index, actor) in actors.iter().enumerate() {
            assert!(actor.position.iter().all(|value| value.is_finite()));
            for other in &actors[..index] {
                let distance = (actor.position[0] - other.position[0])
                    .hypot(actor.position[2] - other.position[2]);
                assert!(
                    distance >= actor.body_radius() + other.body_radius(),
                    "overlapping spawn"
                );
            }
        }
        assert!(!assets.files.diagnostics().has_errors());
        let mut battle = live(prepared.core);
        for _ in 0..240 {
            let frame = battle.step(Default::default())?;
            if battle.phase() == BattlePhase::Combat
                && frame.clock == resonance_battle::BattleClock::Running
            {
                break;
            }
        }
        assert_eq!(battle.phase(), BattlePhase::Combat);
        assert_eq!(
            battle.snapshot().clock,
            resonance_battle::BattleClock::Running
        );
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires current Colette and Ray Thrust publications; CPU only"]
fn learned_ray_thrust_uses_its_prepared_projectile_and_feedback() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    let prepared = prepare(&mut fixture, &[2], [1, 0, 0, 0])?;
    let owner = prepared.results.actors[0].0;
    let mut battle = live(prepared.core);
    for _ in 0..240 {
        let frame = battle.step(Default::default())?;
        if battle.phase() == BattlePhase::Combat
            && frame.clock == resonance_battle::BattleClock::Running
        {
            break;
        }
    }
    assert_eq!(
        battle.snapshot().clock,
        resonance_battle::BattleClock::Running
    );
    const RAY_THRUST: u16 = 35;
    let action = battle.record_technique_acquisition(owner, TechniqueId(RAY_THRUST))?;
    battle
        .prepare_shortcut(owner, resonance_battle::ShortcutSlot::Neutral, Some(action))?
        .commit();
    let mut requested = false;
    let mut projectile = None;
    let mut ring_birth = false;
    let mut shadow = false;
    let mut hit = false;
    for _ in 0..300 {
        let attack = !requested && battle.activity(owner) == Activity::Idle;
        requested |= attack;
        let mut control = ControlInput::neutral(owner);
        control.technique = ButtonInput {
            pressed: attack,
            held: attack,
            released: false,
        };
        let frame = battle.step(BattleInput::from(control))?;
        for cue in &frame.cues {
            match cue {
                Cue::ProjectileStarted {
                    projectile: id,
                    action: handle,
                    ..
                } if battle.action_definition(*handle) == Some(action) => projectile = Some(*id),
                Cue::Hit {
                    source: ContactSource::Projectile(id),
                    ..
                } if Some(*id) == projectile => hit = true,
                _ => {}
            }
        }
        ring_birth |= crate::effects(&frame).any(|effect| {
            effect.owner == owner
                && effect.appearance.member == 30
                && matches!(effect.follow, Some(resonance_battle::EffectFollow::Projectile(id))
                    if Some(id) == projectile)
        });
        shadow |= frame
            .projectile_shadows
            .iter()
            .any(|frame| Some(frame.projectile) == projectile);
        if hit || frame.outcome.is_some() {
            break;
        }
    }
    assert!(
        ring_birth && shadow && hit,
        "Ray Thrust: ring_birth={ring_birth}, shadow={shadow}, hit={hit}"
    );
    assert!(!battle.is_diagnostic());
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current party and arte publications; CPU only"]
fn both_arte_paths_learn_the_missing_branch_on_use() -> Result<()> {
    let mut fixture = ColdEncounter::load()?;
    // Lloyd: Fierce Demon Fang -> Double Demon Fang.
    // Raine: Nurse -> Heal. Both keep the branch they already learned.
    for (character, base, known, missing, balance) in [(1, 1, 4, 2, 50), (4, 98, 99, 100, -50)] {
        let mut party = Party::new(&fixture.session, Default::default())?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.battle_controls = [1; 4];
        party.settings.preferences.learn_both_arte_paths = true;
        let member = &mut party.members[usize::from(character - 1)];
        member.level = 60;
        member.techniques = [base, known].into();
        member.technique_uses = [(base, 200), (known, 200)].into();
        member.shortcuts = [base, 0, 0, 0];
        member.technique_balance = balance;
        member.base_stats[1] = 999;
        member.tp = 999;
        let (_, prepared) = fixture.prepare_formation(&party, 1)?;
        let owner = prepared.results.actors[0].0;
        let mut battle = live(prepared.core);
        assert!(
            !battle
                .current_techniques(owner)
                .unwrap()
                .contains(&TechniqueId(missing))
        );
        enter_battle(&mut battle)?;
        let enemy = battle
            .actor_ids()
            .find(|id| battle.actors()[id.index()].side == Side::Enemy)
            .context("missing learning target")?;
        battle.set_fixture_neutral_control(enemy)?;
        battle.set_actor_vitals(owner, 500, 10000, 999, 999)?;
        let [x, y, z] = battle.actors()[enemy.index()].position;
        battle.restore_fixture_placement(owner, [x - 80., y, z], Some(90.))?;
        let action = battle
            .prepared_technique(owner, TechniqueId(base))
            .unwrap()
            .action;
        for tick in 0..600 {
            let mut frame = battle.step(BattleInput {
                actions: if tick == 0 {
                    vec![ActionRequest {
                        actor: owner,
                        target: if character == 4 { owner } else { enemy },
                        action,
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            })?;
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, false)?;
            if battle
                .current_techniques(owner)
                .unwrap()
                .contains(&TechniqueId(missing))
            {
                break;
            }
        }
        let learned = battle.current_techniques(owner).unwrap();
        assert!(learned.contains(&TechniqueId(known)));
        assert!(
            learned.contains(&TechniqueId(missing)),
            "character {character} did not learn {missing}"
        );
        assert_eq!(battle.technique_uses(owner, TechniqueId(base)), Some(201));
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current party and elemental arte publications; CPU only"]
fn elemental_variants_execute_from_equipment_and_permanent_learning() -> Result<()> {
    use resonance_content::arte::elemental;

    let mut fixture = ColdEncounter::load()?;
    let cases: Vec<_> = fixture
        .session
        .characters
        .iter()
        .enumerate()
        .flat_map(|(index, character)| {
            elemental::VARIANTS
                .iter()
                .filter(move |variant| character.allowed_techniques.contains(&variant.id))
                .flat_map(move |variant| {
                    [false, true].into_iter().flat_map(move |learned| {
                        [false, true]
                            .into_iter()
                            .filter(move |airborne| {
                                !airborne || variant.id == elemental::RISING_PHOENIX
                            })
                            .map(move |airborne| (index as u8 + 1, variant, learned, airborne))
                    })
                })
        })
        .collect();
    for (character, variant, learned, airborne) in cases {
        println!(
            "Executing {} for character {character}: learned={learned}, airborne={airborne}",
            variant.name
        );
        let mut party = Party::new(&fixture.session, Default::default())?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.battle_controls = [1; 4];
        party.settings.preferences.learn_elemental_artes = true;
        let member = &mut party.members[usize::from(character - 1)];
        let selected = if learned { variant.id } else { variant.base };
        member.techniques.insert(selected);
        member.shortcuts[0] = selected;
        member.technique_uses.insert(variant.base, 207);
        if airborne {
            let recipe = fixture.menus.ex_skills.characters[usize::from(character - 1)]
                .compounds
                .iter()
                .find(|recipe| {
                    recipe.skill == resonance_content::menu_data::ex_effect::AERIAL_ARTE_COMPOUND
                })
                .context("missing aerial arte EX recipe")?;
            for (slot, &skill) in recipe.required.iter().enumerate() {
                member.ex_gems[slot] = 5;
                member.ex_skills[slot] = skill;
            }
        }
        if !learned {
            member.equipment[4] = fixture
                .menus
                .items
                .iter()
                .enumerate()
                .find(|(_, item)| {
                    item.category == 40 && item.properties.attack_element == Some(variant.element)
                })
                .context("missing elemental accessory")?
                .0
                .try_into()?;
        }
        member.base_stats[1] = 999;
        member.tp = 999;
        // Keep Pow Hammer's independent lucky Toss Hammer roll out of this case.
        member.luck = 0;
        let settings = party.settings.clone();
        let mut party: Party = serde_json::from_slice(&serde_json::to_vec(&party)?)?;
        party.settings = settings;
        let (assets, prepared) = fixture.prepare_formation(&party, 1)?;
        let owner = prepared.results.actors[0].0;
        let mut effects =
            resonance_battle::Effects::new(prepared.effect_banks, None, 1, Diagnostics::new(true))?;
        let mut battle = live(prepared.core);
        enter_battle(&mut battle)?;
        let enemy = battle
            .actor_ids()
            .find(|id| battle.actors()[id.index()].side == Side::Enemy)
            .context("missing elemental arte target")?;
        for id in battle.actor_ids().collect::<Vec<_>>() {
            battle.set_actor_vitals(id, 20000, 20000, 999, 999)?;
            if battle.actors()[id.index()].side == Side::Enemy {
                battle.set_fixture_neutral_control(id)?;
            }
        }
        let phoenix = variant.id == elemental::RISING_PHOENIX;
        if !phoenix || airborne {
            let [x, y, z] = battle.actors()[enemy.index()].position;
            battle.restore_fixture_placement(owner, [x - 80., y, z], Some(90.))?;
        }
        battle.restore_fixture_target(owner, enemy)?;
        if airborne {
            assert!(
                battle.actors()[owner.index()]
                    .equipment
                    .combo_traits
                    .aerial_arte
            );
            for _ in 0..60 {
                if battle.actors()[owner.index()].position[1] > 30. {
                    break;
                }
                let mut control = ControlInput::neutral(owner);
                control.stick = [0, 127];
                control.guard.held = true;
                let mut frame = battle.step(BattleInput::from(control))?;
                let clock = frame.clock;
                battle.advance_models(&mut frame, clock, false)?;
                effects.advance(&frame, false)?;
            }
            assert!(battle.actors()[owner.index()].position[1] > 30.);
        }
        let action = battle
            .prepared_technique(owner, TechniqueId(variant.id))
            .context("learned elemental arte was not prepared")?
            .action;
        let mut handle = None;
        let mut completed = false;
        let mut phoenix_effect = false;
        let mut landing_sound = false;
        for tick in 0..240 {
            let mut control = ControlInput::neutral(owner);
            control.technique.pressed = tick == 0;
            control.technique.held = tick == 0;
            let mut frame = battle.step(BattleInput::from(control))?;
            let clock = frame.clock;
            battle.advance_models(&mut frame, clock, false)?;
            effects.advance(&frame, false)?;
            for cue in frame.cues {
                match cue {
                    Cue::Started {
                        actor,
                        action: started,
                        ..
                    } if actor == owner => {
                        assert_eq!(battle.action_definition(started), Some(action));
                        handle = Some(started);
                    }
                    Cue::Completed { action } if Some(action) == handle => completed = true,
                    Cue::ActionEffect { request, .. } if request.owner == owner => {
                        phoenix_effect |=
                            request.appearance.member == if airborne { 137 } else { 136 };
                    }
                    Cue::Sound {
                        actor,
                        sound: resonance_battle::Sound::Cue(138),
                        ..
                    } if actor == owner => {
                        assert!(battle.actors()[owner.index()].position[1] <= 0.1);
                        landing_sound = true;
                    }
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(
            completed,
            "{} never completed for character {character}; started={}, activity={:?}",
            variant.name,
            handle.is_some(),
            battle.activity(owner)
        );
        if phoenix {
            assert!(phoenix_effect && landing_sound);
            assert!(battle.actors()[enemy.index()].hp < 20000);
        }
        assert_eq!(
            battle.technique_uses(owner, TechniqueId(variant.id)),
            Some(1)
        );
        assert_eq!(
            battle.technique_uses(owner, TechniqueId(variant.base)),
            Some(if learned { 207 } else { 208 })
        );
        assert!(!assets.files.diagnostics().has_errors());
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires current combined arte publications; CPU only"]
fn every_combined_unison_recipe_executes() -> Result<()> {
    use resonance_content::battle_unison::Catalogue;
    let mut fixture = ColdEncounter::load()?;
    fixture.story = 1_403_000;
    let catalogue: Catalogue = resonance_content::battle_rules::unison(&fixture.files)?;
    let mut failures = Vec::new();
    let mut cases = 0;
    let selected_row = std::env::var("RESONANCE_TEST_UNISON_ROW")
        .ok()
        .map(|value| value.parse::<u8>())
        .transpose()?;
    for row in &catalogue.combinations {
        if selected_row.is_some_and(|id| id != row.id) {
            continue;
        }
        for recipe in &row.recipes {
            anyhow::ensure!(recipe.len() == 2, "unsupported test recipe {recipe:?}");
            let owners = |native| {
                fixture
                    .session
                    .characters
                    .iter()
                    .enumerate()
                    .flat_map(|(index, character)| {
                        character
                            .allowed_techniques
                            .iter()
                            .copied()
                            .filter_map(|arte| {
                                (catalogue.placements.get(&arte)?.native_id == native
                                    && fixture.menus.techniques[usize::from(arte)].unison_usable)
                                    .then_some((index as u8 + 1, arte))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };
            let first = owners(recipe[0]);
            let second = owners(recipe[1]);
            let before = cases;
            for &a in &first {
                for &b in &second {
                    if a.0 == b.0 {
                        continue;
                    }
                    cases += 1;
                    let label = format!(
                        "{} row={} recipe={recipe:?} party={:?}",
                        row.name,
                        row.id,
                        [a, b]
                    );
                    match execute_combination(&mut fixture, row, [a, b]) {
                        Ok(()) => println!("PASS {label}"),
                        Err(error) => {
                            let failure = format!("{label}: {error:#}");
                            println!("FAIL {failure}");
                            failures.push(failure);
                        }
                    }
                }
            }
            anyhow::ensure!(
                cases > before,
                "no legal owners for {} {recipe:?}",
                row.name
            );
        }
    }
    println!(
        "Executed {cases} combined Unison cases; {} failed",
        failures.len()
    );
    anyhow::ensure!(cases > 0, "no combined Unison cases selected");
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "requires current party and combined arte publications; CPU only"]
fn elemental_variants_keep_base_unison_recipes() -> Result<()> {
    use resonance_content::{arte::elemental, battle_unison};
    let mut fixture = ColdEncounter::load()?;
    fixture.story = 1_403_000;
    let catalogue: battle_unison::Catalogue =
        resonance_content::battle_rules::unison(&fixture.files)?;
    for (name, participants) in [
        (
            "Pow Blade",
            [(1, elemental::LIGHTNING_TIGER), (2, elemental::ICE_HAMMER)],
        ),
        ("Cross Thrust", [(1, elemental::LIGHTNING_THRUST), (6, 210)]),
    ] {
        let row = catalogue
            .combinations
            .iter()
            .find(|row| row.name == name)
            .context("missing combined arte")?;
        execute_combination(&mut fixture, row, participants)?;
    }
    Ok(())
}

#[cfg(feature = "test-support")]
fn execute_combination(
    fixture: &mut ColdEncounter,
    row: &resonance_content::battle_unison::Combination,
    participants: [(u8, u16); 2],
) -> Result<()> {
    use resonance_battle::{ActorId, UnisonCompletion, UnisonInput, UnisonPhase};
    let mut party = Party::new(&fixture.session, Default::default())?;
    party.formation = vec![1];
    for (character, _) in participants {
        if !party.formation.contains(&character) {
            party.formation.push(character);
        }
    }
    party.field_leader = 1;
    party.settings.battle_controls = [1; 4];
    party.settings.preferences.learn_elemental_artes = true;
    party.unison_gauge = 200;
    for (character, arte) in participants {
        let member = &mut party.members[usize::from(character - 1)];
        member.techniques = [arte].into();
        member.shortcuts = [arte; 4];
        member.unison_arte = Some(arte);
    }
    let slots = participants.map(|(character, _)| {
        party
            .formation
            .iter()
            .position(|&id| id == character)
            .unwrap()
    });
    let (_, prepared) = fixture.prepare_formation(&party, 1)?;
    let mut effects =
        resonance_battle::Effects::new(prepared.effect_banks, None, 1, Diagnostics::new(true))?;
    let mut battle = live(prepared.core);
    enter_battle(&mut battle)?;
    let enemy = battle
        .actor_ids()
        .find(|id| battle.actors()[id.index()].side == Side::Enemy)
        .unwrap();
    for id in battle.actor_ids().collect::<Vec<_>>() {
        battle.set_actor_vitals(id, 20000, 20000, 9999, 9999)?;
        if battle.actors()[id.index()].side == Side::Enemy {
            battle.set_fixture_neutral_control(id)?;
        }
    }
    let [x, _, z] = battle.actors()[enemy.index()].position;
    battle.restore_fixture_placement(ActorId::from_index(0)?, [x - 100., 0., z], Some(90.))?;
    battle.restore_fixture_target(ActorId::from_index(0)?, enemy)?;
    battle.hold_event_boss_hp(Some(enemy))?;
    let mut selected = 0;
    let mut combined = false;
    let mut damage = false;
    let mut artwork = false;
    let mut animated = false;
    let mut previous_pose: Option<Vec<(ActorId, _)>> = None;
    for tick in 0..2400 {
        let phase = battle.snapshot().unison.map(|run| run.phase);
        let input = if tick == 0 {
            Some(UnisonInput::Activate(ActorId::from_index(0)?))
        } else if matches!(phase, Some(UnisonPhase::Selecting | UnisonPhase::Executing))
            && selected < slots.len()
        {
            let slot = slots[selected];
            selected += 1;
            Some(UnisonInput::Select {
                pressed_slots: 1 << slot,
            })
        } else {
            None
        };
        let mut frame = battle.step(BattleInput {
            unison: input,
            ..Default::default()
        })?;
        let clock = frame.clock;
        battle.advance_models(&mut frame, clock, false)?;
        effects.advance(&frame, false)?;
        if frame
            .unison
            .as_ref()
            .is_some_and(|run| run.phase == UnisonPhase::Combined)
        {
            combined = true;
            damage |= frame.cues.iter().any(|cue| matches!(cue, Cue::Hit { actor, result, .. } if *actor == enemy && result.hp_change < 0));
            artwork |= !effects.started_programs().is_empty();
            let pose: Vec<_> = frame
                .models
                .iter()
                .filter(|model| slots.contains(&model.actor.index()) && model.visible)
                .map(|model| (model.actor, model.bones.clone()))
                .collect();
            if let Some(previous) = &previous_pose {
                animated |= pose.iter().any(|current| {
                    previous
                        .iter()
                        .any(|old| old.0 == current.0 && old.1 != current.1)
                });
            }
            previous_pose = Some(pose);
        }
        if combined && frame.unison.is_none() {
            break;
        }
        if !combined && battle.last_unison_completion().is_some() {
            break;
        }
    }
    anyhow::ensure!(
        matches!(battle.last_unison_completion(), Some(UnisonCompletion::CombinationExecuted { id, native_id, .. }) if *id == resonance_battle::UnisonRecipeId(row.id) && *native_id == resonance_battle::UnisonArteId(row.native_id)),
        "completion {:?}",
        battle.last_unison_completion()
    );
    anyhow::ensure!(
        damage && artwork && animated,
        "damage={damage}, artwork={artwork}, animated={animated}"
    );
    anyhow::ensure!(!battle.is_diagnostic(), "battle diagnostics failed");
    Ok(())
}

#[test]
#[ignore = "requires prepared angel and Colette artwork; CPU only"]
fn angels_and_colette_prepare_independent_animated_wings() -> Result<()> {
    use resonance_game::battle::encounter::AccessoryStyle;
    let fixture = ColdEncounter::load()?;
    let mut party = Party::new(&fixture.session, Default::default())?;
    // Kratos has wings at the first Tower fight, but neither the later Tower fight
    // nor the Torent duel. The Torent package still contains unused wing artwork.
    for (formation, enemy_has_wings) in
        [(33, true), (65, false), (71, true), (74, false), (79, true)]
    {
        party.formation = if formation == 74 {
            vec![1]
        } else {
            vec![1, 2, 3, 4]
        };
        let (assets, prepared) = {
            let mut snapshot = (&fixture.files)
                .library()
                .context("named library is required")?
                .snapshot();
            let catalogue = resonance_content::battle_audio::document::Catalogue::capture(
                &mut snapshot,
                &(|| false),
            )?;
            let inputs = resonance_game::battle::encounter::Assets::capture(
                &mut snapshot,
                &fixture.menus,
                &fixture.session,
                &party,
                opening_setup(formation, DefeatPolicy::GameOver),
                (&fixture.files).diagnostics().clone(),
                &(|| false),
            )?;
            let (assets, prepared, _) = inputs.prepare_named(
                &fixture.menus,
                PrepareOptions {
                    colette_state: 1,
                    story: 23_001_000,
                    ..opening_options(0x2345)
                },
                &catalogue,
            )?;
            Ok::<_, anyhow::Error>((assets, prepared))
        }?;
        let colette_has_wings = formation != 74;
        assert_eq!(
            prepared.accessories.len(),
            usize::from(enemy_has_wings) + usize::from(colette_has_wings),
            "formation {formation}"
        );
        let enemy = prepared
            .accessories
            .iter()
            .find(|wing| !matches!(wing.style, AccessoryStyle::Colette { .. }));
        let colette = prepared
            .accessories
            .iter()
            .find(|wing| matches!(wing.style, AccessoryStyle::Colette { .. }));
        assert_eq!(enemy.is_some(), enemy_has_wings, "formation {formation}");
        assert_eq!(
            colette.is_some(),
            colette_has_wings,
            "formation {formation}"
        );
        if let (Some(enemy), Some(colette)) = (enemy, colette) {
            assert_ne!(enemy.owner, colette.owner);
            assert_ne!(enemy.resource, colette.resource);
            assert!(enemy.sparkle_texture().is_none());
            assert!(colette.sparkle_texture().is_some());
        }
        for wing in &prepared.accessories {
            let body = prepared
                .core
                .models()
                .unwrap()
                .definition(wing.owner)
                .unwrap();
            assert_eq!(
                body.skeleton.bones[wing.attachment.unwrap()].name,
                if matches!(wing.style, AccessoryStyle::Colette { .. }) {
                    resonance_events::Wings::ATTACHMENT_BONE
                } else {
                    "spine-02"
                }
            );
            let model = prepared
                .models
                .iter()
                .find(|model| model.resource == wing.resource)
                .unwrap();
            assert!(matches!(
                model.instances,
                battle::encounter::RenderInstances::Fixed(3)
            ));
            let motion = wing.motion.as_ref().unwrap();
            let start = resonance_content::animation::pose::sample(&wing.skeleton, motion, 0.)?;
            let later = resonance_content::animation::pose::sample(
                &wing.skeleton,
                motion,
                motion.duration_frames / 3.,
            )?;
            assert_ne!(
                start.iter().map(|pose| pose.matrix()).collect::<Vec<_>>(),
                later.iter().map(|pose| pose.matrix()).collect::<Vec<_>>(),
                "wing pose must flap independently of the body"
            );
        }
        assert!(!assets.files.diagnostics().has_errors());
    }
    Ok(())
}

#[test]
#[ignore = "requires original Earth Element, Luna and Meredy artwork; CPU only"]
fn ordinary_attachments_prepare_all_instances_and_selected_motions() -> Result<()> {
    use resonance_game::battle::encounter::{AccessoryStyle, RenderInstances};
    let mut fixture = ColdEncounter::load()?;
    let party = Party::new(&fixture.session, Default::default())?;
    for (formation, monster, count, independent, animated) in [
        (322, 76, 8, false, false),
        (58, 200, 1, false, true),
        (83, 233, 1, true, true),
    ] {
        let (assets, prepared) = fixture.prepare_formation(&party, formation)?;
        let owner = prepared
            .results
            .enemies
            .iter()
            .find(|enemy| enemy.reward.monster == monster)
            .context("missing attachment owner")?
            .actor;
        let accessories: Vec<_> = prepared
            .accessories
            .iter()
            .filter(|accessory| accessory.owner == owner)
            .collect();
        assert_eq!(accessories.len(), count, "enemy {monster}");
        let resources: std::collections::BTreeSet<_> = accessories
            .iter()
            .map(|accessory| accessory.resource)
            .collect();
        assert_eq!(
            resources.len(),
            count,
            "instances must not overwrite each other"
        );
        let bones: std::collections::BTreeSet<_> = accessories
            .iter()
            .filter_map(|accessory| accessory.attachment)
            .collect();
        assert_eq!(bones.len(), if independent { 0 } else { count });
        for accessory in accessories {
            assert!(accessory.visible && accessory.inherit_tint);
            assert!(matches!(accessory.style, AccessoryStyle::Ordinary));
            assert_eq!(accessory.motion.is_some(), animated);
            let render = prepared
                .models
                .iter()
                .find(|model| model.resource == accessory.resource)
                .context("attachment render resource is missing")?;
            assert_eq!(render.instances, RenderInstances::Fixed(1));
            assert!(!render.parts.is_empty());
            for part in &render.parts {
                assert!(assets.files.contains_key(&part.scene.mesh));
                assert!(part.animation.is_none() && part.attached_to.is_none());
            }
            if let Some(motion) = &accessory.motion {
                let poses = [0., motion.duration_frames / 3.].map(|time| {
                    resonance_content::animation::pose::sample(&accessory.skeleton, motion, time)
                        .map(|pose| {
                            pose.into_iter()
                                .map(|bone| bone.matrix())
                                .collect::<Vec<_>>()
                        })
                });
                let [start, later] = poses;
                assert_ne!(start?, later?, "enemy {monster} attachment is frozen");
            }
        }
        let mut core = prepared.core;
        core.set_diagnostics(Diagnostics::new(true));
        for _ in 0..240 {
            let mut frame = core.step(BattleInput::default())?;
            let clock = frame.clock;
            core.advance_models(&mut frame, clock, false)?;
        }
        assert!(!assets.files.diagnostics().has_errors());
        assert!(!core.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires installed library; named profile edits, CPU only, no conversion"]
fn named_profile_edits_reach_prepared_party() -> Result<()> {
    use resonance_content::{
        battle_rules,
        library::{Library, Mod},
    };
    let root = common::asset_root();
    let modifications = tempfile::tempdir()?;
    let path = modifications.path().join(battle_rules::PARTY);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut rules: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(battle_rules::PARTY))?)?;
    let profile = &mut rules["characters"]["lloyd"]["profile"];
    profile["head_anchor"] = serde_json::json!({"bone":"spine-01", "offset":[1.,2.,3.]});
    profile["overlimit_bone"] = "head".into();
    profile["species"] = "plant".into();
    profile["size"] = "huge".into();
    profile["recoil_weight"] = "grounded".into();
    profile["casting"] =
        serde_json::json!({"release_rate":0.75,"chant_once":true,"chant_loop_start":1.0});
    profile["expressions"] = serde_json::json!([
        {"texture":"mouth","frames":8,"idle":1,"active":2,"rescue":3,"blink":[4,5],"blink_exclude":[6]},
        {"texture":0,"frames":16,"idle":7,"active":8,"rescue":9}
    ]);
    profile["shadow"] = serde_json::Value::Null;
    profile.as_object_mut().unwrap().remove("target_anchor");
    std::fs::write(&path, serde_json::to_vec(&rules)?)?;
    let library = Library::open(
        &root,
        &[Mod {
            name: "profile anchors".into(),
            root: modifications.path().into(),
        }],
    )?;
    let files =
        resonance_content::preparation::capture(&library, &[], || false, Diagnostics::new(true))?;
    let session = SessionData::load(&files)?;
    let menus = session.rules.as_ref().unwrap().as_ref().clone();
    let mut party = Party::new(&session, Default::default())?;
    party.formation = vec![1];
    let mut snapshot = library.snapshot();
    let catalogue =
        resonance_content::battle_audio::document::Catalogue::capture(&mut snapshot, &|| false)?;
    let inputs = resonance_game::battle::encounter::Assets::capture(
        &mut snapshot,
        &menus,
        &session,
        &party,
        opening_setup(2, DefeatPolicy::GameOver),
        Diagnostics::new(true),
        &|| false,
    )?;
    // The selected profile is retained with the captured encounter.
    rules["characters"]["lloyd"]["profile"]["head_anchor"]["bone"] = "missing-after-capture".into();
    std::fs::write(&path, serde_json::to_vec(&rules)?)?;
    let (_, prepared, _) = inputs.prepare_named(&menus, opening_options(17), &catalogue)?;
    let (_, body) = resonance_content::battle_model::document::capture_party(
        &mut library.snapshot(),
        1,
        0,
        &Diagnostics::new(true),
        &|| false,
    )?;
    let head = body.body.skeleton.bone("spine-01").unwrap();
    let overlimit = body.body.skeleton.bone("head").unwrap();
    assert_eq!(u16::from(prepared.head_anchors[0].bone), head);
    assert_eq!(prepared.head_anchors[0].offset, [1., 2., 3.]);
    assert_eq!(
        u16::from(prepared.feedback.actors[0].overlimit.as_ref().unwrap().bone),
        overlimit
    );
    let model = prepared
        .models
        .iter()
        .find(|model| {
            model
                .parts
                .iter()
                .any(|p| p.scene.mesh == body.parts[0].mesh)
        })
        .unwrap();
    assert_eq!(model.target_anchor.unwrap().bone, Some(head));
    assert_eq!(model.target_anchor.unwrap().offset, [1., 2., 3.]);
    assert_eq!(
        model
            .texture_channels
            .iter()
            .map(|c| (c.texture, c.frames))
            .collect::<Vec<_>>(),
        [
            (body.parts[0].appearance.as_ref().unwrap().mouth.unwrap(), 8),
            (0, 16)
        ]
    );
    assert_eq!(prepared.feedback.rescues[0].expression, Some([3, 9, 0, 0]));
    let actor = &prepared.core.actors()[0];
    assert_eq!(actor.species, resonance_battle::Species::Plant);
    assert_eq!(actor.movement.steering.positioning_radius, 3);
    assert_eq!(
        actor.reaction.profile.vertical,
        resonance_battle::VerticalRecoil::Grounded
    );
    assert!(!actor.reaction.profile.can_launch);
    let body = prepared
        .core
        .models()
        .unwrap()
        .definition(resonance_battle::ActorId::from_index(0)?)
        .unwrap();
    assert_eq!(body.reactions.chant_rate, 0.75);
    assert!(body.reactions.chant_once);
    assert_eq!(body.reactions.chant_loop_start, 1.0);
    assert_eq!(body.reactions.falling_blend, 12);
    assert_eq!(body.idle_expression, [1, 7, 0, 0]);
    assert_eq!(body.active_expression, [2, 8, 0, 0]);
    assert_eq!(body.blinks[0].frames, [4, 5]);
    assert_eq!(body.blinks[0].excluded_expressions, [6]);
    assert!(body.shadow.is_none());
    Ok(())
}
