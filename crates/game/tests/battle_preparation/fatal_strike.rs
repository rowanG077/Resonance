use super::*;
use anyhow::ensure;

#[cfg(feature = "test-support")]
fn record_motion_evidence(
    directory: &std::path::Path,
    character: u8,
    color: resonance_battle::FatalColor,
    frame: &resonance_battle::BattleFrame,
    trails: &[battle::encounter::RenderTrail],
    definition: &resonance_battle::ModelDefinition,
) -> Result<()> {
    use resonance_content::animation::{Matrix, transform_point};
    let scene = frame
        .fatal_strikes
        .as_ref()
        .and_then(|fatal| fatal.scene.as_ref())
        .context("missing audited performance")?;
    let model = frame
        .models
        .iter()
        .find(|m| m.actor == scene.performer)
        .context("missing audited body")?;
    let point =
        |world: Matrix, bone: Matrix| transform_point(world, [bone[3][0], bone[3][1], bone[3][2]]);
    let landmarks = definition
        .skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            b.name.ends_with("-hips")
                || [
                    "head",
                    "spine-03",
                    "wrist-left",
                    "wrist-right",
                    "hand-left",
                    "hand-right",
                    "leg-03-left",
                    "leg-03-right",
                ]
                .contains(&b.name.as_str())
        })
        .map(|(i, b)| (b.name.clone(), point(model.world, model.bones[i])))
        .collect::<std::collections::BTreeMap<_, _>>();
    let ribbons=frame.weapons.iter().filter(|weapon| weapon.owner==scene.performer)
        .filter_map(|weapon| trails.iter().find(|trail| trail.resource==weapon.resource).map(|trail| {
            serde_json::json!({"resource":weapon.resource.0,"visible":weapon.visible,"slot":weapon.slot.map(|slot| slot.0),
                "points":trail.bones.iter().filter_map(|bone| weapon.bones.get(usize::from(*bone))).map(|bone| point(weapon.world,*bone)).collect::<Vec<_>>()})
        })).collect::<Vec<_>>();
    let actor = &frame.actors[scene.performer.index()];
    let victim = &frame.actors[scene.target.index()];
    std::fs::create_dir_all(directory)?;
    if scene.age == 0 {
        let weapons = definition.weapons.iter().filter(|weapon| weapon.slot.is_some_and(|slot| slot.is_primary()))
            .map(|weapon| serde_json::json!({"slot":weapon.slot.unwrap().0,"attachment":weapon.attachment,
                "skeleton":weapon.primary_layer().skeleton,"playback":format!("{:?}",weapon.primary_layer().playback)})).collect::<Vec<_>>();
        std::fs::write(
            directory.join(format!("{character:02}-rig.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "skeleton":definition.skeleton,"weapons":weapons,"root_suppression":definition.suppress_root_translation,
                "clips":definition.motions.iter().map(|(id,motion)| (*id,motion.duration_frames)).collect::<std::collections::BTreeMap<_,_>>(),
            }))?,
        )?;
    }
    std::fs::write(
        directory.join(format!("{character:02}-{color:?}-{:03}.json", scene.age)),
        serde_json::to_vec_pretty(&serde_json::json!({
            "character":character,"color":format!("{color:?}"),"age":scene.age,"impact_at":scene.impact_at,
            "clip":model.clip,"frame":model.frame,"actor_position":actor.position,"heading":actor.heading,
            "scale":actor.body.scale,"hero_top":actor.body_top(),
            "camera":frame.camera.map(|camera|serde_json::json!({"eye":camera.eye,"focus":camera.focus,"fov":frame.camera_fov_degrees})),
            "target_position":victim.position,"target_center":victim.target_center(),"target_radius":victim.body_radius(),
            "landmarks":landmarks,"weapon_ribbons":ribbons,
        }))?,
    )?;
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires installed character rigs; validates authored limb arcs and support foot plants"]
fn fatal_clips_have_distinct_gestures_and_keep_support_feet_planted() -> Result<()> {
    let mut fixture = all_party_encounter::ColdEncounter::load()?;
    for character in 1..=9 {
        let mut party = resonance_events::party::Party::new(&fixture.session, Default::default())?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.preferences.fatal_strikes = true;
        let (_, prepared) = fixture.prepare(&party)?;
        let actor = prepared.core.actor_ids().next().unwrap();
        let model = prepared.core.models().unwrap().definition(actor).unwrap();
        let mut foot_bones = [
            model.skeleton.bone("leg-03-left").unwrap(),
            model.skeleton.bone("leg-03-right").unwrap(),
        ]
        .to_vec();
        if character == 8 {
            foot_bones.remove(0);
        }
        let hand = model
            .skeleton
            .bone(if character == 8 {
                "leg-03-left"
            } else if matches!(character, 1 | 6 | 7 | 9) {
                "wrist-right"
            } else {
                "wrist-left"
            })
            .unwrap();
        let mut hands = Vec::new();
        for color in resonance_battle::FatalColor::ALL {
            let performance = prepared
                .core
                .fatal_performance(actor, color)
                .context("missing caster performance")?;
            let motion = model
                .motions
                .get(&performance.poses[0].1.motion.clip)
                .context("missing authored caster clip")?;
            motion.validate(&model.skeleton)?;
            let initial = model.skeleton.sample(motion, 0.)?;
            let feet = foot_bones
                .iter()
                .map(|bone| initial.global[usize::from(*bone)][3])
                .collect::<Vec<_>>();
            let mut maximum_drift = 0_f32;
            for tick in 0..=112 {
                let pose = model.skeleton.sample(motion, tick as f32 * 0.5)?;
                for (&bone, planted) in foot_bones.iter().zip(&feet) {
                    let p = pose.global[usize::from(bone)][3];
                    let drift = (0..3)
                        .map(|a| (p[a] - planted[a]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    maximum_drift = maximum_drift.max(drift);
                }
            }
            ensure!(
                maximum_drift < 0.5,
                "character {character} {color:?}: feet slid {maximum_drift} units"
            );
            // Two strikes may share a contact point while using different
            // anticipation and follow-through silhouettes.
            hands.push(
                [18., 28., 39.].map(|at| {
                    model.skeleton.sample(motion, at).unwrap().global[usize::from(hand)][3]
                }),
            );
            eprintln!("FATAL_PLANT {character} {color:?} drift={maximum_drift:.4}");
        }
        for i in 0..3 {
            for j in i + 1..3 {
                let distance = (0..3)
                    .map(|phase| {
                        (0..3)
                            .map(|a| (hands[i][phase][a] - hands[j][phase][a]).powi(2))
                            .sum::<f32>()
                            .sqrt()
                    })
                    .fold(0_f32, f32::max);
                ensure!(
                    distance > 6.,
                    "character {character}: color gestures {i}/{j} overlap ({distance})"
                );
            }
        }
    }
    Ok(())
}

#[test]
#[cfg(feature = "test-support")]
#[ignore = "requires a current cooked library; exercises all nine character rigs"]
fn every_party_member_prepares_and_executes_all_three_fatal_performances() -> Result<()> {
    let mut fixture = all_party_encounter::ColdEncounter::load()?;
    for character in 1..=9 {
        if let Ok(selected) = std::env::var("RESONANCE_FATAL_AUDIT_CHARACTER")
            && selected.parse::<u8>()? != character
        {
            continue;
        }
        let mut party = resonance_events::party::Party::new(&fixture.session, Default::default())?;
        party.formation = vec![character];
        party.field_leader = character;
        party.settings.battle_controls = [0; 4];
        party.members[usize::from(character - 1)].base_stats[0] = 4096;
        party.members[usize::from(character - 1)].hp = 4096;
        party.settings.preferences.fatal_strikes = true;
        for color in resonance_battle::FatalColor::ALL {
            let (_, prepared) = fixture.prepare(&party).with_context(|| {
                format!("Fatal Strike preparation for character {character}, {color:?}")
            })?;
            if std::env::var_os("RESONANCE_FATAL_AUDIT_CHARACTER").is_some() {
                eprintln!(
                    "FATAL_HEAD_ANCHOR {character}: {:?}",
                    prepared.head_anchors[0]
                );
            }
            let head_anchor = prepared.head_anchors[0];
            let mut battle = prepared.core;
            let trails = prepared.trails;
            enter_battle(&mut battle)?;
            let owner = battle
                .actor_ids()
                .next()
                .context("missing finisher performer")?;
            let target = battle.target(owner).context("missing finisher target")?;
            for id in battle.actor_ids().collect::<Vec<_>>() {
                battle.set_actor_vitals(id, 20000, 20000, 9999, 9999)?;
                if battle.actors()[id.index()].side == Side::Enemy {
                    battle.set_fixture_neutral_control(id)?;
                }
            }
            let [x, _, z] = battle.actors()[target.index()].position;
            let distance = if character == 2 { 300. } else { 100. };
            battle.restore_fixture_placement(owner, [x - distance, 0., z], Some(90.))?;
            battle.restore_fixture_target(owner, target)?;
            battle.hold_event_boss_hp(Some(target))?;
            battle.configure_fatal_enemy(
                target,
                resonance_battle::FatalEnemyRules {
                    capacity: [1; 3],
                    outcome: resonance_battle::FatalOutcome::Damage,
                    vulnerability: std::array::from_fn(|index| {
                        if index == color.index() { 100 } else { 0 }
                    }),
                },
            )?;
            // Actual normal contacts drive the gauge; no test-only force-open API.
            let direction = match color {
                resonance_battle::FatalColor::Blue => [0, 70],
                resonance_battle::FatalColor::Red => [70, 0],
                resonance_battle::FatalColor::Green => [0, -70],
            };
            let mut opened = false;
            for tick in 0..420 {
                let mut input = resonance_battle::ControlInput::neutral(owner);
                input.stick = direction;
                input.attack = resonance_battle::ButtonInput {
                    pressed: tick % 24 == 0,
                    held: true,
                    released: false,
                };
                let mut frame = battle.step(input.into())?;
                let clock = frame.clock;
                battle.advance_models(&mut frame, clock, false)?;
                if frame.fatal_strikes.as_ref().is_some_and(|fatal| {
                    fatal.targets.iter().any(|target| target.opening.is_some())
                }) {
                    opened = true;
                    break;
                }
            }
            ensure!(
                opened,
                "character {character} did not open a {color:?} formula"
            );
            ensure!(
                battle.request_fatal_strike(owner)?,
                "character {character} could not claim a {color:?} formula"
            );
            let mut animated = false;
            let mut previous = None;
            let mut retimed = None;
            let mut equipment_restored = character != 3;
            for _ in 0..114 {
                let mut frame = battle.step(BattleInput::default())?;
                let clock = frame.clock;
                battle.advance_models(&mut frame, clock, false)?;
                if let Some(scene) = frame
                    .fatal_strikes
                    .as_ref()
                    .and_then(|fatal| fatal.scene.as_ref())
                {
                    if [0, 12, 22, 38, 50, 56, 62, 80, 94, 110].contains(&scene.age)
                        && let Some(directory) = std::env::var_os("RESONANCE_FATAL_MOTION_AUDIT")
                    {
                        record_motion_evidence(
                            std::path::Path::new(&directory),
                            character,
                            color,
                            &frame,
                            &trails,
                            battle.models().unwrap().definition(owner).unwrap(),
                        )?;
                    }
                    let model = frame
                        .models
                        .iter()
                        .find(|model| model.actor == owner)
                        .context("finisher model disappeared")?;
                    ensure!(model.visible, "finisher performer was hidden");
                    if [56, 78].contains(&scene.age) && !scene.style.caster() {
                        use glam::{Mat4, Vec3};
                        let camera = frame.camera.unwrap();
                        let eye = Vec3::from_array(camera.eye);
                        let forward = (Vec3::from_array(camera.focus) - eye).normalize();
                        let right = forward.cross(Vec3::Y).normalize();
                        let up = right.cross(forward);
                        let world = Mat4::from_cols_array_2d(&model.world);
                        let point = (world
                            * Mat4::from_cols_array_2d(
                                &model.bones[usize::from(head_anchor.bone)],
                            ))
                        .transform_point3(Vec3::ZERO)
                            + Vec3::Y
                                * head_anchor.offset[1]
                                * frame.actors[owner.index()].body.scale;
                        let offset = point - eye;
                        let y = offset.dot(up)
                            / (offset.dot(forward)
                                * (frame.camera_fov_degrees.unwrap().to_radians() * 0.5).tan());
                        ensure!(
                            y.abs() < 0.85,
                            "character {character} {color:?} cropped its head at {}: {y}",
                            scene.age
                        );
                    }
                    if scene.age >= 8 {
                        ensure!(
                            (model.frame - f32::from(scene.age) * 0.5).abs() < 0.001,
                            "finisher motion drifted from its impact clock at {}: {}",
                            scene.age,
                            model.frame
                        );
                    }
                    if scene.age == scene.impact_at && !matches!(character, 3 | 4) {
                        use resonance_content::animation::{multiply, transform_point};
                        let definition = battle.models().unwrap().definition(owner).unwrap();
                        let weapon = definition
                            .weapons
                            .iter()
                            .find(|weapon| weapon.slot == Some(resonance_battle::WeaponSlot(0)))
                            .unwrap();
                        let shown = frame
                            .weapons
                            .iter()
                            .find(|w| w.owner == owner && w.resource == weapon.primary_resource())
                            .unwrap();
                        let points = weapon
                            .primary_layer()
                            .skeleton
                            .bones
                            .iter()
                            .enumerate()
                            .filter(|(_, b)| b.name.to_ascii_lowercase().starts_with("at"))
                            .map(|(i, _)| {
                                transform_point(multiply(shown.world, shown.bones[i]), [0.; 3])
                            })
                            .collect::<Vec<_>>();
                        let contact: [f32; 3] = std::array::from_fn(|a| {
                            points.iter().map(|p| p[a]).sum::<f32>() / points.len() as f32
                        });
                        let victim = &frame.actors[target.index()];
                        let center = victim.target_center();
                        let gap = (contact[0] - center[0]).hypot(contact[2] - center[2]);
                        ensure!(
                            gap < 2.,
                            "character {character} {color:?} missed horizontally by {gap}: {contact:?} vs {center:?}"
                        );
                        ensure!(
                            contact[1] >= victim.position[1] - 5.
                                && contact[1] <= victim.body_top() + 5.,
                            "character {character} {color:?} missed the target's height: {contact:?} vs {center:?}"
                        );
                        eprintln!(
                            "FATAL_CONTACT {character} {color:?} horizontal_gap={gap:.4} height={:.2}",
                            contact[1]
                        );
                    }
                    if character == 3 {
                        ensure!(
                            frame
                                .weapons
                                .iter()
                                .filter(|weapon| weapon.owner == owner
                                    && weapon.slot.is_some_and(|slot| slot.is_primary()))
                                .all(|weapon| !weapon.visible),
                            "Genis retained his melee weapon during the cast"
                        );
                    }
                    if (22..94).contains(&scene.age) {
                        if let Some((clip, previous_frame)) = retimed {
                            ensure!(
                                model.clip == clip
                                    && model.frame + 0.0001 >= previous_frame
                                    && model.frame - previous_frame <= 4.001,
                                "character {character} snapped or restarted its {color:?} strike at {}",
                                scene.age
                            );
                        }
                        retimed = Some((model.clip, model.frame));
                    }
                    if (12..scene.impact_at).contains(&scene.age) {
                        if let Some(previous) = &previous {
                            animated |= *previous != model.bones;
                        }
                        previous = Some(model.bones.clone());
                    }
                } else if character == 3 {
                    equipment_restored |= frame.weapons.iter().any(|weapon| {
                        weapon.owner == owner
                            && weapon.slot.is_some_and(|slot| slot.is_primary())
                            && weapon.visible
                    });
                }
            }
            ensure!(
                animated,
                "character {character} did not animate during {color:?}"
            );
            ensure!(
                equipment_restored,
                "Genis did not recover his equipped weapon after {color:?}"
            );
            ensure!(
                battle.ledger().fatal_strikes.len() == 1,
                "character {character} did not finish {color:?}"
            );
        }
    }
    Ok(())
}
