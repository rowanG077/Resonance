use anyhow::{Context, Result, bail};
use resonance_battle::{ActionRequest, Actor, Battle, BattleInput, Side};
use resonance_content::prepared::Files;
use resonance_game::battle::{
    self, ActionBinding, BattleResources, EffectResource, HitResource, MeleeResource,
    ProjectileResource,
};
use std::sync::Arc;
use symphonia_script_tools::PreparationCache;
mod common;

#[path = "battle_preparation/companion_normals.rs"]
mod companion_normals;

#[path = "battle_preparation/martial.rs"]
mod martial;

#[path = "battle_preparation/death.rs"]
mod death;
#[path = "battle_preparation/enemy_attack.rs"]
mod enemy_attack;
#[path = "battle_preparation/normal_attack.rs"]
mod normal_attack;
#[path = "battle_preparation/victory.rs"]
mod victory;
#[path = "battle_preparation/voice.rs"]
mod voice;

#[path = "battle_preparation/entry.rs"]
mod entry;

#[path = "battle_preparation/encounter.rs"]
mod encounter;

#[path = "battle_preparation/audio_closure.rs"]
mod audio_closure;

#[path = "battle_preparation/party.rs"]
mod party;

#[path = "battle_preparation/player_control.rs"]
mod player_control;

#[path = "battle_preparation/effect_modifiers.rs"]
mod effect_modifiers;
#[path = "battle_preparation/effect_runtime.rs"]
mod effect_runtime;
#[path = "battle_preparation/effect_timeline.rs"]
mod effect_timeline;
#[path = "battle_preparation/fire_ball.rs"]
mod fire_ball;
#[path = "battle_preparation/lightning_cast.rs"]
mod lightning_cast;
#[path = "battle_preparation/lightning_effect.rs"]
mod lightning_effect;
#[path = "battle_preparation/nurse_cast.rs"]
mod nurse_cast;
#[path = "battle_preparation/nurse_models.rs"]
mod nurse_models;
#[path = "battle_preparation/nurse_recipients.rs"]
mod nurse_recipients;
#[path = "battle_preparation/nurse_trails.rs"]
mod nurse_trails;
#[path = "battle_preparation/projectile_effects.rs"]
mod projectile_effects;
#[path = "battle_preparation/projectile_source.rs"]
mod projectile_source;

#[path = "battle_preparation/normal_source.rs"]
mod normal_source;

#[path = "battle_preparation/recoil.rs"]
mod recoil;

#[path = "battle_preparation/profile.rs"]
mod profile;

#[path = "battle_preparation/casting.rs"]
mod casting;

#[path = "battle_preparation/model.rs"]
mod model;

#[path = "battle_preparation/scene.rs"]
mod scene;

fn actor_tints() -> resonance_battle::ResourceBinding {
    let tints: resonance_content::battle_effect::Tints =
        serde_json::from_str(include_str!("fixtures/effect-tints.json")).unwrap();
    resonance_battle::ResourceBinding::ActorTints(tints.actors)
}

fn actor() -> Actor {
    Actor {
        side: Side::Party,
        control: Default::default(),
        activity: Default::default(),
        availability: Default::default(),
        overlimit: 0,
        overlimit_active: false,
        guard: Default::default(),
        hp: 10,
        max_hp: 100,
        tp: 40,
        max_tp: 40,
        hud: Default::default(),
        luck: 0,
        stats: resonance_battle::CombatStats::default(),
        affinities: [resonance_battle::Affinity::Normal; 9],
        elements: Default::default(),
        attack_power: 100,
        physical_arte_boost: false,
        recovery: Default::default(),
        petrified: false,
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        framing: Default::default(),
        movement: Default::default(),
        hit_stop: 0,
        reaction: Default::default(),
        body: resonance_battle::Body::default(),
    }
}

fn files(source: &str) -> Files {
    let mut files = Files::default();
    files.bytes.insert(
        resonance_content::battle_recoil::PATH.into(),
        serde_json::to_vec(&recoil::source()).unwrap().into(),
    );
    let phase = serde_json::json!({
        "duration": 90, "recovery_ticks": 0, "buffer_until": 0,
        "combo_at": 0, "startup_effect": -1, "indices": [0, 0, 0, 0]
    });
    files.bytes.insert(
        "test-actions.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "source_sha256": "a".repeat(64), "records": [{
                "pool_offsets": [128, 156, 156, 156],
                "phases": [phase, phase, phase, phase],
                "hit_rules": [{
                    "flags": 0, "element": 10, "hitstun": 0, "contact_cooldown": 3,
                    "stun_chance": 0, "stagger": 0, "guard_pressure": 0,
                    "conditions": 0, "condition_chance": 0, "power_mode": 3, "power": 5,
                    "sound": 0, "armor_damage": 0, "knockback_delay": 0,
                    "impact_effect": 0, "condition_parameter": 0, "impact_bank": 0, "storage": []
                }], "hits": [], "animations": [], "commands": [], "storage": []
            }]
        }))
        .unwrap()
        .into(),
    );
    let actions: resonance_content::battle_action::Table = files.json("test-actions.json").unwrap();
    files.bytes.insert(
        "test-normal.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "source_sha256": "a".repeat(64), "weapon_flights": [], "groups": [{
                "selectors": [{"action":0,"allowed_directions":14,"fallback":255,"storage":0}],
                "actions": [{"descriptor":0,"hit":0,"animation":0,"command":0}],
                "descriptors": [], "hit_rules": actions.records[0].as_ref().unwrap().hit_rules,
                "hits": [{"start":0,"emission":1,"attachment_count":1,"emission_operands":[0,0,0,0],
                    "radius":30.0,"height":30.0,"shape":0,"damage_kind":0,"rule":0,"hit_class":1,
                    "reaction":0,"projectile_modifier":0,"inner_radius":0.0,"storage":[]}],
                "animations": [],"commands": [],"command_storage": []
            }]
        }))
        .unwrap()
        .into(),
    );
    files
        .bytes
        .insert("scripts/test.sym".into(), Arc::from(source.as_bytes()));
    let effects = resonance_content::battle_effect::SourceBank {
        art: None,
        source_sha256: "a".repeat(64),
        programs: vec![
            vec![resonance_content::battle_effect::Record {
                age: 0,
                command: 254,
                argument: 0,
                operand: 0,
            }];
            29
        ],
        actors: vec![],
        modifiers: Default::default(),
        uv: vec![],
        uv_roots: vec![],
    };
    files.bytes.insert(
        "test-effects.json".into(),
        serde_json::to_vec(&effects).unwrap().into(),
    );
    files.bytes.insert(
        "test-projectiles.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "source_sha256": "a".repeat(64),
            "records": [{
                "flags": 0x44a, "lifetime": 20,
                "ground_effect": {"bank": 0, "member": 0},
                "damage_kind": 2, "hit_class": 0, "reaction": 1,
                "shape": 1, "knockback": 1, "repeat_limit": 0,
                "velocity": [0., 0., 0.], "acceleration": [0., 0., 0.],
                "speed": 0., "bounce_restitution": 0., "radius": 10., "height": 20.,
                "inner_radius": 0., "growth": [0., 0.],
                "birth_effect": {"bank": 1, "member": 28},
                "trail_effect": {"bank": 1, "member": 0},
                "spawn_offset": [0., 0., 0.], "velocity_jitter": [0., 0., 0.],
                "hit_offset": [0., 0., 0.], "active_start": 4, "active_duration": 0,
                "shadow_color": [0, 0, 0, 0], "trail_interval": 0, "pulse_state": 0,
                "steering_blend": 0., "steering_end": 0, "steering_start": 0,
                "toward_bone": 0, "from_bone": 0, "update_mode": 0,
                "velocity_reset_age": 0, "storage": []
            }]
        }))
        .unwrap()
        .into(),
    );
    files
}
fn binding() -> ActionBinding {
    ActionBinding {
        phase: resonance_battle::ActionPhase::Resident,
        id: 1,
        module: "test".into(),
        entry: "run".into(),
        duration: 8,
        tp_cost: 2,
    }
}
struct Resources {
    paths: Vec<String>,
    fail: bool,
}
impl BattleResources for Resources {
    fn voice(&mut self, path: &str) -> Result<Vec<Option<resonance_battle::VoiceLine>>> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing actor voice");
        }
        if path.starts_with("battle/voices/techniques/genis/") {
            return Ok(vec![None; 2]);
        }
        Ok(vec![Some(resonance_battle::VoiceLine {
            sound: resonance_battle::SoundBinding {
                resource: 1,
                index: 43,
            },
            duration: 0,
        })])
    }

    fn casting(&mut self, path: &str) -> Result<battle::casting::CastingResource> {
        self.paths.push(path.into());
        let technique = path
            .rsplit('/')
            .next()
            .context("missing casting technique")?
            .parse()?;
        Ok(battle::casting::CastingResource {
            character: 3,
            technique,
            model: 1,
            stored_scene: None,
        })
    }
    fn sound(&mut self, path: &str) -> Result<resonance_battle::SoundBinding> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing battle sound {path}");
        }
        Ok(resonance_battle::SoundBinding {
            resource: 1,
            index: match path {
                "battle/sounds/common/104" => 104,
                "battle/sounds/common/109" => 109,
                "battle/sounds/common/123" => 123,
                _ => 60,
            },
        })
    }
    fn particle(&mut self, _: &str) -> Result<Arc<resonance_battle::ParticleDefinition>> {
        bail!("particle not supplied by this fixture")
    }
    fn spell(&mut self, path: &str) -> anyhow::Result<u16> {
        self.paths.push(path.into());
        if self.fail {
            anyhow::bail!("missing battle resource");
        }
        Ok(100)
    }
    fn motion(&mut self, path: &str) -> Result<resonance_battle::MotionBinding> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing battle motion {path}");
        }
        Ok(resonance_battle::MotionBinding { model: 1, clip: 1 })
    }
    fn melee(&mut self, path: &str) -> Result<MeleeResource> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing melee definition {path}");
        }
        Ok(MeleeResource {
            impact: None,
            source: "test-normal.json".into(),
            selection: battle::MeleeSelection::Normal {
                character: 0,
                selection: 0,
            },
            row: 0,
            anchor_groups: vec![vec![0]],
        })
    }
    fn projectile(&mut self, path: &str) -> Result<ProjectileResource> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing battle projectile {path}");
        }
        Ok(ProjectileResource {
            impact: None,
            trail: None,
            ground: None,
            source: "test-projectiles.json".into(),
            member: 0,
            hit: HitResource {
                source: "test-actions.json".into(),
                selection: battle::HitSelection::Technique {
                    member: 0,
                    phase: 0,
                },
                rule: 0,
            },
            birth: Some(EffectResource {
                models: Default::default(),
                scene: None,
                source: "test-effects.json".into(),
                resource: 37,
                members: vec![28],
            }),
            clash: None,
        })
    }
    fn effect(&mut self, path: &str) -> Result<EffectResource> {
        self.paths.push(path.into());
        if self.fail {
            bail!("missing battle effect {path}");
        }
        if path == "battle/scenes/237.json" {
            return Ok(EffectResource {
                models: Default::default(),
                source: "battle/scenes/237.effects.json".into(),
                resource: 237,
                members: vec![1, 2, 3, 4, 5, 6],
                scene: Some(battle::SceneResources {
                    technique: 237,
                    models: (0..4).map(|slot| (slot, 700 + u32::from(slot))).collect(),
                }),
            });
        }
        Ok(EffectResource {
            models: Default::default(),
            scene: None,
            source: if path == "battle/effects/common" {
                resonance_content::battle_effect::COMMON_PATH
            } else {
                "test-effects.json"
            }
            .into(),
            resource: 37,
            members: if path == "battle/effects/common" {
                vec![3, 7]
            } else {
                vec![6]
            },
        })
    }
}

#[test]
fn failed_replacement_preserves_the_running_generation_and_its_pending_wait() {
    let source = "script battle; use battle; pub task run() { await battle::at_age(ticks(2)); battle::heal_percent(battle::owner(), 40); }";
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let prepared = battle::prepare(
        &mut cache,
        &files(source),
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut current = Battle::new(prepared);
    current
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                action: 1,
                target: id,
            }],
            ..Default::default()
        })
        .unwrap();
    for invalid in [
        "script battle; pub task run(",
        "script field; pub task run() {}",
        "script battle; pub fn run() {}",
    ] {
        assert!(
            battle::prepare(
                &mut cache,
                &files(invalid),
                &[binding()],
                vec![actor()],
                1,
                &mut resources,
                vec![]
            )
            .is_err()
        );
    }
    assert!(
        battle::prepare(
            &mut cache,
            &Files::default(),
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    assert_eq!((current.actors()[0].hp, current.actors()[0].tp), (10, 40));
    current.step(BattleInput::default()).unwrap(); // First active visit observes age zero.
    current.step(BattleInput::default()).unwrap();
    current.step(BattleInput::default()).unwrap();
    assert_eq!(current.actors()[0].hp, 50);
}

#[test]
fn preparation_binds_resources_in_unexecuted_branches_before_activation() {
    let mut files = files(
        r#"script battle; use battle;
        asset bank: battle::Effect = "effects/recovery";
        pub task run() { if false { battle::show(bank, 6, battle::owner()); } }
    "#,
    );
    let mut effects: resonance_content::battle_effect::SourceBank =
        files.json("test-effects.json").unwrap();
    effects.programs[6].insert(
        0,
        resonance_content::battle_effect::Record {
            age: 0,
            command: 252,
            argument: 92,
            operand: 0,
        },
    );
    files.bytes.insert(
        "test-effects.json".into(),
        serde_json::to_vec(&effects).unwrap().into(),
    );
    let mut resources = Resources {
        paths: vec![],
        fail: true,
    };
    let mut cache = PreparationCache::default();
    assert!(
        battle::prepare(
            &mut cache,
            &files,
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    assert_eq!(resources.paths, ["effects/recovery"]);
    resources.fail = false;
    assert!(
        battle::prepare(
            &mut cache,
            &files,
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_ok()
    );
    assert_eq!(
        resources.paths,
        [
            "effects/recovery",
            "effects/recovery",
            "battle/sounds/common/92"
        ],
        "a compiler cache hit cannot skip resource verification"
    );
}

#[test]
fn changed_and_missing_transitive_imports_are_never_hidden_by_the_cache() {
    let mut snapshot = files(
        "script battle; use shared; use battle; pub task run() { battle::heal_percent(battle::owner(), shared::amount()); }",
    );
    snapshot.bytes.insert(
        "scripts/shared.sym".into(),
        Arc::from(b"script library; pub fn amount() -> i32 { return 20; }".as_slice()),
    );
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let old = battle::prepare(
        &mut cache,
        &snapshot,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    let cached = battle::prepare(
        &mut cache,
        &snapshot,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    snapshot.bytes.insert(
        "scripts/shared.sym".into(),
        Arc::from(b"script library; pub fn amount() -> i32 { return 30; }".as_slice()),
    );
    let new = battle::prepare(
        &mut cache,
        &snapshot,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    snapshot.bytes.remove("scripts/shared.sym");
    assert!(
        battle::prepare(
            &mut cache,
            &snapshot,
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    for (prepared, expected) in [(old, 30), (cached, 30), (new, 40)] {
        let id = prepared.actor_ids().next().unwrap();
        let mut battle = Battle::new(prepared);
        battle
            .step(BattleInput {
                actions: vec![ActionRequest {
                    actor: id,
                    target: id,
                    action: 1,
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.actors()[0].hp, expected);
    }
}

#[test]
fn cooked_battle_source_is_verified_again_on_cache_hits_without_fallback() {
    use resonance_content::{
        field_preload::{File, Inputs, Manifest, Role, VERSION},
        prepared::Cache,
    };
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "resonance-battle-sources-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("scripts")).unwrap();
    let source = b"script battle; pub task run() {}";
    let files = BTreeMap::from([
        (
            "field.json".to_owned(),
            File {
                sha256: format!("{:x}", Sha256::digest(b"{}")),
                bytes: 2,
                roles: [Role::Field].into(),
            },
        ),
        (
            "scripts/test.sym".to_owned(),
            File {
                sha256: format!("{:x}", Sha256::digest(source)),
                bytes: source.len() as u64,
                roles: [Role::Script].into(),
            },
        ),
    ]);
    let manifest = Manifest {
        version: VERSION,
        map_id: 1,
        inputs: Inputs {
            field: "field.json".into(),
            audio: Default::default(),
            movies: Default::default(),
        },
        missing_inputs: Default::default(),
        total_file_bytes: files.values().map(|file| file.bytes).sum(),
        files,
        scenes: vec![],
        features: Default::default(),
        scripts: vec![],
    };
    fs::write(root.join("field.json"), b"{}").unwrap();
    fs::write(root.join("scripts/test.sym"), source).unwrap();
    fs::write(
        root.join("preload.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let mut files_cache = Cache::default();
    let snapshot = Files::load(&root, &["preload.json"], &mut files_cache, || false).unwrap();
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    battle::prepare(
        &mut cache,
        &snapshot,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    // Same size: a metadata/length check alone cannot catch the edit.
    fs::write(
        root.join("scripts/test.sym"),
        b"script battle; pub task bad() {}",
    )
    .unwrap();
    let modified = Files::load(&root, &["preload.json"], &mut files_cache, || false);
    fs::remove_file(root.join("scripts/test.sym")).unwrap();
    let missing = Files::load(&root, &["preload.json"], &mut files_cache, || false);
    fs::remove_dir_all(root).unwrap();
    assert!(
        modified
            .err()
            .unwrap()
            .to_string()
            .contains("digest differs")
    );
    assert!(missing.is_err());
    // A retained active generation still owns precisely the old verified bytes.
    assert_eq!(&*snapshot.read("scripts/test.sym").unwrap(), source);
}

#[test]
fn mixed_resources_preserve_module_indices_and_projectile_failure_prevents_activation() {
    let files = files(
        r#"script battle; use battle;
        asset visuals: battle::Effect = "effects/cast";
        asset bolt: battle::Projectile = "projectiles/bolt";
        pub task run() {
            battle::show(visuals, 6, battle::owner());
            battle::emit(bolt, battle::ground_point(battle::owner(), 0.0, 0.0));
            battle::finish();
        }"#,
    );
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let prepared = battle::prepare(
        &mut cache,
        &files,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    assert_eq!(resources.paths, ["effects/cast", "projectiles/bolt"]);
    let owner = prepared.actor_ids().next().unwrap();
    let mut current = Battle::new(prepared);
    let start = current
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: owner,
                action: 1,
                target: owner,
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        start.cues[1],
        resonance_battle::Cue::Effect { member: 6, .. }
    ));
    assert_eq!(
        current
            .step(BattleInput::default())
            .unwrap()
            .projectiles
            .len(),
        1
    );
    resources.fail = true;
    let only_projectile = files_for_projectile();
    assert!(
        battle::prepare(
            &mut cache,
            &only_projectile,
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    assert_eq!(resources.paths.last().unwrap(), "projectiles/bolt");
    assert_eq!(
        current.step(BattleInput::default()).unwrap().projectiles[0].age,
        1
    );
}

fn files_for_projectile() -> Files {
    files(
        r#"script battle; use battle;
        asset bolt: battle::Projectile = "projectiles/bolt";
        pub task run() { if false { battle::emit(bolt, battle::ground_point(battle::owner(), 0.0, 0.0)); } }
    "#,
    )
}

#[test]
fn melee_source_compiles_on_load_and_all_contact_bindings_precede_activation() {
    let files = files(
        r#"script battle; use battle;
        asset hit: battle::Melee = "battle/melee/lloyd/right";
        asset unused: battle::Melee = "battle/melee/lloyd/left";
        pub task run() { await battle::hit_window(hit, ticks(0), ticks(1)); }
    "#,
    );
    let mut binding = binding();
    binding.phase = resonance_battle::ActionPhase::Actor;
    let mut owner = actor();
    owner.body.anchors.push([0.; 3]);
    let mut target = actor();
    target.side = Side::Enemy;
    target.body.points.push(resonance_battle::HurtPoint {
        center: [0.; 3],
        radius: 1.,
    });
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: Vec::new(),
        fail: false,
    };
    let prepared = battle::prepare(
        &mut cache,
        &files,
        &[binding.clone()],
        vec![owner.clone(), target.clone()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    assert_eq!(
        resources.paths,
        ["battle/melee/lloyd/right", "battle/melee/lloyd/left"]
    );
    let ids = prepared.actor_ids().collect::<Vec<_>>();
    let mut active = Battle::new(prepared);
    active
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: ids[0],
                target: ids[1],
                action: 1,
            }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(active.actors()[1].hp, 5);
    let mut changed = files.clone();
    let mut table: resonance_content::battle_action::NormalTable =
        changed.json("test-normal.json").unwrap();
    table.groups[0].hit_rules[0].stagger = 1;
    changed.bytes.insert(
        "test-normal.json".into(),
        serde_json::to_vec(&table).unwrap().into(),
    );
    let error = battle::prepare(
        &mut cache,
        &changed,
        &[binding.clone()],
        vec![owner.clone(), target.clone()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap_err();
    assert!(error.to_string().contains("knockdown resources"));
    assert_eq!(active.actors()[1].hp, 5);
    resources.fail = true;
    assert!(
        battle::prepare(
            &mut cache,
            &files,
            &[binding],
            vec![owner, target],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    assert_eq!(active.actors()[1].hp, 5);
    active.step(BattleInput::default()).unwrap();
    assert_eq!(active.actors()[1].hp, 5); // Same window preserves its struck marker.
}

#[test]
fn model_resources_are_verified_before_activation_and_failed_replacement_keeps_playback() {
    use resonance_battle::{ModelDefinition, Playback};
    use resonance_content::animation::{Bone, Motion, Skeleton, Transform, TransformChannels};
    let files = files(
        r#"script battle; use battle;
        asset clip: battle::Motion = "battle/motions/lloyd/30";
        pub task run() {
            await battle::animate(clip, ticks(4), 0.0, 0.5, false);
            await battle::animation_end();
            battle::heal_percent(battle::owner(), 10);
        }"#,
    );
    let mut binding = binding();
    binding.phase = resonance_battle::ActionPhase::Actor;
    binding.duration = 20;
    let model = Arc::new(ModelDefinition {
        secondary_motion: vec![],
        hurt_motions: [None; 2],
        idle_motions: [None; 2],
        guard_motions: [None; 2],
        stun: None,
        knockdown: None,
        resource: 1,
        skeleton: Skeleton {
            bones: vec![Bone {
                name: "root".into(),
                parent: None,
                bind_channels: TransformChannels(0),
                bind: Transform::default(),
            }],
        },
        motions: [(
            1,
            Motion {
                duration_frames: 3.,
                tracks: vec![],
            },
        )]
        .into(),
        initial: Playback {
            clip: 1,
            frame: 0.,
            rate: 0.,
            repeat: true,
        },
        anchors: vec![],
        approach_bones: vec![],
        target_bones: vec![],
        shadow: None,
        target_marker: None,
        weapons: vec![],
        hurt_bones: vec![],
        suppress_root_translation: [false; 3],
    });
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    assert!(
        battle::prepare(
            &mut cache,
            &files,
            &[binding.clone()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err(),
        "a compiled module cannot activate without its model"
    );
    let prepared = battle::prepare(
        &mut cache,
        &files,
        &[binding.clone()],
        vec![actor()],
        1,
        &mut resources,
        vec![Some(model.clone())],
    )
    .unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut active = Battle::new(prepared);
    active
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                target: id,
                action: 1,
            }],
            ..Default::default()
        })
        .unwrap();
    let before = active.step(BattleInput::default()).unwrap();
    resources.fail = true;
    assert!(
        battle::prepare(
            &mut cache,
            &files,
            &[binding],
            vec![actor()],
            1,
            &mut resources,
            vec![Some(model)]
        )
        .is_err()
    );
    let held = active
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(before.models, held.models);
    assert_eq!(before.actions, held.actions);
    for _ in 0..10 {
        active.step(BattleInput::default()).unwrap();
    }
    assert_eq!(active.actors()[0].hp, 20);
}

#[test]
fn sound_preparation_failure_keeps_the_active_sequence() -> Result<()> {
    let files = files(
        r#"script battle; use battle;
        asset swing: battle::Sound = "battle/sounds/common/60";
        pub task run() { await battle::at_age(ticks(2)); battle::sound(swing, 1); }
    "#,
    );
    let binding = binding();
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let ready = battle::prepare(
        &mut cache,
        &files,
        std::slice::from_ref(&binding),
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )?;
    assert_eq!(resources.paths, ["battle/sounds/common/60"]);
    let id = ready.actor_ids().next().unwrap();
    let mut active = Battle::new(ready);
    active.step(BattleInput {
        actions: vec![ActionRequest {
            actor: id,
            target: id,
            action: binding.id,
        }],
        ..Default::default()
    })?;
    resources.fail = true;
    let error = battle::prepare(
        &mut cache,
        &files,
        &[binding],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap_err();
    assert!(error.to_string().contains("missing battle sound"));
    assert_eq!(active.actors()[0].hp, 10);
    let mut sounds = vec![];
    for _ in 0..4 {
        sounds.extend(
            active
                .step(BattleInput::default())?
                .cues
                .into_iter()
                .filter(|cue| matches!(cue, resonance_battle::Cue::Sound { .. })),
        );
    }
    assert_eq!(
        sounds,
        [resonance_battle::Cue::Sound {
            actor: id,
            sound: resonance_battle::SoundBinding {
                resource: 1,
                index: 60
            },
            position: [0.; 3],
            priority: 1,
        }]
    );
    Ok(())
}

#[test]
fn spell_bindings_require_the_complete_compiled_generation_before_activation() {
    let root = r#"script battle; use battle;
        asset child: battle::Spell = "battle/spells/child";
        pub task run() { battle::release(child, false); battle::finish(); }
    "#;
    let mut snapshot = files(root);
    snapshot.bytes.insert("scripts/child.sym".into(), Arc::from(b"script battle; use battle; pub task run() { await battle::at_age(ticks(1)); battle::heal_percent(battle::owner(), 10); }".as_slice()));
    let mut parent = binding();
    parent.phase = resonance_battle::ActionPhase::Actor;
    let mut child = binding();
    child.id = 100;
    child.module = "child".into();
    child.tp_cost = 0;
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    assert!(
        battle::prepare(
            &mut cache,
            &snapshot,
            &[parent.clone()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    let prepared = battle::prepare(
        &mut cache,
        &snapshot,
        &[parent.clone(), child.clone()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    let owner = prepared.actor_ids().next().unwrap();
    let mut active = Battle::new(prepared);
    let first = active
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: owner,
                action: 1,
                target: owner,
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(
        first
            .cues
            .iter()
            .any(|c| matches!(c, resonance_battle::Cue::Released { .. }))
    );
    snapshot.bytes.insert(
        "scripts/child.sym".into(),
        Arc::from(b"script battle; pub task run(".as_slice()),
    );
    assert!(
        battle::prepare(
            &mut cache,
            &snapshot,
            &[parent, child],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    active.step(BattleInput::default()).unwrap(); // First active age zero.
    let healed = active.step(BattleInput::default()).unwrap();
    assert_eq!(healed.actors[0].hp, 20);
    assert_eq!(healed.actors[0].tp, 40);
}

#[test]
fn missing_or_invalid_effect_source_blocks_replacement_without_touching_live_tasks() {
    let source = "script battle; use battle; asset bank: battle::Effect = \"effects/cast\"; pub task run() { await battle::at_age(ticks(2)); battle::show(bank, 6, battle::owner()); battle::heal_percent(battle::owner(), 40); }";
    let valid = files(source);
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let prepared = battle::prepare(
        &mut cache,
        &valid,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    let id = prepared.actor_ids().next().unwrap();
    let mut live = Battle::new(prepared);
    live.step(BattleInput {
        actions: vec![ActionRequest {
            actor: id,
            target: id,
            action: 1,
        }],
        ..Default::default()
    })
    .unwrap();
    let age = live
        .step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap()
        .update;
    for bytes in [None, Some(b"{}".as_slice()), Some(b"not JSON".as_slice())] {
        let mut changed = valid.clone();
        changed.bytes.remove("test-effects.json");
        if let Some(bytes) = bytes {
            changed
                .bytes
                .insert("test-effects.json".into(), bytes.into());
        }
        assert!(
            battle::prepare(
                &mut cache,
                &changed,
                &[binding()],
                vec![actor()],
                1,
                &mut resources,
                vec![]
            )
            .is_err()
        );
        assert_eq!(
            live.step(BattleInput {
                menu_open: true,
                ..Default::default()
            })
            .unwrap()
            .update,
            age
        );
    }
    for _ in 0..3 {
        live.step(BattleInput::default()).unwrap();
    }
    assert_eq!(live.actors()[0].hp, 50);
}

#[test]
fn projectile_and_direct_effect_dependencies_share_one_verified_preparation() {
    let source = "script battle; use battle; asset bank: battle::Effect = \"effects/cast\"; asset p: battle::Projectile = \"test/projectile\"; pub task run() { battle::show(bank, 6, battle::owner()); battle::emit(p, battle::ground_point(battle::owner(), 0.0, 0.0)); battle::finish(); }";
    let valid = files(source);
    let mut cache = PreparationCache::default();
    let mut resources = Resources {
        paths: vec![],
        fail: false,
    };
    let ready = battle::prepare(
        &mut cache,
        &valid,
        &[binding()],
        vec![actor()],
        1,
        &mut resources,
        vec![],
    )
    .unwrap();
    let id = ready.actor_ids().next().unwrap();
    let mut live = Battle::new(ready);
    let first = live
        .step(BattleInput {
            actions: vec![ActionRequest {
                actor: id,
                target: id,
                action: 1,
            }],
            ..Default::default()
        })
        .unwrap();
    let second = live.step(BattleInput::default()).unwrap();
    let members: Vec<_> = first
        .cues
        .iter()
        .chain(&second.cues)
        .filter_map(|cue| match cue {
            resonance_battle::Cue::Effect { member, .. } => Some(*member),
            _ => None,
        })
        .collect();
    assert_eq!(members, [6, 28]);
    let mut changed = valid;
    let mut bank: resonance_content::battle_effect::SourceBank =
        changed.json("test-effects.json").unwrap();
    bank.programs.truncate(28);
    changed.bytes.insert(
        "test-effects.json".into(),
        serde_json::to_vec(&bank).unwrap().into(),
    );
    assert!(
        battle::prepare(
            &mut cache,
            &changed,
            &[binding()],
            vec![actor()],
            1,
            &mut resources,
            vec![]
        )
        .is_err()
    );
    assert_eq!(
        live.step(BattleInput {
            menu_open: true,
            ..Default::default()
        })
        .unwrap()
        .projectiles,
        second.projectiles
    );
}
