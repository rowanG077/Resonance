//! Real-resource encounter coverage for mixed Ghoul, Pumpkin and Phantom groups.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionRequest, Activity, ActorId, BattleInput, ButtonInput, ContactSource, ControlInput, Cue,
    Sound,
};
use resonance_content::{
    battle_effect,
    diagnostics::Diagnostics,
    menu_data::MenuData,
    prepared::{Cache, Files},
    session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Setup},
    libc_random,
    party::Party,
};
use resonance_game::battle::encounter::{Assets, PrepareOptions, Prepared};
use std::{collections::BTreeSet, sync::Arc};

const SEED: u64 = 0x2345;
const ENCOUNTER_COMPLETION_BUDGET: u32 = 600;
const SPELL_COMPLETION_BUDGET: u32 = 1000;

struct Fixture {
    prepared: Prepared,
    party: Vec<ActorId>,
    phantom_cast: Option<(ActorId, resonance_battle::ActionKey)>,
}

fn prepare(formation: u16, solo_regal: bool) -> Result<Fixture> {
    let root = crate::common::asset_root();
    let mut files = Cache::default();
    let retained = Files::load(&root, &["fields/map-332.preload.json"], &mut files, || {
        false
    })?;
    let menus: MenuData = retained.json("game/menu-data.json")?;
    let mut data: SessionData = retained.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default())?;
    if solo_regal {
        let growth = menus.titles[7][usize::from(party.members[7].title - 1)].growth;
        let mut random = 4357;
        party
            .raise_level(&data, 7, 30, Some(growth), || libc_random(&mut random))
            .map_err(anyhow::Error::msg)?;
        party.formation = vec![8];
    } else {
        party.formation = vec![1, 2, 3, 4];
    }
    party.field_leader = party.formation[0];
    party.settings.battle_controls = [0; 4];
    party.validate(&data)?;
    let assets = Assets::load(
        &root,
        &retained,
        &menus,
        &data,
        &party,
        Setup {
            route: [0; 5],
            encounter: resonance_events::battle::Encounter::Formation(formation),
            arena: 13,
            defeat: DefeatPolicy::ResumeEvent,
            music: None,
        },
        &mut files,
        || false,
    )?;
    let audio = assets.audio.as_ref().context("missing battle audio")?;
    let prepared = assets.prepare(
        &menus,
        PrepareOptions {
            random_seed: SEED,
            map: 332,
            world_music: 0,
            story: 0,
            story3: false,
            colette_state: 0,
            devils_arms_unlocked: false,
            victory_story_flags: [false; 2],
            overlimit_boost: false,
        },
        |request| {
            let path = match request {
                Sound::Cue(index) => {
                    &audio
                        .assets
                        .sounds
                        .get(&i16::try_from(index)?)
                        .context("unpublished sound")?
                        .path
                }
                Sound::Stream(index) => {
                    &audio
                        .assets
                        .voices
                        .get(&u32::from(index))
                        .context("unpublished voice")?
                        .asset
                        .path
                }
            };
            assets.files.read(path)?;
            Ok(Some(request))
        },
    )?;
    assert_eq!(prepared.characters, party.formation);
    let phantom_cast = prepared
        .results
        .enemies
        .iter()
        .find(|enemy| enemy.reward.monster == 50)
        .map(|enemy| -> Result<_> {
            let source: resonance_content::battle_enemy::Definition = assets
                .files
                .json(&resonance_content::battle_enemy::path(enemy.reward.monster))?;
            let cast = source
                .actions
                .rows
                .iter()
                .position(|row| {
                    row.attack == Some(resonance_content::battle_action::EnemyAttack::FireBall)
                })
                .context("missing native Fire Ball")?;
            Ok((
                enemy.actor,
                prepared.core.enemy_choices(enemy.actor).unwrap()[cast].action,
            ))
        })
        .transpose()?;
    assert!(!assets.files.diagnostics().has_errors());

    if solo_regal {
        // Enemy-only Fire Ball must prepare its contact particles and cast palette
        // even when the party has no Fire Ball resource dependency.
        let source: battle_effect::SourceBank = assets.files.json(battle_effect::COMMON_PATH)?;
        let common = prepared
            .effects
            .iter()
            .find(|effect| effect.source.source_sha256 == source.source_sha256)
            .context("missing common effect bank")?;
        let tints: battle_effect::Tints = assets.files.json(battle_effect::TINTS_PATH)?;
        let fire = 3;
        let contact = common
            .source
            .program(usize::from(tints.contact_effects[fire]))?;
        for member in contact.iter().filter_map(|event| match event.operation {
            battle_effect::EffectOperation::Spawn { particle, .. } => Some(particle),
            _ => None,
        }) {
            assert!(common.members.contains(&u16::from(member)));
        }
        let pulse = common.source.particle(82)?;
        assert!(
            common.palettes.get(&82).is_some_and(
                |pairs| pairs.contains(&[tints.palettes[fire], pulse.state.palettes[1]])
            )
        );
    }
    Ok(Fixture {
        party: prepared
            .results
            .actors
            .iter()
            .map(|(actor, _)| *actor)
            .collect(),
        prepared,
        phantom_cast,
    })
}

fn guard_input(actors: &[ActorId], update: u32) -> BattleInput {
    BattleInput {
        controllers: actors
            .iter()
            .map(|&actor| ControlInput {
                guard: ButtonInput {
                    held: true,
                    pressed: update == 0,
                    released: false,
                },
                ..ControlInput::neutral(actor)
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
#[ignore = "requires current enemy encounter assets; CPU only"]
fn enemy_formations_select_attacks_and_damage_a_guarding_party() -> Result<()> {
    for formation in [0, 2, 251, 266] {
        let fixture = prepare(formation, false)?;
        let mut battle = fixture.prepared.core;
        let mut hit = false;
        for update in 0..ENCOUNTER_COMPLETION_BUDGET {
            let frame = battle.step(guard_input(&fixture.party, update))?;
            hit |= frame.cues.iter().any(|cue| matches!(cue,
                Cue::Hit { actor, result, .. } if fixture.party.contains(actor) && result.hp_change < 0));
            if hit {
                break;
            }
        }
        assert!(hit, "formation {formation} never damaged the party");
        assert!(!battle.is_diagnostic());
    }
    Ok(())
}

#[test]
#[ignore = "requires prepared formation266 and Regal progression assets; CPU only"]
fn phantom_cast_releases_damaging_projectiles_and_cleans_up() -> Result<()> {
    let fixture = prepare(266, true)?;
    let (phantom, cast_action) = fixture.phantom_cast.context("missing Phantom cast")?;
    let mut battle = fixture.prepared.core;
    let diagnostics = Diagnostics::new(false);
    battle.set_diagnostics(diagnostics.clone());
    let mut parent = None;
    let mut release = None;
    let mut admission_tp = None;
    let mut projectiles = BTreeSet::new();
    let mut expired = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut hit = false;
    let mut effect = false;
    let mut sound = false;
    let mut finished = false;
    let mut requested = false;
    for update in 0..SPELL_COMPLETION_BUDGET {
        let mut input = guard_input(&fixture.party, update);
        if !requested
            && battle.phase() != resonance_battle::BattlePhase::Entry
            && battle.activity(phantom) == Activity::Idle
            && battle.actors()[phantom.index()].movement.hover_ready()
        {
            input.actions.push(ActionRequest {
                actor: phantom,
                target: fixture.party[0],
                action: cast_action,
            });
            requested = true;
        }
        let frame = battle.step(input)?;
        for cue in &frame.cues {
            if let Cue::Started { action, actor, .. } = *cue
                && actor == phantom
                && parent.is_none()
                && matches!(
                    frame.actors[actor.index()].activity,
                    Activity::Casting { .. }
                )
            {
                parent = Some(action);
                admission_tp = Some(frame.actors[actor.index()].tp);
            }
        }
        for cue in &frame.cues {
            if let Cue::Released {
                action,
                parent: Some(owner),
                actor,
                ..
            } = *cue
                && Some(owner) == parent
            {
                assert_eq!(actor, phantom);
                assert!(release.replace(action).is_none(), "cast released twice");
                assert!(frame.actors[actor.index()].tp < admission_tp.unwrap());
            }
        }
        for cue in &frame.cues {
            if let Cue::ProjectileStarted { projectile, action } = *cue
                && Some(action) == release
            {
                assert!(projectiles.insert(projectile));
                let shown = frame
                    .projectiles
                    .iter()
                    .find(|row| row.id == projectile)
                    .context("emitted Fire Ball is absent")?;
                assert_eq!(shown.owner, phantom);
                assert!(fixture.party.contains(&shown.target));
            }
        }
        for cue in &frame.cues {
            match cue {
                Cue::Hit {
                    source: ContactSource::Projectile(projectile),
                    actor,
                    result,
                    ..
                } if projectiles.contains(projectile) => {
                    assert!(fixture.party.contains(actor));
                    hit |= result.hp_change < 0;
                }
                Cue::Completed { action } => {
                    completed.insert(*action);
                }
                Cue::ProjectileExpired { projectile } if projectiles.contains(projectile) => {
                    expired.insert(*projectile);
                }
                Cue::Sound { actor, .. } if *actor == phantom && parent.is_some() => {
                    sound = true;
                }
                _ => {}
            }
        }
        effect |= crate::effects(&frame).any(|particle| particle.owner == phantom);
        finished = parent.is_some_and(|id| completed.contains(&id))
            && release.is_some_and(|id| completed.contains(&id))
            && projectiles.len() == 3
            && expired == projectiles
            && hit
            && effect
            && sound;
        if finished {
            break;
        }
        ensure!(
            fixture
                .party
                .iter()
                .any(|actor| frame.actors[actor.index()].available()),
            "party defeated before the Phantom spell completed"
        );
    }
    assert!(
        finished,
        "Phantom did not complete its spell within {SPELL_COMPLETION_BUDGET} updates: parent={parent:?} release={release:?} parent_done={} resident_done={} projectiles={} expired={} hit={hit} effect={effect} sound={sound}, actors(hp,tp,activity,position)={:?}",
        parent.is_some_and(|id| completed.contains(&id)),
        release.is_some_and(|id| completed.contains(&id)),
        projectiles.len(),
        expired.len(),
        battle
            .snapshot()
            .actors
            .iter()
            .map(|actor| (actor.hp, actor.tp, actor.activity, actor.position))
            .collect::<Vec<_>>()
    );
    assert!(!diagnostics.has_errors());
    Ok(())
}
