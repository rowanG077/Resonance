//! Family policies use measured scene placement and the authored participant tracks.
use super::*;
use resonance_battle::{AttackOrigin, UnisonPlacement, UnisonRole};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    PowBlade,
    CrossThrust,
    MirageThrust,
    DarkSerpent,
    StardustRain,
    LPunishment,
    ArchWind,
    PhotonTempest,
    Mjollnir,
    PrismStars,
    PowDevastation,
    LTigerBlade,
    PowSpear,
    FieryBeast,
    PhotonBlast,
    TTigerBlade,
    PlasmaBlade,
}

impl Family {
    fn from_id(id: u16) -> Option<Self> {
        Some(match id {
            301 => Self::PowBlade,
            302 => Self::CrossThrust,
            303 => Self::MirageThrust,
            304 => Self::DarkSerpent,
            305 => Self::StardustRain,
            307 => Self::LPunishment,
            308 => Self::ArchWind,
            309 => Self::PhotonTempest,
            310 => Self::Mjollnir,
            311 => Self::PrismStars,
            312 => Self::PowDevastation,
            313 => Self::LTigerBlade,
            314 => Self::PowSpear,
            315 => Self::FieryBeast,
            316 => Self::PhotonBlast,
            317 => Self::TTigerBlade,
            318 => Self::PlasmaBlade,
            _ => return None,
        })
    }
}
use Family::*;

struct Settings {
    placements: [Option<UnisonPlacement>; 2],
    color: [u8; 3],
    effect_scale: Option<f32>,
    projectile_heading: Option<f32>,
    projectile_height: Option<f32>,
    end_bones: &'static [&'static str],
}

fn settings(family: Family) -> Settings {
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
    let placements = match family {
        PowBlade => [place(EAST, 130.), None],
        CrossThrust => pair([EAST, NORTH], [250.; 2]),
        MirageThrust | DarkSerpent | StardustRain => pair([EAST, ANGLED], [250.; 2]),
        LPunishment | ArchWind => pair([ANGLED, EAST], [250., 50.]),
        PhotonTempest => pair([EAST, ANGLED], [450.; 2]),
        Mjollnir => pair([EAST, ANGLED], [200.; 2]),
        PrismStars => pair([EAST, ANGLED], [250.; 2]),
        PowDevastation | FieryBeast | TTigerBlade => [place(EAST, 100.), None],
        LTigerBlade => [place(EAST, 200.), None],
        PowSpear => [place(EAST, 50.), None],
        PhotonBlast => pair([EAST, ANGLED], [100., 300.]),
        PlasmaBlade => pair([EAST, ANGLED], [200., 300.]),
    };
    Settings {
        placements,
        color: match family {
            DarkSerpent => [48; 3],
            LPunishment => [16, 16, 24],
            ArchWind => [24, 16, 16],
            StardustRain | PrismStars => [0; 3],
            _ => [16; 3],
        },
        effect_scale: matches!(
            family,
            LPunishment
                | ArchWind
                | PhotonTempest
                | FieryBeast
                | PhotonBlast
                | TTigerBlade
                | PlasmaBlade
        )
        .then_some(1.),
        projectile_heading: matches!(family, LTigerBlade | FieryBeast).then_some(0.),
        projectile_height: (family == TTigerBlade).then_some(0.),
        end_bones: match family {
            PowBlade => &["kk00", "kk01"],
            PowDevastation | PowSpear => &["kk00"],
            _ => &[],
        },
    }
}

fn phase(family: Family, role: usize, character: Character) -> Option<usize> {
    use Character::*;
    match (family, role, character) {
        (PowBlade, 0, Lloyd) | (PowDevastation, 0, Presea) | (PowSpear, 0, Zelos) => Some(0),
        (PowBlade | PowDevastation, 1, Colette) | (PowSpear, 0, Kratos) => Some(1),
        (PowSpear, 1, Colette) => Some(2),
        (CrossThrust, 0, Lloyd | Zelos | Kratos) => Some(0),
        (CrossThrust, 1, Lloyd | Zelos | Kratos) => Some(1),
        (MirageThrust | DarkSerpent, 0, Lloyd) => Some(0),
        (MirageThrust | DarkSerpent, 0, Zelos) => Some(1),
        (MirageThrust | DarkSerpent, 0, Kratos) => Some(2),
        (MirageThrust | DarkSerpent, 1, Sheena) => Some(3),
        (StardustRain, 0, Colette) | (Mjollnir, 0, Colette) => Some(0),
        (StardustRain, 1, Lloyd) | (Mjollnir, 1, Genis | Zelos | Kratos) => Some(1),
        (LPunishment, 0, Genis) | (ArchWind, 0, Zelos) => Some(0),
        (LPunishment, 0, Zelos) | (ArchWind, 0, Kratos) => Some(1),
        (LPunishment, 0, Kratos) | (ArchWind, 1, Presea) => Some(2),
        (LPunishment, 1, Presea) => Some(3),
        (PhotonTempest | LTigerBlade | FieryBeast | TTigerBlade, 0, Lloyd)
        | (PhotonBlast, 0, Colette)
        | (PlasmaBlade, 0, Zelos) => Some(0),
        (LTigerBlade | FieryBeast, 1, Genis | Zelos | Kratos)
        | (TTigerBlade, 1, Zelos | Kratos)
        | (PhotonTempest | PhotonBlast, 1, Raine)
        | (PlasmaBlade, 0, Kratos) => Some(1),
        (PlasmaBlade, 1, Raine) => Some(2),
        (PrismStars, 0, Genis) => Some(0),
        (PrismStars, 1, Raine) => Some(1),
        _ => None,
    }
}

pub(super) fn has_pair(native: u16, participants: &[Participant<'_>]) -> bool {
    let Some(family) = Family::from_id(native) else {
        return false;
    };
    participants.iter().any(|first| {
        phase(family, 0, first.character).is_some()
            && participants.iter().any(|second| {
                first.actor != second.actor && phase(family, 1, second.character).is_some()
            })
    })
}

pub(super) fn prepare(
    combination: &Combination,
    scene: &Scene,
    bank: &EffectResource,
    participants: &[Participant<'_>],
    resources: &mut Resources<'_>,
    actions: &mut ActionDefinitions,
) -> Result<Option<UnisonCombination>> {
    let Some(family) = Family::from_id(combination.native_id) else {
        return Ok(None);
    };
    let candidates: [Vec<_>; 2] = std::array::from_fn(|role| {
        participants
            .iter()
            .filter_map(|actor| phase(family, role, actor.character).map(|phase| (actor, phase)))
            .collect()
    });
    if candidates.iter().any(Vec::is_empty)
        || !candidates[0]
            .iter()
            .any(|(a, _)| candidates[1].iter().any(|(b, _)| a.actor != b.actor))
    {
        return Ok(None);
    }
    let settings = settings(family);
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
                family,
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
                visible: role == 0
                    || !matches!(
                        family,
                        PowBlade
                            | PowDevastation
                            | LTigerBlade
                            | PowSpear
                            | FieryBeast
                            | TTigerBlade
                    ),
                temporary_weapons: if role == 0
                    && matches!(family, PowBlade | PowDevastation | PowSpear)
                {
                    actor.temporary_weapons.clone()
                } else {
                    Vec::new()
                },
            });
        }
    }
    let mut voices = Vec::new();
    if matches!(family, CrossThrust | ArchWind) {
        for &(first, _) in &candidates[0] {
            for &(second, _) in &candidates[1] {
                let mut pair = [first.character, second.character];
                pair.sort_unstable();
                let (voice, speaker) = match (family, pair, first.character) {
                    (CrossThrust, [Character::Lloyd, Character::Zelos], _) => (0x87a8, first.actor),
                    (CrossThrust, [Character::Lloyd, Character::Kratos], _) => {
                        (0x87a9, first.actor)
                    }
                    (CrossThrust, [Character::Zelos, Character::Kratos], _) => {
                        (0x87aa, first.actor)
                    }
                    (ArchWind, _, Character::Zelos) => (0x87c1, second.actor),
                    (ArchWind, _, Character::Kratos) => (0x87c2, second.actor),
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
fn decorate(
    combination: &Combination,
    family: Family,
    role: usize,
    actor: &Participant<'_>,
    source: &resonance_content::battle_martial::Attack,
    settings: &Settings,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut Resources<'_>,
    attack: &mut resonance_battle::PreparedAttack,
) -> Result<()> {
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
    let initial = match (family, role) {
        (PowBlade | PowDevastation, 0) => Some((2, AttackOrigin::ActorRoot, Some(1.), true)),
        (PowSpear, 0) => Some((5, AttackOrigin::ActorRoot, None, true)),
        (DarkSerpent, 1) => Some((2, AttackOrigin::TargetRoot, Some(1.), true)),
        (LPunishment | ArchWind, 1) => {
            Some((1, AttackOrigin::ActorRoot, settings.effect_scale, false))
        }
        (PhotonTempest | PhotonBlast | PlasmaBlade, 0) => {
            Some((1, AttackOrigin::ActorAim, settings.effect_scale, true))
        }
        (Mjollnir, 0) => Some((1, AttackOrigin::ActorAim, Some(1.), true)),
        (FieryBeast | TTigerBlade, 0) => {
            Some((1, AttackOrigin::ActorRoot, settings.effect_scale, true))
        }
        (StardustRain | PrismStars, 0) => Some((1, AttackOrigin::World([0.; 3]), Some(1.), false)),
        _ => None,
    };
    if let Some((member, origin, scale, follow)) = initial {
        resources.prepared.effect_member(bank, member)?;
        attack
            .events
            .push((0, effect(member, origin, scale, follow)));
    }
    if role == 0 && matches!(family, PowBlade | PowDevastation | PowSpear) {
        anyhow::ensure!(
            !actor.temporary_weapons.is_empty(),
            "missing temporary combination weapon"
        );
        let member = if family == PowSpear { 4 } else { 5 };
        if !settings.end_bones.is_empty() {
            resources.prepared.effect_member(bank, member)?;
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
    let voice = match (family, role) {
        (MirageThrust, 1) => Some(if combination.id == 3 { 0x879e } else { 0x879f }),
        (DarkSerpent, 1) => Some(0x87a4),
        (StardustRain, 0) => Some(0x87bb),
        (LPunishment, 1) => Some(0x87b1),
        (PhotonTempest, 1) => Some(0x879a),
        (Mjollnir, 0) => Some(0x87ba),
        (PrismStars, 0) => Some(0x87c3),
        (PhotonBlast | PlasmaBlade, 1) => Some(0x87b3),
        _ => None,
    };
    if let Some(voice) = voice
        && let Some(sound) = (resources.sound)(Sound::Stream(voice & 0x7fff))?
    {
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
    if matches!(family, CrossThrust | MirageThrust | DarkSerpent) && role == 0 {
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
    let feedback = match family {
        StardustRain => Some((40, 140)),
        LPunishment | ArchWind => Some((30, 130)),
        PhotonTempest => Some((55, 40)),
        Mjollnir => Some((90, 30)),
        PrismStars => Some((30, 140)),
        PhotonBlast | PlasmaBlade => Some((70, 40)),
        _ => None,
    };
    if let Some((at, duration)) = feedback {
        attack.events.push((
            action::elapsed(source, at)?,
            AttackEvent::UnisonFeedback {
                duration,
                limit: if family == PrismStars { 12 } else { 8 },
            },
        ));
    }
    if family == StardustRain && role == 0 {
        attack.events.push((
            0,
            AttackEvent::WeaponVisibility(vec![
                (Some(resonance_battle::WeaponSlot(0)), false),
                (Some(resonance_battle::WeaponSlot(1)), false),
            ]),
        ));
        let choices = (1..=3)
            .map(|member| projectile(scene, member, 0, bank, resources))
            .collect::<Result<Arc<[_]>>>()?;
        for tick in 30..180 {
            attack.events.push((
                action::elapsed(source, tick)?,
                AttackEvent::ScatteredProjectile {
                    choices: Arc::clone(&choices),
                    offset: [0., 600., 0.],
                    step: [6., 2., 6.],
                    range: 100,
                },
            ));
        }
    }
    if family == PrismStars && role == 0 {
        let mut star = projectile(scene, 1, 0, bank, resources)?;
        Arc::make_mut(&mut star).velocity = [0., 0., 1.];
        let mut glint = Arc::clone(&star);
        Arc::make_mut(&mut glint).contact = None;
        for tick in 30..150 {
            attack.events.push((
                action::elapsed(source, tick)?,
                AttackEvent::ProjectileRing {
                    definition: Arc::clone(if (tick - 30) % 4 == 0 { &star } else { &glint }),
                    radius: 800.,
                    directions: 30,
                },
            ));
        }
    }
    let release = match (family, role) {
        (LTigerBlade, 0) => Some((35, 1, 1)),
        (FieryBeast, 0) => Some((66, 1, 2)),
        (TTigerBlade, 0) => Some((65, 1, 2)),
        (LPunishment, 1) => Some((30, 1, 0)),
        (ArchWind, 1) => Some((30, 1, 0)),
        (PhotonTempest, 1) => Some((60, 2, 1)),
        (Mjollnir, 0) => Some((42, 1, 0)),
        (Mjollnir, 1) => Some((86, 2, 1)),
        (PhotonBlast, 1) => Some((88, 2, 1)),
        (PlasmaBlade, 0) => Some((78, 3, 2)),
        (PlasmaBlade, 1) => Some((70, 2, 1)),
        _ => None,
    };
    if let Some((tick, member, rule)) = release {
        let definition = projectile(scene, member, rule, bank, resources)?;
        let at = action::elapsed(source, tick)?;
        if family == LTigerBlade {
            resources.prepared.effect_member(bank, 2)?;
            attack
                .events
                .push((at, effect(2, AttackOrigin::TargetRoot, None, true)));
        }
        if family == Mjollnir && role == 1 {
            resources.prepared.effect_member(bank, 4)?;
            attack
                .events
                .push((at, effect(4, AttackOrigin::TargetRoot, Some(1.), false)));
        }
        attack.events.push((
            at,
            AttackEvent::ProjectileAt {
                definition,
                origin: if family == Mjollnir && role == 0 {
                    AttackOrigin::ActorRoot
                } else if matches!(family, Mjollnir | LTigerBlade | FieryBeast) {
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

fn projectile(
    scene: &Scene,
    member: usize,
    rule: usize,
    bank: &EffectResource,
    resources: &mut Resources<'_>,
) -> Result<Arc<resonance_battle::ProjectileDefinition>> {
    let row = scene
        .projectiles
        .get(member)
        .context("missing combination projectile")?;
    // Callback hit rules are independent of projectile template selectors.
    let rule = row.hit_variant(rule)?;
    let recoil = resources.recoil;
    let mut hit = crate::battle::hit::authored(rule, recoil)?;
    if let Some(sound) = rule.sound {
        hit.sound = (resources.sound)(Sound::Cue(sound))?;
    }
    if let Some(member) = rule.effect {
        hit.effect = Some(resources.prepared.effect_member(bank, member)?);
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
        .cooldown = rule.cooldown;
    Ok(definition)
}
