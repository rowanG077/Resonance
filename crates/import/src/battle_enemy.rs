//! Decode enemy decisions, including model contacts required by their projectiles.
use crate::read::u16 as half;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_enemy::{Definition, EntranceCamera, Presentation, WeaponVisibility};
use resonance_content::battle_voice::TimedVoice;
use std::{fs, path::Path};

/// Refresh every enemy's behavior/profile metadata without converting its artwork.
pub fn publish_all(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    publish(extracted, output, false)
}

/// Refresh every enemy against body and effect descriptors already seeded into output.
/// Artwork is retained; newly declared palette windows are published as needed.
pub fn publish_cached(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    publish(extracted, output, true)
}

fn publish(extracted: &Path, output: &Path, cached: bool) -> Result<Vec<String>> {
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let archive = extracted.join("files").join(&sources.enemy);
    let voices = crate::battle_voice::read(&usual)?;
    let module = crate::rel::Rel::read(&extracted.join("files").join(&sources.module))?;
    let guard_bonus = crate::battle_recoil::guard_defaults(&module)?[0];
    let mut shared = cached
        .then(|| crate::battle_effect::SharedPalettes::read(&usual, output))
        .transpose()?;
    let mut paths = Vec::new();
    for id in 0..resonance_content::monster::MONSTER_COUNT {
        let bytes = crate::source_assets::enemy_package(&archive, &usual, id as u16)?;
        let definition = read(&bytes, &usual, id as u8, &voices, guard_bonus)?;
        let model_path = resonance_content::battle_model::enemy_path(id as u8);
        if cached || output.join(&model_path).is_file() {
            let mut model: resonance_content::battle_model::Enemy = serde_json::from_slice(
                &fs::read(output.join(&model_path))
                    .with_context(|| format!("missing cached enemy body {id}"))?,
            )
            .context("incompatible cached enemy descriptor; recook the asset library")?;
            ensure!(
                model.source_sha256 == definition.source_sha256,
                "cached enemy {id} body differs from source"
            );
            crate::battle_model::refresh_enemy_attachment_animation(&bytes, &mut model)?;
            if let Some(shared) = shared.as_mut() {
                crate::battle_model::refresh_enemy_attached_models(&bytes, &mut model)?;
                paths.push(
                    refresh_effects(&bytes, id as u8, &mut model, shared, output)
                        .with_context(|| format!("refresh cached enemy {id} effect bank"))?,
                );
            }
            let motions = crate::battle_model::publish_enemy_carried_motions(
                &bytes,
                &definition.actions,
                &mut model,
                output,
            )?;
            crate::write_atomic(&output.join(&model_path), &serde_json::to_vec(&model)?)?;
            paths.push(model_path);
            paths.extend(motions);
        }
        let path = resonance_content::battle_enemy::path(id as u8);
        crate::write_atomic(&output.join(&path), &serde_json::to_vec(&definition)?)?;
        paths.push(path);
    }
    Ok(paths)
}

fn refresh_effects(
    bytes: &[u8],
    enemy: u8,
    model: &mut resonance_content::battle_model::Enemy,
    shared: &mut crate::battle_effect::SharedPalettes,
    output: &Path,
) -> Result<String> {
    let path = resonance_content::battle_model::enemy_effects_path(enemy);
    let mut bank: resonance_content::battle_effect::SourceBank =
        serde_json::from_slice(&fs::read(output.join(&path))?)?;
    ensure!(
        bank.art
            .as_ref()
            .is_some_and(|art| art.source_sha256 == model.source_sha256),
        "cached enemy effect artwork differs from source"
    );
    if !model.attached_models.is_empty() {
        crate::battle_model::publish_enemy_effects(bytes, enemy, model, shared, output)?;
        return Ok(path);
    }
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    if let Some(source) = members.model(0x1cc)? {
        let (mut fresh, strides) =
            crate::battle_effect::read_enemy_with_palettes(source, &model.body.skeleton)?;
        ensure!(
            fresh.source_sha256 == bank.source_sha256 && fresh.actors.len() == bank.actors.len(),
            "cached enemy effect declarations differ from source"
        );
        let mut art = bank
            .art
            .take()
            .context("cached enemy effect artwork is missing")?;
        let mut textures = std::collections::BTreeMap::new();
        for (field, slot) in [(0x1d0, 2), (0x1d4, 14)] {
            if let Some(bytes) = members.model(field)? {
                textures.insert(
                    slot,
                    crate::battle_effect::art::cached_bank_textures(
                        bytes,
                        &fresh,
                        &strides,
                        slot,
                        art.textures
                            .get(&slot)
                            .context("cached enemy texture slot is missing")?,
                        output,
                    )
                    .with_context(|| format!("enemy texture slot {slot}"))?,
                );
            }
        }
        crate::battle_effect::art::bind_palettes(&mut fresh, &strides, &mut textures)?;
        shared.bind(&mut fresh, &strides)?;
        for (slot, textures) in textures {
            let textures: Vec<_> = textures.into_iter().map(|texture| texture.effect).collect();
            for image in textures.iter().flat_map(|texture| &texture.images) {
                if !art.files.contains_key(&image.path) {
                    art.files.insert(
                        image.path.clone(),
                        resonance_content::field_preload::File {
                            sha256: crate::media::hash_file(&output.join(&image.path))?,
                            bytes: fs::metadata(output.join(&image.path))?.len(),
                            roles: [resonance_content::field_preload::Role::Texture].into(),
                        },
                    );
                }
            }
            art.textures.insert(slot, textures);
        }
        model.files.extend(art.files.clone());
        fresh.art = Some(art);
        bank = fresh;
    } else {
        ensure!(
            bank.source_sha256 == model.source_sha256
                && bank.actors.is_empty()
                && bank.programs.is_empty(),
            "cached enemy has commands without an authored effect bank"
        );
    }
    let encoded = serde_json::to_vec(&bank)?;
    let entry = model
        .files
        .get_mut(&path)
        .context("cached enemy body omits its effect bank")?;
    entry.sha256 = crate::digest(&encoded);
    entry.bytes = encoded.len() as u64;
    crate::write_atomic(&output.join(&path), &encoded)?;
    Ok(path)
}

pub(crate) fn read(
    bytes: &[u8],
    usual: &[u8],
    enemy: u8,
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
) -> Result<Definition> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("missing enemy profile")?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let projectiles = members
        .model(0x1c8)?
        .map(|member| {
            let start = crate::read::u32(bytes, 0x1c8)? as usize;
            crate::battle_projectile::read_enemy_member(member, start, &members)
        })
        .transpose()?
        .unwrap_or_default();
    let strategy = enemy_strategy(bytes)?;
    let mut actions = crate::battle_action::read(
        bytes,
        &projectiles,
        crate::source_assets::section(usual, 8)?,
    )?;
    if enemy == 165 {
        use resonance_content::battle_action::{EnemyEvent, EnemyExecution};
        for row in &mut actions.rows {
            if let Some(EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. }) =
                &mut row.execution
            {
                for (_, event) in &mut attack.events {
                    if let EnemyEvent::Defeat { rewards } = event {
                        *rewards = false;
                    }
                }
            }
        }
    }
    let policy = usize::from(half(bytes, 12)?);
    match bytes.get(policy + 8) {
        Some(1) => ensure!(
            matches!(enemy, 105 | 165),
            "unprepared retained-body enemy {enemy}"
        ),
        Some(2) => ensure!(enemy == 191, "unprepared staged enemy {enemy}"),
        Some(4) => ensure!(
            matches!(enemy, 211..=214),
            "unprepared linked enemy {enemy}"
        ),
        _ => {}
    }
    if bytes.get(policy + 8) == Some(&13) {
        actions.policy.behavior = Some(guardian_parts(enemy)?);
    }
    let target_strategy =
        match resonance_content::battle_action::TargetPolicy::try_from(strategy[0]) {
            Ok(policy) => Some(policy),
            Err(error) => {
                actions
                    .policy
                    .unsupported_reason
                    .get_or_insert_with(|| error.to_string());
                None
            }
        };
    let (entry_row, guard_recovery_bonus) = guard_policy(strategy[2], default_guard_bonus)?;
    let mut profile =
        crate::battle_profile::read(metadata, voices, crate::battle_voice::ENEMY_ROLES)?;
    let presentation = presentation(bytes, metadata, &mut profile, voices)?;
    let motion_count = crate::read::u32(bytes, 0x14)?.max(31) as usize;
    ensure!(
        motion_count <= (0x160 - 0x20) / 4,
        "enemy motion count overlaps attachments"
    );
    let mut null_motions = std::collections::BTreeSet::new();
    for clip in 0..motion_count {
        if crate::read::u32(bytes, 0x20 + clip * 4)? == 0 {
            null_motions.insert(clip as u16);
        }
    }
    Ok(Definition {
        source_sha256: crate::digest(bytes),
        null_motions,
        profile,
        weapon_visibility: match enemy {
            105 => [(1, WeaponVisibility::Alternating)].into(),
            208 | 209 => (0..4)
                .map(|slot| (slot, WeaponVisibility::Hidden))
                .collect(),
            // Yggdrasill's shared model carries the Eternal Sword, but fights unarmed.
            236..=238 => [(0, WeaponVisibility::Hidden)].into(),
            _ => Default::default(),
        },
        projectiles,
        actions,
        target_strategy,
        entry_row,
        guard_recovery_bonus,
        presentation,
    })
}

fn guardian_parts(enemy: u8) -> Result<resonance_content::battle_action::EnemyBehavior> {
    const WING_SCALE: f32 = 0.5;
    const BACK_ARMS: [&str; 2] = ["Bone21_BArm_L", "Bone21_BArm_R"];
    const FRONT_ARMS: [&str; 2] = ["Bone11_FArm_L", "Bone11_FArm_R"];
    let hidden: Vec<_> = match enemy {
        208 => BACK_ARMS.to_vec(),
        209 => FRONT_ARMS.to_vec(),
        210 => BACK_ARMS.into_iter().chain(FRONT_ARMS).collect(),
        _ => anyhow::bail!("Guardian parts on an incompatible enemy {enemy}"),
    };
    Ok(
        resonance_content::battle_action::EnemyBehavior::ModelParts {
            bone_scales: std::iter::once(("kk06_Hane".to_owned(), WING_SCALE))
                .chain(hidden.into_iter().map(|name| (name.to_owned(), 0.)))
                .collect(),
        },
    )
}

fn presentation(
    bytes: &[u8],
    metadata: &[u8],
    profile: &mut resonance_content::battle_profile::Profile,
    voices: &crate::battle_voice::Source,
) -> Result<Option<Presentation>> {
    use resonance_content::battle_enemy::Opening;
    let policy = usize::from(half(bytes, 12)?);
    let selector = *bytes
        .get(policy + 8)
        .context("missing enemy presentation policy")?;
    if !matches!(
        selector,
        2 | 3 | 6 | 8 | 10 | 12 | 13 | 15 | 16 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27
    ) {
        return Ok(None);
    }
    let timed = |sound| -> Result<TimedVoice> {
        Ok(TimedVoice {
            sound,
            ticks: sound
                .map(|sound| voices.duration(sound))
                .transpose()?
                .unwrap_or(0),
        })
    };
    let defeat_override = crate::battle_voice::sound(half(metadata, 0xf4)?);
    if let Some(sound) = defeat_override {
        profile.voices.get_or_insert_with(Default::default).defeat = Some(sound);
    }
    let defeat = timed(profile.voices.as_ref().and_then(|voices| voices.defeat))?;
    let opening = match selector {
        3 | 6 | 8 | 19 | 20 | 23 | 24 => Some(Opening::Voice(timed(crate::battle_voice::sound(
            half(metadata, 0xf6)?,
        ))?)),
        _ => match selector {
            2 => Some(sword_dancer_entrance()),
            10 => Some(descending_entrance()),
            12 => Some(falling_entrance(bytes, metadata)?),
            15 => Some(duel_entrance()),
            21 => Some(confrontation_entrance()),
            22 => Some(betrayal_entrance(timed(crate::battle_voice::sound(half(
                metadata, 0xf6,
            )?))?)),
            26 => Some(final_entrance(profile)),
            _ => None,
        }
        .map(Opening::Entrance),
    };
    Ok(Some(Presentation { opening, defeat }))
}

fn stationary_camera(radius: f32, focus_height: f32, yaw: f32, elevation: f32) -> EntranceCamera {
    EntranceCamera {
        radius,
        height_radius: 1500.,
        focus_height,
        focus_height_step: 0.,
        yaw,
        yaw_step: 0.,
        maximum_yaw: None,
        elevation,
        elevation_step: 0.,
        minimum_elevation: elevation,
    }
}

fn confrontation_entrance() -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{
        Entrance, EntranceCue as Cue, EntranceShot, EntranceStaging,
    };
    use resonance_content::battle_voice::Sound;
    let opening = stationary_camera(1150., 75., -170., 5.);
    Entrance {
        ticks: 715,
        gravity: 0.,
        letterbox: 0,
        show_party: false,
        show_hud: false,
        flash: None,
        launches: vec![],
        cues: vec![
            (25, Cue::Voice(Sound::Stream(2047))),
            (
                40,
                Cue::Pose {
                    clip: 27,
                    rate: 0.2,
                    repeat: false,
                    blend: 10,
                },
            ),
            (300, Cue::Dissolve),
            (320, Cue::PartyVoice(Sound::Stream(2048))),
            (320, Cue::PartyActiveExpression),
            (530, Cue::Voice(Sound::Stream(2049))),
            (550, Cue::Dissolve),
            (
                650,
                Cue::Feedback {
                    duration: 50,
                    limit: 16,
                    alpha: 160,
                },
            ),
        ],
        camera: opening,
        staging: Some(EntranceStaging {
            character: 1,
            party_positions: vec![
                [-300., 0., 0.],
                [-450., 0., 200.],
                [-450., 0., 100.],
                [-600., 0., -200.],
            ],
            enemy_position: [300., 0., 0.],
            hide_party_at: Some(550),
            fallback: None,
            shots: vec![
                (
                    0,
                    EntranceShot {
                        party: true,
                        camera: opening,
                        lateral: 20.,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
                (
                    301,
                    EntranceShot {
                        party: false,
                        camera: stationary_camera(1300., 100., -10., 5.),
                        lateral: 19.75,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
                (
                    551,
                    EntranceShot {
                        party: false,
                        camera: stationary_camera(850., 100., 170., 5.),
                        lateral: 0.,
                        lateral_step: 0.,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
            ],
        }),
    }
}

fn sword_dancer_entrance() -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{Entrance, EntranceCamera, EntranceCue};
    Entrance {
        ticks: 360,
        gravity: 0.,
        letterbox: 0,
        show_party: true,
        show_hud: true,
        flash: None,
        launches: vec![],
        cues: vec![(
            280,
            EntranceCue::Feedback {
                duration: 175,
                limit: 8,
                alpha: 160,
            },
        )],
        camera: EntranceCamera {
            radius: 2000.,
            height_radius: 1000.,
            focus_height: 80.,
            focus_height_step: 0.,
            yaw: 0.,
            yaw_step: 0.46,
            maximum_yaw: Some(165.),
            elevation: 70.,
            elevation_step: -0.175,
            minimum_elevation: 10.,
        },
        staging: None,
    }
}

fn duel_entrance() -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{
        Entrance, EntranceCamera, EntranceCue as Cue, EntranceShot, EntranceStaging,
    };
    use resonance_content::battle_voice::Sound;
    let camera = stationary_camera(1150., 100., -170., 7.5);
    Entrance {
        ticks: 430,
        gravity: 0.,
        letterbox: 64,
        show_party: false,
        show_hud: false,
        flash: None,
        launches: vec![],
        cues: vec![
            (
                5,
                Cue::Pose {
                    clip: 23,
                    rate: 0.65,
                    repeat: false,
                    blend: 10,
                },
            ),
            (30, Cue::Voice(Sound::Stream(2008))),
            (150, Cue::Dissolve),
            (165, Cue::Voice(Sound::Stream(2009))),
            (
                165,
                Cue::PartyPose {
                    clip: 100,
                    rate: 0.5,
                    repeat: false,
                    blend: 10,
                },
            ),
            (165, Cue::PartyActiveExpression),
            (210, Cue::Sound(Sound::Cue(60))),
            (430, Cue::Dissolve),
        ],
        camera,
        staging: Some(EntranceStaging {
            character: 1,
            party_positions: vec![[-300., 0., 0.]],
            enemy_position: [300., 0., 0.],
            hide_party_at: None,
            fallback: None,
            shots: vec![
                (
                    0,
                    EntranceShot {
                        party: true,
                        camera,
                        lateral: 20.,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
                (
                    151,
                    EntranceShot {
                        party: false,
                        camera: EntranceCamera {
                            yaw: -10.,
                            ..camera
                        },
                        lateral: 19.75,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
            ],
        }),
    }
}

fn betrayal_entrance(fallback: TimedVoice) -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{
        Entrance, EntranceCue as Cue, EntranceShot, EntranceStaging,
    };
    use resonance_content::battle_voice::Sound;
    let opening = stationary_camera(1150., 75., -170., 5.);
    Entrance {
        ticks: 660,
        gravity: 0.,
        letterbox: 0,
        show_party: false,
        show_hud: false,
        flash: None,
        launches: vec![],
        cues: vec![
            (
                2,
                Cue::Pose {
                    clip: 0,
                    rate: 0.4,
                    repeat: true,
                    blend: 8,
                },
            ),
            (25, Cue::Voice(Sound::Stream(1998))),
            (210, Cue::Dissolve),
            (220, Cue::PartyVoice(Sound::Stream(1999))),
            (220, Cue::PartyActiveExpression),
            (330, Cue::Voice(Sound::Stream(2000))),
            (330, Cue::ActiveExpression),
            (330, Cue::Dissolve),
            (
                450,
                Cue::Pose {
                    clip: 27,
                    rate: 0.3,
                    repeat: false,
                    blend: 8,
                },
            ),
            (660, Cue::Dissolve),
        ],
        camera: opening,
        staging: Some(EntranceStaging {
            character: 1,
            party_positions: vec![
                [-300., 0., 0.],
                [-450., 0., 200.],
                [-450., 0., 100.],
                [-600., 0., -200.],
            ],
            enemy_position: [300., 0., 0.],
            hide_party_at: Some(330),
            fallback: Some(fallback),
            shots: vec![
                (
                    0,
                    EntranceShot {
                        party: true,
                        camera: opening,
                        lateral: 20.,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
                (
                    211,
                    EntranceShot {
                        party: false,
                        camera: stationary_camera(1000., 100., -10., 5.),
                        lateral: 19.75,
                        lateral_step: -0.25,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
                (
                    331,
                    EntranceShot {
                        party: false,
                        camera: stationary_camera(700., 100., 170., 5.),
                        lateral: 0.,
                        lateral_step: 0.,
                        vertical: -49.75,
                        vertical_step: 0.25,
                    },
                ),
                (
                    530,
                    EntranceShot {
                        party: false,
                        camera: stationary_camera(700., 100., 170., 5.),
                        lateral: 0.,
                        lateral_step: 0.,
                        vertical: 0.,
                        vertical_step: 0.,
                    },
                ),
            ],
        }),
    }
}

fn final_entrance(
    profile: &resonance_content::battle_profile::Profile,
) -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{Entrance, EntranceCamera, EntranceCue};
    Entrance {
        ticks: 210,
        gravity: 0.,
        letterbox: 0,
        show_party: false,
        show_hud: false,
        flash: None,
        launches: vec![],
        cues: vec![(210, EntranceCue::Dissolve)],
        camera: EntranceCamera {
            focus_height_step: 0.1,
            ..stationary_camera(
                1950.,
                profile.center_offset[1] * profile.model_scale - 20.,
                180.,
                9.,
            )
        },
        staging: None,
    }
}

fn falling_entrance(
    bytes: &[u8],
    metadata: &[u8],
) -> Result<resonance_content::battle_enemy::Entrance> {
    use resonance_content::battle_enemy::{Entrance, EntranceCamera, EntranceCue as Cue};
    use resonance_content::battle_voice::Sound;
    let mut launches = Vec::new();
    let mut cursor = usize::from(half(bytes, 14)?) + 2 * usize::from(half(metadata, 0xf2)?);
    loop {
        let age = half(bytes, cursor)?;
        if age == u16::MAX {
            break;
        }
        ensure!(
            half(bytes, cursor + 2)? == 1,
            "unsupported entrance motion command"
        );
        launches.push((age, f32::from(half(bytes, cursor + 4)? as i16) / 10.));
        cursor += 6;
    }
    Ok(Entrance {
        ticks: 300,
        staging: None,
        gravity: -1.,
        letterbox: 0,
        show_party: true,
        show_hud: true,
        flash: None,
        launches,
        cues: vec![
            (1, Cue::Height(1000.)),
            (
                1,
                Cue::Pose {
                    clip: 36,
                    rate: 0.5,
                    repeat: false,
                    blend: 4,
                },
            ),
            (1, Cue::Expression(4)),
            (10, Cue::Effect(1)),
            (
                86,
                Cue::Pose {
                    clip: 37,
                    rate: 0.4,
                    repeat: false,
                    blend: 4,
                },
            ),
            (
                86,
                Cue::Shake {
                    duration: 45,
                    amplitude: 16,
                },
            ),
            (86, Cue::Sound(Sound::Cue(93))),
            (86, Cue::Expression(1)),
            (86, Cue::Vertical(14.)),
            (104, Cue::Voice(Sound::Stream(2075))),
            (
                134,
                Cue::Pose {
                    clip: 27,
                    rate: 0.4,
                    repeat: false,
                    blend: 4,
                },
            ),
            (134, Cue::Expression(0)),
            (
                196,
                Cue::Pose {
                    clip: 0,
                    rate: 0.5,
                    repeat: true,
                    blend: 4,
                },
            ),
        ],
        camera: EntranceCamera {
            radius: 1750.,
            height_radius: 1200.,
            focus_height: 120.,
            focus_height_step: 0.,
            yaw: 180.,
            yaw_step: 0.01,
            maximum_yaw: None,
            elevation: 20.,
            elevation_step: -0.01,
            minimum_elevation: 16.5,
        },
    })
}

fn descending_entrance() -> resonance_content::battle_enemy::Entrance {
    use resonance_content::battle_enemy::{
        Entrance, EntranceCamera, EntranceCue as Cue, EntranceFlash,
    };
    use resonance_content::battle_voice::Sound;
    Entrance {
        ticks: 570,
        staging: None,
        gravity: 0.,
        letterbox: 0,
        show_party: true,
        show_hud: true,
        launches: vec![],
        flash: Some(EntranceFlash {
            tick: 220,
            color: [128; 3],
            step: 16,
            hold: 4,
        }),
        cues: vec![
            (1, Cue::Height(100.)),
            (
                10,
                Cue::StageTint {
                    color: [28; 3],
                    duration: 540,
                    step: 2,
                },
            ),
            (10, Cue::Effect(1)),
            (
                220,
                Cue::Shake {
                    duration: 30,
                    amplitude: 24,
                },
            ),
            (
                220,
                Cue::Feedback {
                    duration: 160,
                    limit: 6,
                    alpha: 160,
                },
            ),
            (
                240,
                Cue::Pose {
                    clip: 44,
                    rate: 0.5,
                    repeat: false,
                    blend: 0,
                },
            ),
            (240, Cue::Vertical(-0.4)),
            (280, Cue::Voice(Sound::Stream(2077))),
            (
                490,
                Cue::Pose {
                    clip: 45,
                    rate: 0.3,
                    repeat: false,
                    blend: 8,
                },
            ),
        ],
        camera: EntranceCamera {
            radius: 1750.,
            height_radius: 1000.,
            focus_height: 120.,
            focus_height_step: 0.,
            yaw: 180.,
            yaw_step: 0.01,
            maximum_yaw: None,
            elevation: 22.,
            elevation_step: -0.01,
            minimum_elevation: 18.5,
        },
    }
}

fn guard_policy(
    value: u8,
    default_bonus: u8,
) -> Result<(resonance_content::battle_enemy::EntryRow, u8)> {
    use resonance_content::battle_enemy::EntryRow;
    let row = match value {
        0 => EntryRow::Random,
        1 => EntryRow::Front,
        2 => EntryRow::Middle,
        3 => EntryRow::Back,
        _ => anyhow::bail!("invalid enemy entry row {value}"),
    };
    let bonus = if value == 0 {
        default_bonus
    } else {
        resonance_content::battle_recoil::strategy_guard_recovery(value)
    };
    Ok((row, bonus))
}

fn enemy_strategy(bytes: &[u8]) -> Result<[u8; 3]> {
    let start = usize::from(half(bytes, 6)?);
    Ok(bytes
        .get(start..start + 3)
        .context("truncated enemy strategy")?
        .try_into()
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_policy_decodes_placement_and_resolves_actual_recovery_bonus() -> Result<()> {
        use resonance_content::battle_enemy::EntryRow;
        for (value, row, bonus) in [
            (0, EntryRow::Random, 7),
            (1, EntryRow::Front, 0),
            (2, EntryRow::Middle, 5),
            (3, EntryRow::Back, 25),
        ] {
            assert_eq!(guard_policy(value, 7)?, (row, bonus));
        }
        assert!(guard_policy(4, 7).is_err());
        Ok(())
    }
}
