use super::*;
use resonance_battle::{Control, Cue, SoundBinding};
use resonance_content::{battle_action, battle_effect};

struct Original<'a>(&'a Files, Resources, EffectResource);

fn effect(member: u16) -> EffectResource {
    EffectResource {
        models: Default::default(),
        source: battle_effect::TECHNIQUES_PATH.into(),
        resource: 38,
        members: vec![member],
        scene: None,
    }
}

impl BattleResources for Original<'_> {
    fn effects(&mut self) -> Vec<EffectResource> {
        vec![self.2.clone()]
    }
    fn casting(&mut self, path: &str) -> Result<battle::casting::CastingResource> {
        self.1.casting(path)
    }
    fn voice(&mut self, path: &str) -> Result<Vec<Option<resonance_battle::VoiceLine>>> {
        use battle::{
            model::ModelSource,
            voice::{Phase, Sound},
        };
        let phase = match path.rsplit('/').next() {
            Some("chant") => Phase::Chant,
            Some("fallback") => Phase::Fallback,
            Some("release") => Phase::Release,
            _ => bail!("unexpected Fire Ball voice {path}"),
        };
        battle::voice::technique(
            self.0,
            &[ModelSource::Party(3), ModelSource::Enemy(49)],
            204,
            phase,
            |sound| {
                Ok(match sound {
                    Sound::Cue(index) => SoundBinding { resource: 1, index },
                    Sound::Stream(index) => SoundBinding { resource: 2, index },
                })
            },
        )
    }
    fn sound(&mut self, path: &str) -> Result<SoundBinding> {
        Ok(SoundBinding {
            resource: 1,
            index: path.rsplit('/').next().context("sound index")?.parse()?,
        })
    }
    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        if path == "battle/effects/techniques/fire_ball" {
            Ok(effect(23))
        } else {
            self.1.effect(path)
        }
    }
    fn particle(&mut self, path: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        self.1.particle(path)
    }
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        self.1.melee(path)
    }
    fn motion(&mut self, path: &str) -> Result<resonance_battle::MotionBinding> {
        self.1.motion(path)
    }
    fn spell(&mut self, path: &str) -> Result<u16> {
        self.1.spell(path)
    }
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        assert_eq!(path, "battle/projectiles/techniques/3");
        battle::fire_ball::projectile(self.0, &effect(23))
    }
}

#[test]
#[ignore = "requires current Genis model, casting and Fire Ball publications; no devices"]
fn cold_fire_ball_runs_the_original_three_projectiles_cost_voice_and_attachment_cleanup()
-> Result<()> {
    use battle::model::{self, ModelSource};
    let root = common::asset_root();
    let mut cache = resonance_content::prepared::Cache::default();
    let files = Files::load(&root, &["fields/map-340.preload.json"], &mut cache, || {
        false
    })?;
    let files = model::load_files(
        &root,
        files,
        &[ModelSource::Party(3), ModelSource::Enemy(49)],
        &mut cache,
        || false,
    )?;
    for (name, source) in [
        (
            "fire_ball",
            include_bytes!("../../../../scripts/battle/fire_ball.sym").as_slice(),
        ),
        (
            "genis_fire_ball",
            include_bytes!("../../../../scripts/battle/genis_fire_ball.sym").as_slice(),
        ),
    ] {
        assert_eq!(
            files.read(&format!("scripts/battle/{name}.sym"))?.as_ref(),
            source
        );
    }
    let menu: resonance_content::menu_data::MenuData = files.json("game/menu-data.json")?;
    let spells: battle_action::Table = files.json(battle_action::SPELL_PATH)?;
    let duration = spells.records[4]
        .as_ref()
        .context("Fire Ball action")?
        .phases[0]
        .duration;
    assert_eq!(duration, 90);
    let mut owner = actor();
    owner.control = Control::Auto;
    owner.stats.intelligence = 100;
    let (owner, model) = model::party(&files, 3, owner, super::lightning_cast::setup(&files, 1)?)?;
    let (mut target, target_model) = model::enemy(
        &files,
        &menu.monsters.records[49],
        1,
        super::lightning_cast::setup(&files, 2)?,
    )?;
    target.position = [0., 0., 400.];
    target.hp = 10000;
    target.max_hp = 10000;
    let tints: battle_effect::Tints = files.json(battle_effect::TINTS_PATH)?;
    let (feedback, common_members) = battle::contact_feedback::prepare(
        &tints,
        37,
        &[None, Some(resonance_battle::Element::Fire)].into(),
    )?;
    for interruption in [None, Some(10), Some(147)] {
        let mut resources = Original(
            &files,
            Resources {
                paths: vec![],
                fail: false,
            },
            EffectResource {
                source: battle_effect::COMMON_PATH.into(),
                resource: 37,
                members: common_members.clone(),
                scene: None,
                models: Default::default(),
            },
        );
        let prepared = battle::prepare(
            &mut PreparationCache::default(),
            &files,
            &battle::fire_ball::bindings(&files, 1, 100)?,
            vec![owner.clone(), target.clone()],
            1,
            &mut resources,
            vec![Some(model.clone()), Some(target_model.clone())],
        )?;
        let prepared = Arc::new(
            Arc::try_unwrap(prepared)
                .unwrap()
                .with_contact_feedback(feedback.clone())?,
        );
        let ids: Vec<_> = prepared.actor_ids().collect();
        let mut active = Battle::new(prepared);
        let mut frame = active.step(BattleInput {
            actions: vec![ActionRequest {
                actor: ids[0],
                target: ids[1],
                action: 1,
            }],
            ..Default::default()
        })?;
        let casting = frame.actions[0].0;
        let mut payment = None;
        let mut releases = vec![];
        let mut births = vec![];
        let mut shots = vec![];
        let mut voices = vec![];
        let mut hits = vec![];
        let mut impacts = vec![];
        let mut trails = 0;
        for update in 1..=300 {
            frame = active.step(BattleInput {
                interrupt: if interruption == Some(update) {
                    vec![casting]
                } else {
                    vec![]
                },
                ..Default::default()
            })?;
            if frame.actors[0].tp != 40 {
                assert_eq!(frame.actors[0].tp, 33);
                payment.get_or_insert(update);
            }
            for cue in &frame.cues {
                match cue {
                    Cue::Released { .. } => releases.push(update),
                    Cue::ProjectileStarted { .. } => births.push(update),
                    Cue::Sound { sound, .. } if sound.index == 81 => shots.push(update),
                    Cue::Voice { sound, .. } => voices.push((update, sound.index)),
                    Cue::Hit { actor, result, .. } => {
                        assert_eq!(*actor, ids[1]);
                        assert!(result.amount > 0);
                        hits.push(update);
                    }
                    Cue::Effect { member: 25, .. } => trails += 1,
                    Cue::Effect {
                        resource: 37,
                        member: member @ (29 | 12),
                        ..
                    } => impacts.push((update, *member)),
                    _ => {}
                }
            }
            if let Some(projectile) = frame.projectiles.iter().find(|p| p.age == 0) {
                let index = births.len() - 1;
                assert_eq!(
                    projectile.position,
                    [[2., 186., 10.], [-6., 176., 10.], [6., 182., 10.]][index]
                );
                assert!(!projectile.contact_active);
                assert!(projectile.shadow.is_some());
            }
        }
        if interruption == Some(10) {
            assert!(releases.is_empty() && births.is_empty() && hits.is_empty());
            assert_eq!(payment, None);
            assert!(voices.is_empty());
            assert!(impacts.is_empty());
        } else {
            assert_eq!(payment, Some(141));
            assert_eq!(releases, [142]);
            assert_eq!(births, [146, 154, 162]);
            assert_eq!(shots, [145, 153, 161]);
            assert_eq!(voices, [(84, 248), (141, 303)]);
            assert_eq!(hits.len(), 3);
            assert_eq!(
                impacts,
                hits.iter()
                    .flat_map(|&update| [(update, 29), (update, 12)])
                    .collect::<Vec<_>>()
            );
            assert!(trails > 3);
        }
        assert!(frame.projectiles.is_empty());
        assert!(frame.particles.iter().all(|p| p.resource != 38));
    }
    Ok(())
}

#[test]
#[ignore = "requires the current common effect and tint publications; no devices"]
fn cold_guard_break_prepares_and_executes_original_polar_modifiers_bounce_and_follow() -> Result<()>
{
    use resonance_battle::{Element, ParticleFrame};
    use std::collections::BTreeMap;
    let root = common::asset_root();
    let files = Files::load(
        &root,
        &["fields/map-340.preload.json"],
        &mut Default::default(),
        || false,
    )?;
    let source: battle_effect::SourceBank = files.json(battle_effect::COMMON_PATH)?;
    let tints: battle_effect::Tints = files.json(battle_effect::TINTS_PATH)?;
    let (_, members) =
        battle::contact_feedback::prepare(&tints, 77, &[None, Some(Element::Fire)].into())?;
    assert_eq!(members, [0, 1, 2, 11, 12, 16, 29, 47]);
    let bank = battle::effect_program::load(
        &files,
        battle_effect::COMMON_PATH,
        77,
        &members,
        &mut super::effect_runtime::no_sound,
    )?;
    assert_eq!(source.particle(16)?.ground_restitution, Some(0.65));
    assert!(source.particle(17)?.follow_origin);
    let mut owner = actor();
    owner.body.center_offset = [0., 10., 0.];
    // This fixture observes the effect program alone. Keep its synthetic actor
    // launcher active so ordinary action recovery cannot draw from the stream.
    let (mut active, id) = super::effect_runtime::battle_from_actor(
        "battle::show_centered(effect, 0, battle::owner(), 1.0, false,
         battle::EffectTint { enabled: false, palette: 0, red: 0, green: 0, blue: 0 });
         await battle::wait_ticks(ticks(60));",
        bank.members,
        1,
        owner,
    );
    let mut previous = BTreeMap::<_, ParticleFrame>::new();
    let mut born = vec![];
    let mut bounces = 0;
    let mut frame = active.step(BattleInput {
        actions: vec![ActionRequest {
            actor: id,
            target: id,
            action: 1,
        }],
        ..Default::default()
    })?;
    for update in 0..48 {
        for particle in &frame.particles {
            if particle.age == 0 {
                born.push((particle.member, particle.state.clone()));
                assert_eq!(particle.origin, [0., 10., 0.]);
            }
            if particle.member == 17 {
                assert_eq!(particle.origin, frame.actors[0].body.center);
                assert_eq!(particle.draw_after, Some(id));
            }
            if particle.member == 16 {
                let absolute_y = particle.origin[1] + particle.state.offset[1];
                assert!(absolute_y >= 0.);
                if let Some(before) = previous.get(&particle.id) {
                    let integrated_y =
                        before.origin[1] + (before.state.offset[1] + before.state.velocity[1]);
                    if integrated_y < 0. {
                        assert_eq!(absolute_y, 0.);
                        assert_eq!(
                            particle.state.velocity[1],
                            (before.state.velocity[1] + before.state.acceleration[1]) * -0.65
                        );
                        bounces += 1;
                    }
                }
            }
        }
        previous = frame
            .particles
            .iter()
            .map(|particle| (particle.id, particle.clone()))
            .collect();
        if update < 47 {
            frame = active.step(BattleInput::default())?;
        }
    }
    let shards: Vec<_> = born
        .iter()
        .filter(|(member, _)| *member == 16)
        .map(|(_, state)| state)
        .collect();
    assert_eq!(shards.len(), 6);
    // Seed 1 produces radius 13.7 and Z rotation -313.5 degrees on the
    // first polar modifier. The source then replaces Y with 9.4; the
    // particle's first update applies its -1.4 vertical acceleration.
    for (actual, expected) in shards[0].velocity.into_iter().zip([9.430458, 8., 9.937629]) {
        assert!((actual - expected).abs() < 0.0001);
    }
    assert_eq!(born.iter().filter(|(member, _)| *member == 17).count(), 1);
    assert_eq!(
        shards
            .iter()
            .filter(|state| state.colors[0][..3] == [64, 64, 128])
            .count(),
        3
    );
    assert_eq!(
        shards
            .iter()
            .filter(|state| state.colors[0][..3] == [128, 64, 64])
            .count(),
        3
    );
    assert!(
        shards
            .iter()
            .all(|state| matches!(state.uv[0], 1 | 33) && state.uv[1..] == [33, 30, 30])
    );
    // Three first-color shards consume seven source draws each; the other
    // three add the unsigned halfword random/multiply/UV-add draw.
    let expected = (0..45).fold(1_u32, |state, _| {
        state.wrapping_mul(0x41c6_4e6d).wrapping_add(0x12d687)
    });
    assert_eq!(active.random_state(), expected);
    assert!(bounces > 0);
    assert!(frame.particles.is_empty());
    Ok(())
}
