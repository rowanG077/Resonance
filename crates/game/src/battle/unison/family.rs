//! Family policies use measured scene placement and the authored participant tracks.
use super::*;
use resonance_battle::{AttackOrigin, UnisonPlacement, UnisonRole};

struct Settings {
    placements: [Option<UnisonPlacement>; 2],
    color: [u8; 3],
    effect_scale: Option<f32>,
    projectile_heading: Option<f32>,
    projectile_height: Option<f32>,
    end_bones: &'static [&'static str],
}

fn settings(native: u16) -> Option<Settings> {
    const EAST: [f32; 3] = [1., 0., 0.];
    const NORTH: [f32; 3] = [0., 0., 1.];
    const ANGLED: [f32; 3] = [0.9063078, 0., 0.42261818];
    let place = |direction, distance| {
        Some(UnisonPlacement {
            direction,
            distance,
        })
    };
    let pair = |directions: [[f32; 3]; 2], distances: [f32; 2]| {
        std::array::from_fn(|role| place(directions[role], distances[role]))
    };
    let placements = match native {
        301 => [place(EAST, 130.), None],
        302 => pair([EAST, NORTH], [250.; 2]),
        303 | 304 => pair([EAST, ANGLED], [250.; 2]),
        307 | 308 => pair([ANGLED, EAST], [250., 50.]),
        309 => pair([EAST, ANGLED], [450.; 2]),
        312 | 315 | 317 => [place(EAST, 100.), None],
        313 => [place(EAST, 200.), None],
        314 => [place(EAST, 50.), None],
        316 => pair([EAST, ANGLED], [100., 300.]),
        318 => pair([EAST, ANGLED], [200., 300.]),
        _ => return None,
    };
    Some(Settings {
        placements,
        color: match native {
            304 => [48; 3],
            307 => [16, 16, 24],
            308 => [24, 16, 16],
            _ => [16; 3],
        },
        effect_scale: matches!(native, 307..=309 | 315..=318).then_some(1.),
        projectile_heading: matches!(native, 313 | 315).then_some(0.),
        projectile_height: (native == 317).then_some(0.),
        end_bones: match native {
            301 => &["kk00", "kk01"],
            312 | 314 => &["kk00"],
            _ => &[],
        },
    })
}

fn phase(native: u16, role: usize, character: u8) -> Option<usize> {
    match (native, role, character) {
        (301, 0, 1) | (312, 0, 7) | (314, 0, 6) => Some(0),
        (301 | 312, 1, 2) | (314, 0, 9) => Some(1),
        (314, 1, 2) => Some(2),
        (302, 0, 1 | 6 | 9) => Some(0),
        (302, 1, 1 | 6 | 9) => Some(1),
        (303 | 304, 0, 1) => Some(0),
        (303 | 304, 0, 6) => Some(1),
        (303 | 304, 0, 9) => Some(2),
        (303 | 304, 1, 5) => Some(3),
        (307, 0, 3) | (308, 0, 6) => Some(0),
        (307, 0, 6) | (308, 0, 9) => Some(1),
        (307, 0, 9) | (308, 1, 7) => Some(2),
        (307, 1, 7) => Some(3),
        (309 | 313 | 315 | 317, 0, 1) | (316, 0, 2) | (318, 0, 6) => Some(0),
        (313 | 315, 1, 3 | 6 | 9) | (317, 1, 6 | 9) | (309 | 316, 1, 4) | (318, 0, 9) => Some(1),
        (318, 1, 4) => Some(2),
        _ => None,
    }
}

pub(super) fn has_pair(native: u16, participants: &[Participant<'_>]) -> bool {
    participants.iter().any(|first| {
        phase(native, 0, first.character).is_some()
            && participants.iter().any(|second| {
                first.actor != second.actor && phase(native, 1, second.character).is_some()
            })
    })
}

pub(super) fn prepare<S: FnMut(Sound) -> Result<Option<Sound>>>(
    combination: &Combination,
    scene: &Scene,
    bank: &EffectResource,
    participants: &[Participant<'_>],
    resources: &mut Resources<'_, S>,
    actions: &mut ActionDefinitions,
) -> Result<Option<UnisonCombination>> {
    let native = combination.native_id;
    let candidates: [Vec<_>; 2] = std::array::from_fn(|role| {
        participants
            .iter()
            .filter_map(|actor| phase(native, role, actor.character).map(|phase| (actor, phase)))
            .collect()
    });
    if candidates.iter().any(Vec::is_empty)
        || !candidates[0]
            .iter()
            .any(|(a, _)| candidates[1].iter().any(|(b, _)| a.actor != b.actor))
    {
        return Ok(None);
    }
    let Some(settings) = settings(native) else {
        return Ok(None);
    };
    let mut roles: [Vec<UnisonRole>; 2] = Default::default();
    for role in 0..2 {
        for &(actor, variant) in &candidates[role] {
            let mut source = scene
                .actions
                .get(&(variant as u8))
                .context("missing combination participant track")?
                .prepared()?
                .clone();
            action::remove_empty_blends(&mut source, actor.null_motions)?;
            let mut attack = action::prepare(&source, actor, bank, resources)?;
            decorate(
                combination,
                role,
                actor,
                &source,
                &settings,
                scene,
                bank,
                resources,
                &mut attack,
            )?;
            attack.events.sort_by_key(|(at, _)| *at);
            roles[role].push(UnisonRole {
                actor: actor.actor,
                action: actions.insert(ActionDefinition {
                    normal: None,
                    tp_cost: 0,
                    execution: ActionExecution::Attack(attack),
                }),
                placement: settings.placements[role],
                visible: role == 0 || !matches!(native, 301 | 312..=315 | 317),
                temporary_weapons: if role == 0 && matches!(native, 301 | 312 | 314) {
                    actor.temporary_weapons.clone()
                } else {
                    Vec::new()
                },
            });
        }
    }
    let mut voices = Vec::new();
    if matches!(native, 302 | 308) {
        for &(first, _) in &candidates[0] {
            for &(second, _) in &candidates[1] {
                let mut pair = [first.character, second.character];
                pair.sort_unstable();
                let (voice, speaker) = match (native, pair, first.character) {
                    (302, [1, 6], _) => (0x87a8, first.actor),
                    (302, [1, 9], _) => (0x87a9, first.actor),
                    (302, [6, 9], _) => (0x87aa, first.actor),
                    (308, _, 6) => (0x87c1, second.actor),
                    (308, _, 9) => (0x87c2, second.actor),
                    _ => continue,
                };
                if let Some(sound) = (resources.sound)(Sound::Stream(voice & 0x7fff))? {
                    voices.push(([first.actor, second.actor], speaker, sound));
                }
            }
        }
    }
    Ok(Some(UnisonCombination {
        roles,
        color: Some(scene.stage_color.unwrap_or(settings.color)),
        voices,
    }))
}

#[allow(clippy::too_many_arguments)]
fn decorate<S: FnMut(Sound) -> Result<Option<Sound>>>(
    combination: &Combination,
    role: usize,
    actor: &Participant<'_>,
    source: &resonance_content::battle_martial::Attack,
    settings: &Settings,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut Resources<'_, S>,
    attack: &mut resonance_battle::PreparedAttack,
) -> Result<()> {
    let native = combination.native_id;
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let effect = |member, origin, scale, follow| AttackEvent::EffectAt {
        appearance: appearance(member),
        origin,
        heading: None,
        scale,
        follow,
    };
    let initial = match (native, role) {
        (301 | 312, 0) => Some((2, AttackOrigin::ActorRoot, Some(1.), true)),
        (314, 0) => Some((5, AttackOrigin::ActorRoot, None, true)),
        (304, 1) => Some((2, AttackOrigin::TargetRoot, Some(1.), true)),
        (307 | 308, 1) => Some((1, AttackOrigin::ActorRoot, settings.effect_scale, false)),
        (309 | 316 | 318, 0) => Some((1, AttackOrigin::ActorAim, settings.effect_scale, true)),
        (315 | 317, 0) => Some((1, AttackOrigin::ActorRoot, settings.effect_scale, true)),
        _ => None,
    };
    if let Some((member, origin, scale, follow)) = initial {
        action::effect(bank, member, resources)?;
        attack
            .events
            .push((0, effect(member, origin, scale, follow)));
    }
    if role == 0 && matches!(native, 301 | 312 | 314) {
        anyhow::ensure!(
            !actor.temporary_weapons.is_empty(),
            "missing temporary combination weapon"
        );
        let member = if native == 314 { 4 } else { 5 };
        if !settings.end_bones.is_empty() {
            action::effect(bank, member, resources)?;
        }
        attack.events.push((
            attack.end_at,
            AttackEvent::Sound((resources.sound)(Sound::Cue(126))?),
        ));
        for name in settings.end_bones {
            let bone = actor
                .model
                .skeleton
                .bones
                .iter()
                .position(|bone| bone.name == *name)
                .and_then(|index| u16::try_from(index).ok())
                .context("missing combination release bone")?;
            attack.events.push((
                attack.end_at,
                effect(
                    member,
                    AttackOrigin::Bone(resonance_battle::MeleeAnchor {
                        resource: actor.model.resource,
                        bone,
                    }),
                    None,
                    false,
                ),
            ));
        }
    }
    let voice = match (native, role) {
        (303, 1) => Some(if combination.id == 3 { 0x879e } else { 0x879f }),
        (304, 1) => Some(0x87a4),
        (307, 1) => Some(0x87b1),
        (309, 1) => Some(0x879a),
        (316 | 318, 1) => Some(0x87b3),
        _ => None,
    };
    if let Some(voice) = voice {
        if let Some(sound) = (resources.sound)(Sound::Stream(voice & 0x7fff))? {
            attack.events.insert(
                0,
                (
                    0,
                    AttackEvent::Voice {
                        sound,
                        priority: resonance_battle::VoicePriority::Action,
                    },
                ),
            );
        }
    }
    if matches!(native, 302..=304) && role == 0 {
        let at = source
            .events
            .iter()
            .find_map(|(at, event)| {
                matches!(event, resonance_content::battle_normal::Event::Forward(speed) if *speed > 0.)
                    .then_some(*at)
            })
            .context("missing combination thrust movement")?;
        // The prepared movement timestamp already includes the pose waits.
        attack.events.push((
            at,
            AttackEvent::UnisonFeedback {
                duration: 60,
                limit: 8,
            },
        ));
    }
    let feedback = match native {
        307 | 308 => Some((30, 130)),
        309 => Some((55, 40)),
        316 | 318 => Some((70, 40)),
        _ => None,
    };
    if let Some((at, duration)) = feedback {
        attack.events.push((
            action::elapsed(source, at)?,
            AttackEvent::UnisonFeedback { duration, limit: 8 },
        ));
    }
    let release = match (native, role) {
        (313, 0) => Some((35, 1, 1)),
        (315, 0) => Some((66, 1, 2)),
        (317, 0) => Some((65, 1, 2)),
        (307, 1) => Some((30, 1, 0)),
        (308, 1) => Some((30, 1, 0)),
        (309, 1) => Some((60, 2, 1)),
        (316, 1) => Some((88, 2, 1)),
        (318, 0) => Some((78, 3, 2)),
        (318, 1) => Some((70, 2, 1)),
        _ => None,
    };
    if let Some((tick, member, rule)) = release {
        let row = scene
            .projectiles
            .get(member)
            .context("missing combination projectile")?;
        // Callback hit rules are independent of projectile template selectors.
        let source_rule = row
            .hits
            .get(rule)
            .copied()
            .flatten()
            .context("combination callback hit rule is not prepared")?;
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let mut hit = crate::battle::hit::authored(&source_rule, &recoil)?;
        if let Some(sound) = source_rule.sound {
            hit.sound = (resources.sound)(Sound::Cue(sound))?;
        }
        if let Some(member) = source_rule.effect {
            action::effect(bank, member, resources)?;
            hit.effect = Some(appearance(member));
        }
        let mut definition = resources.prepared.projectile(
            resources.files,
            &row.projectile,
            hit,
            &[(0, bank), (6, bank), (1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut definition)
            .contact
            .as_mut()
            .context("combination projectile has no contact")?
            .cooldown = source_rule.cooldown;
        let at = action::elapsed(source, tick)?;
        if native == 313 {
            action::effect(bank, 2, resources)?;
            attack
                .events
                .push((at, effect(2, AttackOrigin::TargetRoot, None, true)));
        }
        attack.events.push((
            at,
            AttackEvent::ProjectileAt {
                definition,
                origin: if matches!(native, 313 | 315) {
                    AttackOrigin::TargetRoot
                } else {
                    AttackOrigin::TargetAim
                },
                heading: settings.projectile_heading,
                height: settings.projectile_height,
            },
        ));
    }
    Ok(())
}
