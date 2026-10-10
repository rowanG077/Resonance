use super::*;
use resonance_battle::{
    Activity, BattlePhase, ButtonInput, ContactSource, ControlInput, Cue, Sound,
};
use resonance_content::{
    diagnostics::Diagnostics, menu_data::MenuData, prepared::Cache, session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Setup},
    party::Party,
};
use resonance_game::battle::encounter::{Assets, PrepareOptions, Prepared};

pub(super) struct ColdEncounter {
    root: std::path::PathBuf,
    cache: Cache,
    files: Files,
    pub(super) menus: MenuData,
    pub(super) session: SessionData,
}

impl ColdEncounter {
    pub(super) fn load() -> Result<Self> {
        let root = common::asset_root();
        let mut cache = Cache::default();
        let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
            false
        })?;
        let menus: MenuData = files.json("game/menu-data.json")?;
        let mut session: SessionData = files.json("game/session-data.json")?;
        session.rules = Some(Arc::new(menus.clone()));
        Ok(Self {
            root,
            cache,
            files,
            menus,
            session,
        })
    }

    pub(super) fn prepare(&mut self, party: &Party) -> Result<(Assets, Prepared)> {
        party.validate(&self.session)?;
        let assets = Assets::load(
            &self.root,
            &self.files,
            &self.menus,
            &self.session,
            party,
            Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(2),
                arena: 13,
                defeat: DefeatPolicy::GameOver,
                music: None,
            },
            &mut self.cache,
            || false,
        )?;
        let audio = assets.audio.as_ref().context("missing battle audio")?;
        let prepared = assets.prepare(
            &self.menus,
            PrepareOptions {
                devils_arms_unlocked: false,
                victory_story_flags: [false; 2],
                random_seed: 0x2345,
                map: 332,
                world_music: 0,
                story: 2500,
                story3: false,
                colette_state: 0,
                overlimit_boost: false,
            },
            |request| {
                let exists = match request {
                    Sound::Cue(index) => audio.assets.sounds.contains_key(&i16::try_from(index)?),
                    Sound::Stream(index) => audio.assets.voices.contains_key(&u32::from(index)),
                };
                anyhow::ensure!(exists, "unprepared encounter audio {request:?}");
                Ok(Some(request))
            },
        )?;
        Ok((assets, prepared))
    }
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
    assert_eq!(prepared.characters, formation);
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
            if character == 2 {
                assert!(traits.reflect_damage);
            }
            if character == 4 {
                assert!(traits.aid_revenge);
            }
        }
        let audio = assets.audio.as_ref().context("missing battle audio")?;
        assert!(
            audio
                .assets
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
        if battle.phase() == BattlePhase::Combat {
            break;
        }
        battle.step(Default::default())?;
    }
    const RAY_THRUST: u16 = 35;
    let action = battle.record_technique_acquisition(owner, RAY_THRUST)?;
    battle.prepare_shortcut(owner, 0, Some(action))?.commit();
    let mut requested = false;
    let mut projectile = None;
    let mut visible = false;
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
        let frame = battle.step(BattleInput {
            controllers: vec![control],
            ..Default::default()
        })?;
        for cue in &frame.cues {
            match cue {
                Cue::ProjectileStarted {
                    projectile: id,
                    action: handle,
                } if battle.action_definition(*handle) == Some(action) => projectile = Some(*id),
                Cue::Hit {
                    source: ContactSource::Projectile(id),
                    ..
                } if Some(*id) == projectile => hit = true,
                _ => {}
            }
        }
        visible |= projectile.is_some() && crate::effects(&frame).any(|p| p.owner == owner);
        if hit || frame.outcome.is_some() {
            break;
        }
    }
    assert!(visible && hit, "Ray Thrust: visible={visible}, hit={hit}");
    assert!(!battle.is_diagnostic());
    Ok(())
}
