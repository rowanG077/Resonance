//! Shared CPU encounter preparation and lifecycle helpers for battle result tests.
use super::*;
use crate::battle::{
    command,
    encounter::{Assets, PrepareOptions},
    lifecycle,
    voice::Sound,
};
use resonance_battle::{BattleFrame, BattleInput, ButtonInput};
use resonance_content::diagnostics::Diagnostics;
use resonance_events::battle::{DefeatPolicy, Setup as Encounter};

pub(super) fn edge() -> ButtonInput {
    ButtonInput {
        held: true,
        pressed: true,
        released: false,
    }
}

pub(crate) struct Display;
impl Presentation for Display {
    fn request(&mut self, _: Request, _: Option<&Results>, _: &Battle) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn fixture(formation: &[u8], encounter: u16) -> Result<(Candidate, Battle, Party)> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = prepared_fixture(formation, encounter, &[])?;
    enter_battle(&mut candidate, &mut battle)?;
    Ok((candidate, battle, field))
}

pub(crate) struct PreparedFixture {
    pub candidate: Candidate,
    pub battle: Battle,
    pub field: Party,
    pub lifecycle: crate::battle::lifecycle::Lifecycle,
}

pub(super) fn command_visit(
    f: &mut PreparedFixture,
    command: command::Input,
) -> Result<(BattleFrame, Vec<command::Event>)> {
    let frame = f.lifecycle.step(
        &mut f.battle,
        lifecycle::Input {
            command,
            ..Default::default()
        },
        &mut f.candidate,
        &mut Display,
    )?;
    Ok((frame, f.lifecycle.take_command_events()))
}

pub(crate) fn native_battle(actors: Vec<resonance_battle::Actor>) -> Result<Battle> {
    resonance_battle::PreparedBattle::new(
        actors
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )?
    .finish()
}

/// Small native encounter shared by result arithmetic and lifecycle boundary tests.
pub(crate) fn native_fixture(
    formation: &[u8],
    configure: impl FnOnce(&mut Party, &SessionData, &mut MenuData) -> Result<()>,
    prepare: impl FnOnce(Vec<resonance_battle::Actor>, &mut Setup) -> Result<Battle>,
) -> Result<PreparedFixture> {
    use crate::battle::{lifecycle::Lifecycle, party, rewards};
    use resonance_battle::{Control, Side};
    let (session, catalogue, titles, mut field) = rewards::tests::reward_fixture();
    let (mut menus, _) = party::projection_tests::fixture()?;
    menus.titles = titles;
    field.formation = formation.to_vec();
    configure(&mut field, &session, &mut menus)?;
    let roster = formation
        .iter()
        .take(4)
        .enumerate()
        .map(|(slot, &character)| Ok((ActorId::from_index(slot)?, Character::try_from(character)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut actors = roster
        .iter()
        .map(|&(_, character)| {
            let member = &field.members[character.index()];
            party::actor(
                &party::loadout(&menus, member, character)?,
                member,
                Control::Manual,
                [0.; 3],
                0.,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let enemy_id = ActorId::from_index(actors.len())?;
    let mut enemy = actors[0].clone();
    enemy.side = Side::Enemy;
    enemy.control = Control::Enemy;
    actors.push(enemy);
    let mut setup = Setup {
        devils_arms_unlocked: false,
        enemies: vec![PreparedEnemy {
            actor: enemy_id,
            level: 1,
            grade: 0,
            steal: None,
            steal_chance: 0,
            reward: rewards::EnemyReward {
                monster: 0,
                experience: 0,
                gald: 0,
                drops: [None; 2],
                drop_chances: [0; 2],
            },
        }],
        level_difference: 0,
        actors: roster,
        formation: 1,
        formation_count: 1,
        hunting_symbol: None,
        style: ResultStyle {
            play_music: false,
            celebrate: false,
        },
        story: 0,
        colette_state: ColetteState::Normal,
        victory_story_flags: [false; 2],
        groups: vec![],
        performances: vec![],
        postures: vec![],
        victory_voices: Default::default(),
    };
    let battle = prepare(actors, &mut setup)?;
    let candidate = Candidate::new(
        setup,
        battle.actors(),
        field.clone(),
        Default::default(),
        Arc::new(session),
        Arc::new(menus),
        Arc::new(catalogue),
    )?;
    Ok(PreparedFixture {
        candidate,
        battle,
        field,
        lifecycle: Lifecycle::new(None),
    })
}

pub(crate) fn prepared_fixture(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
) -> Result<PreparedFixture> {
    prepared_fixture_with_party(formation, encounter, techniques, |_, _, _| Ok(()))
}

pub(crate) fn prepared_fixture_with_party(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    configure: impl FnOnce(&mut Party, &SessionData, &MenuData) -> Result<()>,
) -> Result<PreparedFixture> {
    prepared_fixture_with_story(formation, encounter, techniques, 2500, configure)
}

pub(crate) fn prepared_fixture_with_story(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    story: i32,
    configure: impl FnOnce(&mut Party, &SessionData, &MenuData) -> Result<()>,
) -> Result<PreparedFixture> {
    load_fixture(
        formation,
        encounter,
        techniques,
        Diagnostics::default(),
        |party, session, menus, _, _| configure(party, session, menus),
    )?
    .prepare(story, |sound| Ok(Some(sound)))
}

pub(crate) struct LoadedFixture {
    pub inputs: Assets,
    pub(super) modification: tempfile::TempDir,
    pub(super) party: Party,
    pub(super) session: Arc<SessionData>,
    pub(super) menus: Arc<MenuData>,
    pub(super) catalogue: Arc<Catalogue>,
}

pub(crate) fn load_fixture(
    formation: &[u8],
    encounter: u16,
    techniques: &[(u8, u16)],
    diagnostics: Diagnostics,
    configure: impl FnOnce(
        &mut Party,
        &SessionData,
        &MenuData,
        &mut resonance_content::library::Snapshot,
        &std::path::Path,
    ) -> Result<()>,
) -> Result<LoadedFixture> {
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
        });

    let retained = resonance_content::preparation::capture(
        &resonance_content::library::Library::open(&root, &[])?,
        &[],
        || false,
        diagnostics,
    )?;
    let session = Arc::new(SessionData::load(&retained)?);
    let menus = session
        .rules
        .as_ref()
        .context("missing session rules")?
        .clone();
    let mut party = Party::new(&session, Default::default())?;
    party.formation = formation.to_vec();
    party.field_leader = formation[0];
    party.settings.battle_controls = [0; 4];
    for &character in formation {
        party
            .raise_level(&session, usize::from(character - 1), 30, None, || 7)
            .map_err(anyhow::Error::msg)?;
    }
    for member in &mut party.members {
        member.techniques.clear();
        member.disabled_techniques.clear();
        member.shortcuts = [0; 4];
        member.assist_shortcuts = [None; 2];
        member.ex_skills = [0; 4];
        member.compound_ex_skills.clear();
        member.recent_compound_ex_skills.clear();
    }
    party
        .change_item(&session, 1, 2)
        .map_err(anyhow::Error::msg)?;
    party
        .change_item(&session, 37, 4)
        .map_err(anyhow::Error::msg)?;
    for &(character, technique) in techniques {
        let member = &mut party.members[usize::from(character - 1)];
        member.techniques.insert(technique);
    }
    let modification = tempfile::tempdir()?;
    configure(
        &mut party,
        &session,
        &menus,
        &mut retained
            .library()
            .context("named library is required")?
            .snapshot(),
        modification.path(),
    )?;
    party.validate(&session)?;
    let library = resonance_content::library::Library::open(
        &root,
        &[resonance_content::library::Mod {
            name: "battle fixture edits".into(),
            root: modification.path().into(),
        }],
    )?;
    let (session, menus) = if modification
        .path()
        .join(resonance_content::strategy::PATH)
        .is_file()
    {
        let retained = resonance_content::preparation::capture(
            &library,
            &[],
            || false,
            Diagnostics::default(),
        )?;
        let session = Arc::new(SessionData::load(&retained)?);
        let menus = session
            .rules
            .as_ref()
            .context("missing modified menu rules")?
            .clone();
        (session, menus)
    } else {
        (session, menus)
    };
    let inputs = Assets::capture(
        &mut library.snapshot(),
        &menus,
        &session,
        &party,
        Encounter {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(encounter),
            arena: 13,
            defeat: DefeatPolicy::ResumeEvent,
            music: None,
        },
        (&retained).diagnostics().clone(),
        &(|| false),
    )?;
    let catalogue = inputs.catalogue.clone();
    Ok(LoadedFixture {
        inputs,
        modification,
        party,
        session,
        menus,
        catalogue,
    })
}

impl LoadedFixture {
    pub fn prepare(
        self,
        story: i32,
        sound: impl FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    ) -> Result<PreparedFixture> {
        let Self {
            inputs,
            modification: _modification,
            party,
            session,
            menus,
            catalogue,
        } = self;
        let prepared = inputs.prepare(
            &menus,
            PrepareOptions {
                hunting_symbol: None,
                event_flags: Default::default(),
                random_seed: 0x2345,
                map: 332,
                world_music: 0,
                story,
                colette_state: 0,
                devils_arms_unlocked: false,
                victory_story_flags: [true; 2],
                overlimit_boost: false,
            },
            sound,
        )?;
        let lifecycle = prepared.lifecycle;
        let candidate = Candidate::new(
            prepared.results,
            prepared.core.actors(),
            party.clone(),
            Default::default(),
            session,
            menus,
            catalogue,
        )?;
        Ok(PreparedFixture {
            candidate,
            battle: prepared.core,
            field: party,
            lifecycle,
        })
    }
}

pub(crate) fn step(
    candidate: &mut Candidate,
    battle: &mut Battle,
) -> Result<resonance_battle::BattleFrame> {
    let cues = candidate.world_update(battle, BattleInput::default())?;
    Ok(battle.publish(cues))
}

pub(crate) fn enter_battle(candidate: &mut Candidate, battle: &mut Battle) -> Result<()> {
    for _ in 0..600 {
        let frame = step(candidate, battle)?;
        if battle.phase() == resonance_battle::BattlePhase::Combat && !frame.clock.paused() {
            return Ok(());
        }
        ensure!(!battle.is_diagnostic(), "battle entry entered diagnostics");
    }
    anyhow::bail!("battle entry did not enable actor control")
}

pub(crate) fn escape(candidate: &mut Candidate, battle: &mut Battle) -> Result<BattleOutcome> {
    // Isolate the persistent finish boundary from the authored escape timeline.
    // Actual event/once-only commit is covered by the existing Completed tests.
    battle.recognize_escape(true)?;
    ensure!(
        battle.recognize_result() == Some(BattleResult::Escaped),
        "escape not recognized"
    );
    candidate.recorded_escape = true;
    candidate.party.battles.record_escape();
    battle
        .finish_result()?
        .outcome
        .context("missing escape outcome")
}
