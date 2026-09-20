mod defeat_only;
mod duel;
#[cfg(test)]
mod fire_tests;
mod guardian;
#[cfg(test)]
mod movement_motion_tests;
use super::*;
use crate::battle::all::ActorSettings;
pub(super) use crate::battle::casting_voices::VoiceDurations;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Parameters {
    pub(super) close_distance: f32,
    policies: [NativeParameters; 28],
}

impl Parameters {
    pub(super) fn read(rel: &Rel) -> Result<Self> {
        let close_distance = float(rel.at((4, 0x1b78))?, 0)?;
        ensure!(
            close_distance.is_finite() && close_distance >= 0.,
            "invalid enemy close distance"
        );
        Ok(Self {
            close_distance,
            policies: (0..28)
                .map(|kind| NativeParameters::read(kind, rel))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .ok()
                .context("enemy policy count")?,
        })
    }

    pub(super) fn bind(
        &self,
        kind: u8,
        metadata: &ActorSettings,
        lengths: &VoiceDurations,
    ) -> Result<EnemyNativePolicy> {
        self.policies
            .get(usize::from(kind))
            .context("unknown enemy native policy")?
            .bind(metadata, lengths)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeParameters {
    Ordinary,
    CarriedAnimationRate { rate: f32 },
    ThreeStations { stations: [EnemyStation; 3] },
    GuardianParts { wing_scale: f32 },
    BossPresentation,
    DefeatPresentation,
    ItemlessDuel,
    Unsupported { id: u8 },
}

impl NativeParameters {
    fn bind(self, metadata: &ActorSettings, lengths: &VoiceDurations) -> Result<EnemyNativePolicy> {
        let policy = match self {
            Self::Ordinary => EnemyNativePolicy::Ordinary,
            Self::CarriedAnimationRate { rate } => {
                ensure!(
                    metadata.model.attachment_count > 0,
                    "carried animation rate policy has no carried model"
                );
                EnemyNativePolicy::CarriedAnimationRate { rate }
            }
            Self::ThreeStations { stations } => EnemyNativePolicy::ThreeStations { stations },
            Self::GuardianParts { wing_scale } => guardian::policy(wing_scale, metadata, lengths)?,
            Self::BossPresentation => boss_policy(metadata, lengths)?,
            Self::DefeatPresentation => defeat_only::policy(metadata, lengths)?,
            Self::ItemlessDuel => EnemyNativePolicy::ItemlessDuel {
                defeat_ticks: defeat_only::duration(metadata, lengths)?,
            },
            Self::Unsupported { id } => EnemyNativePolicy::Unsupported { id },
        };
        policy.validate()?;
        Ok(policy)
    }
}

const CARRIED_RATE_CODE: &[u32] = &[
    0x4e800020, 0x80a31560, 0x3c800000, 0x38c40000, 0x38800001, 0xc0060000, 0x80a505fc, 0xd0050018,
    0x880301c6, 0x50802e34, 0x980301c6, 0x4e800020, 0x9421fff0, 0x7c0802a6, 0x3c800000, 0x90010014,
    0x38840000, 0x880301c6, 0x5400eefa, 0x7d84002e, 0x7d8903a6, 0x4e800421, 0x80010014, 0x7c0803a6,
    0x38210010, 0x4e800020,
];

#[cfg(test)]
pub(super) fn native_policy(
    kind: u8,
    rel: &Rel,
    metadata: &ActorSettings,
    lengths: &VoiceDurations,
) -> Result<EnemyNativePolicy> {
    NativeParameters::read(kind, rel)?.bind(metadata, lengths)
}

impl NativeParameters {
    fn read(kind: u8, rel: &Rel) -> Result<Self> {
        ensure!(kind <= 27, "unknown enemy native policy {kind}");
        Ok(match kind {
            0 => Self::Ordinary,
            5 => {
                ensure!(
                    !rel.pointers.contains_key(&(5, 0x54ac))
                        && word(rel.at((5, 0x54ac))?, 0)? == 0
                        && rel.pointer(5, 0x551c)? == (1, 0x73758)
                        && rel.pointer(5, 0x5a88)? == (1, 0x7372c)
                        && rel.pointer(5, 0x5a8c)? == (1, 0x73728),
                    "unexpected carried animation rate callbacks"
                );
                for (index, &instruction) in CARRIED_RATE_CODE.iter().enumerate() {
                    ensure!(
                        word(rel.at((1, 0x73728))?, index * 4)? == instruction,
                        "changed carried animation rate controller at {:#x}",
                        0x73728 + index * 4
                    );
                }
                let source = rel.at((4, 0x4c00))?;
                ensure!(
                    word(source, 0)? == 0x3e19999a,
                    "unexpected carried animation rate constant"
                );
                Self::CarriedAnimationRate {
                    rate: float(source, 0)?,
                }
            }
            11 => {
                ensure!(
                    rel.pointer(5, 0x5508 + 11 * 4)? == (1, 0x7e814)
                        && rel.pointer(5, 0x5d68)? == (1, 0x7e7f4)
                        && rel.pointer(5, 0x5d6c)? == (1, 0x7e6a8),
                    "unexpected three-station enemy callbacks"
                );
                Self::ThreeStations {
                    stations: decode_stations(rel.at((4, 0x6190))?, rel.at((4, 0x61b4))?)?,
                }
            }
            13 => Self::GuardianParts {
                wing_scale: guardian::read(rel)?,
            },
            20 => {
                ensure!(
                    rel.pointer(5, 0x5558)? == (1, 0x8aed0)
                        && rel.pointer(5, 0x54e8)? == (1, 0x8adb4)
                        && rel.pointer(5, 0x62d0)? == (1, 0x8ae08)
                        && rel.pointer(5, 0x62d4)? == (1, 0x8ade4),
                    "unexpected boss presentation callbacks"
                );
                Self::BossPresentation
            }
            25 => {
                defeat_only::validate(rel)?;
                Self::DefeatPresentation
            }
            27 => {
                duel::validate(rel)?;
                Self::ItemlessDuel
            }
            id => {
                ensure!(
                    rel.pointer(5, 0x5508 + usize::from(id) * 4)?.0 == 1,
                    "missing enemy native callback {id}"
                );
                Self::Unsupported { id }
            }
        })
    }
}

fn boss_policy(metadata: &ActorSettings, lengths: &VoiceDurations) -> Result<EnemyNativePolicy> {
    let voice = metadata.effects.opening_voice;
    let opening = BossVoice {
        id: voice,
        ticks: lengths.get(voice)?,
    };
    let policy = EnemyNativePolicy::BossPresentation {
        opening,
        defeat_ticks: defeat_ticks(metadata, lengths)?,
    };
    policy.validate()?;
    Ok(policy)
}

fn defeat_ticks(metadata: &ActorSettings, lengths: &VoiceDurations) -> Result<u16> {
    let death = metadata.effects.death_voice;
    let death = if death != 0 {
        death
    } else {
        metadata.effects.voice_base.wrapping_add(6) as u16
    };
    lengths.get(death)
}

#[cfg(test)]
fn empty_settings() -> ActorSettings {
    ActorSettings::read(&[0; crate::battle::embedded::SETTINGS_BYTES]).unwrap()
}

fn decode_stations(positions: &[u8], headings: &[u8]) -> Result<[EnemyStation; 3]> {
    let mut stations = [EnemyStation {
        position: [0.; 3],
        heading: 0.,
    }; 3];
    for (index, station) in stations.iter_mut().enumerate() {
        for axis in 0..3 {
            station.position[axis] = float(positions, index * 12 + axis * 4)?;
        }
        station.heading = float(headings, index * 4)?.to_radians();
    }
    EnemyNativePolicy::ThreeStations { stations }.validate()?;
    Ok(stations)
}

/// Enemy lookup takes the first matching native ID in the complete menu table.
pub(super) fn native_costs(catalogue: &crate::arte::Catalogue) -> BTreeMap<u16, u16> {
    let mut costs = BTreeMap::new();
    for row in &catalogue.definitions {
        costs
            .entry(row.native_id as u16)
            .or_insert(u16::from(row.tp_cost));
    }
    costs
}

/// Zero CAB slots leave the current animation running when movement is requested.
#[cfg(test)]
pub(super) fn absent_movement_motions(bytes: &[u8]) -> Result<BTreeSet<EnemyMovementMotion>> {
    [
        EnemyMovementMotion::Walk,
        EnemyMovementMotion::Run,
        EnemyMovementMotion::Stop,
    ]
    .into_iter()
    .map(|motion| Ok((motion, word(bytes, 0x20 + usize::from(motion.clip()) * 4)?)))
    .collect::<Result<Vec<_>>>()
    .map(|slots| {
        slots
            .into_iter()
            .filter_map(|(motion, offset)| (offset == 0).then_some(motion))
            .collect()
    })
}

pub(super) fn approach(row: &EnemyActionRecord) -> Result<EnemyApproach> {
    Ok(if row.movement_clip != 0 {
        let rate = row.movement_rate.finite()?;
        let speed = row.movement_speed.finite()?;
        ensure!(
            rate.is_finite() && rate > 0. && speed.is_finite() && speed >= 0.,
            "invalid enemy approach motion"
        );
        EnemyApproach::Custom {
            clip: row.movement_clip,
            rate,
            speed,
        }
    } else if row.requirements & 0x40 != 0 {
        EnemyApproach::Run
    } else {
        EnemyApproach::Walk
    })
}

pub(super) fn prepared_contact_recovery(
    row: &EnemyActionRecord,
    records: &binding::Records,
) -> Result<Option<EnemyContactRecovery>> {
    contact_recovery_with(
        row,
        |index| records.commands.select(index),
        || Ok(records.settings.appearance.idle_face),
    )
}

#[cfg(test)]
pub(super) fn contact_recovery(
    row: &EnemyActionRecord,
    stream: &[u8],
    metadata: &[u8],
) -> Result<Option<EnemyContactRecovery>> {
    contact_recovery_with(
        row,
        |index| {
            commands(
                stream
                    .get(index as usize * 2..)
                    .context("enemy contact recovery commands outside package")?,
            )
        },
        || {
            Ok(metadata
                .get(0xd7..0xdb)
                .context("missing enemy texture defaults")?
                .try_into()?)
        },
    )
}

fn contact_recovery_with(
    row: &EnemyActionRecord,
    select: impl FnOnce(i16) -> Result<(Vec<TimedCommand>, bool)>,
    texture_layers: impl FnOnce() -> Result<[u8; 4]>,
) -> Result<Option<EnemyContactRecovery>> {
    if row.requirements & 0x100 == 0 {
        return Ok(None);
    }
    let index = row.recovery_command_index as i16;
    ensure!(index >= 0, "negative enemy contact recovery command index");
    let commands = if index == 0 {
        Vec::new()
    } else {
        let (commands, looping) = select(index)?;
        ensure!(
            !looping && commands.iter().all(|command| command.tick == 0),
            "enemy contact recovery needs a one-shot entry stream"
        );
        commands
            .into_iter()
            .map(|command| command.command)
            .collect()
    };
    let recipe = EnemyContactRecovery {
        animation: (row.hit_recovery_clip != 0).then_some(AnimationCommand::Play {
            clip: row.hit_recovery_clip,
            blend: 4,
            start: 0,
            end: None,
            layer: 8,
            looping: false,
            mirror: false,
            resource: -1,
            rate: 0.5,
        }),
        commands,
        texture_layers: texture_layers()?,
    };
    recipe.validate()?;
    Ok(Some(recipe))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carried_rate_rel() -> Rel {
        let rodata = 4 + 0x73800;
        let data = rodata + 0x4c04;
        let mut rel = Rel {
            bytes: vec![0; data + 0x5a90],
            sections: vec![
                (0, 0),
                (4, 0x73800),
                (0, 0),
                (0, 0),
                (rodata, 0x4c04),
                (data, 0x5a90),
            ],
            pointers: [
                ((5, 0x551c), (1, 0x73758)),
                ((5, 0x5a88), (1, 0x7372c)),
                ((5, 0x5a8c), (1, 0x73728)),
            ]
            .into(),
            local_targets: Default::default(),
        };
        for (index, instruction) in CARRIED_RATE_CODE.iter().enumerate() {
            rel.bytes[4 + 0x73728 + index * 4..4 + 0x7372c + index * 4]
                .copy_from_slice(&instruction.to_be_bytes());
        }
        rel.bytes[rodata + 0x4c00..rodata + 0x4c04].copy_from_slice(&0x3e19999au32.to_be_bytes());
        rel
    }

    #[test]
    fn carried_rate_policy_requires_the_original_controller_dispatch_constant_and_model() {
        let mut rel = carried_rate_rel();
        let parameters = NativeParameters::read(5, &rel).unwrap();
        let parameters: NativeParameters =
            serde_json::from_value(serde_json::to_value(parameters).unwrap()).unwrap();
        let mut metadata = empty_settings();
        metadata.model.attachment_count = 1;
        assert_eq!(
            parameters
                .bind(&metadata, &VoiceDurations::default())
                .unwrap(),
            EnemyNativePolicy::CarriedAnimationRate {
                rate: f32::from_bits(0x3e19999a)
            }
        );
        assert!(EnemyNativePolicy::Unsupported { id: 5 }.validate().is_err());
        for rate in [0., -0.15, f32::NAN, f32::INFINITY] {
            assert!(
                EnemyNativePolicy::CarriedAnimationRate { rate }
                    .validate()
                    .is_err()
            );
        }
        for at in [(5, 0x551c), (5, 0x5a88), (5, 0x5a8c)] {
            let target = rel.pointers.remove(&at).unwrap();
            assert!(native_policy(5, &rel, &metadata, &VoiceDurations::default()).is_err());
            rel.pointers.insert(at, target);
        }
        rel.pointers.insert((5, 0x54ac), (1, 0x7372c));
        assert!(native_policy(5, &rel, &metadata, &VoiceDurations::default()).is_err());
        rel.pointers.remove(&(5, 0x54ac));
        // Changing the carried slot load, rate store or one-time selector store
        // must fail even when the table still points to the same routines.
        for address in [0x7372c, 0x73744, 0x73750] {
            let original = rel.bytes[4 + address..8 + address].to_vec();
            rel.bytes[4 + address..8 + address].copy_from_slice(&0x60000000u32.to_be_bytes());
            assert!(native_policy(5, &rel, &metadata, &VoiceDurations::default()).is_err());
            rel.bytes[4 + address..8 + address].copy_from_slice(&original);
        }
        let rate_at = rel.sections[4].0 + 0x4c00;
        rel.bytes[rate_at..rate_at + 4].copy_from_slice(&0.5f32.to_be_bytes());
        assert!(NativeParameters::read(5, &rel).is_err());
        assert!(
            parameters
                .bind(&metadata, &VoiceDurations::default())
                .is_ok()
        );
        rel.bytes[rate_at..rate_at + 4].copy_from_slice(&0x3e19999au32.to_be_bytes());
        metadata.model.attachment_count = 0;
        assert!(
            parameters
                .bind(&metadata, &VoiceDurations::default())
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires privately extracted GameCube records; reads all original enemy policies"]
    fn original_carried_rate_policy_has_only_volt_and_a_real_carried_animation() {
        use sha2::{Digest, Sha256};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&rel.bytes)),
            "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
        );
        let executable = fs::read(root.join("sys/main.dol")).unwrap();
        // fn_8006EB68 uses this rate before fn_1_15B20's initial model update.
        assert_eq!(
            word(dol::slice(&executable, 0x8035b8ac, 4).unwrap(), 0).unwrap(),
            0x3f000000
        );
        let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        let mut owners = Vec::new();
        for id in 0..251 {
            let start = word(&usual, table + id * 4).unwrap() as usize;
            let end = word(&usual, table + (id + 1) * 4).unwrap() as usize;
            let bytes = compression::decode(&archive[start..end]).unwrap();
            if bytes[usize::from(half(&bytes, 12).unwrap()) + 8] != 5 {
                continue;
            }
            owners.push(id);
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                "4a2c27e50216296a9b4e66b3cbba097cccd673dc5cdc9824f8b98d7f062bfa44"
            );
            let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
            let settings = ActorSettings::read(metadata).unwrap();
            assert_eq!(
                native_policy(
                    5,
                    &rel,
                    &settings,
                    &VoiceDurations::original(&usual).unwrap()
                )
                .unwrap(),
                EnemyNativePolicy::CarriedAnimationRate {
                    rate: f32::from_bits(0x3e19999a)
                }
            );
            assert_eq!((metadata[0x1e4], metadata[0x1e7]), (1, 0));
            let carried = word(&bytes, 0x160).unwrap() as usize;
            assert_eq!(carried, 375680);
            let package = &bytes[carried..];
            assert_eq!(word(package, 0).unwrap(), 5);
            assert_eq!(word(package, 8).unwrap(), 96); // Model member1.
            assert_eq!(word(package, 16).unwrap(), 76192); // ANM member3.
            assert!(carried + 76192 < bytes.len());
        }
        assert_eq!(owners, [198]);
    }

    #[test]
    fn native_dispatch_recovers_named_stations_and_keeps_other_handlers_explicit() {
        let metadata = empty_settings();
        let handlers = [
            0, 0x8217c, 0x5f504, 0x644f0, 0x72ecc, 0x73758, 0x74bc0, 0x76390, 0x7d0f4, 0x7d838,
            0x7e668, 0x7e814, 0x7f110, 0x7f480, 0x82238, 0x86be8, 0x86cf4, 0x879d0, 0x88fcc,
            0x89a34, 0x8aed0, 0x89a34, 0x8c408, 0x8dad4, 0x91cdc, 0x92f68, 0x93304, 0x93398,
        ];
        let mut rel = Rel {
            bytes: vec![0; 0x6204],
            sections: vec![(4, 0x6200); 6],
            pointers: handlers
                .into_iter()
                .enumerate()
                .skip(1)
                .map(|(index, target)| ((5, 0x5508 + index * 4), (1, target)))
                .chain([((5, 0x5d68), (1, 0x7e7f4)), ((5, 0x5d6c), (1, 0x7e6a8))])
                .collect(),
            local_targets: Default::default(),
        };
        for (index, value) in [
            1650_f32, 0., 0., -825., 0., 1428., -825., 0., -1428., -90., 150., 30.,
        ]
        .into_iter()
        .enumerate()
        {
            rel.bytes[0x6194 + index * 4..0x6198 + index * 4].copy_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            native_policy(0, &rel, &metadata, &VoiceDurations::default()).unwrap(),
            EnemyNativePolicy::Ordinary
        );
        for id in (1..=27).filter(|id| ![5, 11, 13, 20, 25, 27].contains(id)) {
            assert_eq!(
                native_policy(id, &rel, &metadata, &VoiceDurations::default()).unwrap(),
                EnemyNativePolicy::Unsupported { id }
            );
        }
        let parameters = NativeParameters::read(11, &rel).unwrap();
        let parameters: NativeParameters =
            serde_json::from_value(serde_json::to_value(parameters).unwrap()).unwrap();
        let EnemyNativePolicy::ThreeStations { stations } = parameters
            .bind(&metadata, &VoiceDurations::default())
            .unwrap()
        else {
            panic!("station controller")
        };
        assert_eq!(
            stations.map(|station| station.position),
            [[1650., 0., 0.], [-825., 0., 1428.], [-825., 0., -1428.]]
        );
        for (station, heading) in stations.into_iter().zip([-90., 150., 30.]) {
            assert!((station.heading.to_degrees() - heading).abs() < 0.0001);
        }
        assert!(native_policy(28, &rel, &metadata, &VoiceDurations::default()).is_err());
        rel.pointers.remove(&(5, 0x5d6c));
        assert!(native_policy(11, &rel, &metadata, &VoiceDurations::default()).is_err());
        assert!(decode_stations(&[0; 35], &[0; 12]).is_err());
        let invalid = [f32::NAN.to_be_bytes(); 9].concat();
        assert!(decode_stations(&invalid, &[0; 12]).is_err());
    }

    fn row(motion: u8, command: i16) -> EnemyActionRecord {
        let mut row = vec![0; ACTION_BYTES];
        row[8..12].copy_from_slice(&0x100_u32.to_be_bytes());
        row[0x35] = motion;
        row[0x36..0x38].copy_from_slice(&command.to_be_bytes());
        EnemyActionRecord::read(&row).unwrap()
    }

    fn bytes(words: &[i16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_be_bytes()).collect()
    }

    #[test]
    fn recovery_recipe_decodes_signed_scalars_masked_body_flag_and_truncated_timer() {
        // Initial word is outside the selected command stream. Operand bit zero,
        // byte truncation and signed tenths are independent instruction operands.
        let stream = bytes(&[99, 0, 5, 3, 0x101, 0, 9, -8, 0, 0, 40, -1]);
        let recipe = contact_recovery(&row(32, 1), &stream, &vec![0; 0xdb])
            .unwrap()
            .unwrap();
        assert!(matches!(
            recipe.animation,
            Some(AnimationCommand::Play {
                clip: 32,
                blend: 4,
                rate: 0.5,
                looping: false,
                ..
            })
        ));
        assert!(matches!(&recipe.commands[..], [
            ActionCommand::BodyPush { enabled: false, restore_after: 1 },
            ActionCommand::ForwardDeceleration(value), ActionCommand::ForwardSpeed(4.),
        ] if *value == -0.8));
        for (operand, timer, enabled, restored) in
            [(2, 0, true, 0), (1, 256, false, 0), (1, 255, false, 255)]
        {
            let recipe = contact_recovery(
                &row(0, 1),
                &bytes(&[0, 0, 5, operand, timer, -1]),
                &vec![0; 0xdb],
            )
            .unwrap()
            .unwrap();
            assert!(recipe.animation.is_none());
            assert!(
                matches!(recipe.commands[0], ActionCommand::BodyPush { enabled: actual, restore_after }
                if actual == enabled && restore_after == restored)
            );
        }
    }

    #[test]
    fn companion_104_action_3_preserves_flagged_recovery_voice_and_retained_motion() {
        // Original package row+0x36 = 70, command section 1456 + 70*2 = 1596.
        // Exact bytes: 0000 001b 8616 0002 0000 0000 0028 ffff.
        let mut stream = vec![0; 70 * 2];
        stream.extend(bytes(&[0, 27, 0x8616_u16 as i16, 2, 0, 0, 40, -1]));
        let recipe = contact_recovery(&row(0, 70), &stream, &vec![0; 0xdb])
            .unwrap()
            .unwrap();
        assert!(recipe.animation.is_none());
        assert_eq!(recipe.texture_layers, [0; 4]);
        assert!(matches!(
            &recipe.commands[..],
            [
                ActionCommand::Voice {
                    id: 34326,
                    priority: 2
                },
                ActionCommand::ForwardSpeed(4.),
            ]
        ));
    }

    #[test]
    fn recovery_stream_is_one_shot_and_never_reads_executable_fragments() {
        let metadata = vec![0; 0xdb];
        let empty = contact_recovery(&row(0, 0), &[], &metadata)
            .unwrap()
            .unwrap();
        assert!(empty.animation.is_none() && empty.commands.is_empty());
        for stream in [
            bytes(&[0, 1, 9, 8, -1]),
            bytes(&[0, 0, 9, 8, -2]),
            bytes(&[0, 0, -3, -1]),
            bytes(&[0, 0, 5, 1]),
        ] {
            assert!(contact_recovery(&row(32, 1), &stream, &metadata).is_err());
        }
        assert!(contact_recovery(&row(32, -1), &[], &metadata).is_err());
        assert!(contact_recovery(&row(32, 1), &[], &metadata).is_err());
        assert!(contact_recovery(&row(32, 0), &[], &[]).is_err());
        let mut unused = row(0, -1);
        unused.requirements = 0;
        assert!(contact_recovery(&unused, &[], &[]).unwrap().is_none());
        unused.movement_rate = crate::read::FloatOperand::from_bits(0x7fc12345);
        unused.movement_speed = crate::read::FloatOperand::from_bits(0xff800000);
        assert!(matches!(approach(&unused).unwrap(), EnemyApproach::Walk));
        unused.movement_clip = 1;
        assert!(approach(&unused).is_err());
    }
}

#[cfg(test)]
mod boss_tests {
    use super::*;

    #[test]
    fn boss_recipes_use_duration_keys_and_preserve_the_explicit_death_override() {
        // Original metadata f4/f6/104 and member12 lengths: Undine, Kratos,
        // second Pronyma, Gatekeeper. High-bit stream IDs alias duration keys.
        for (opening, opening_ticks, death_override, base, defeat_ticks) in [
            (34836_u16, 132_u16, 0_u16, 1284_u32, 235_u16),
            (34772, 177, 0, 909, 31),
            (34795, 145, 34797, 1145, 261),
            (34626, 113, 0, 1847, 137),
        ] {
            let mut metadata = empty_settings();
            metadata.effects.death_voice = death_override;
            metadata.effects.opening_voice = opening;
            metadata.effects.voice_base = base;
            let mut lengths = VoiceDurations {
                duration_ticks: vec![0; 0x8000],
            };
            for (id, ticks) in [
                (opening, opening_ticks),
                (
                    if death_override == 0 {
                        base as u16 + 6
                    } else {
                        death_override
                    },
                    defeat_ticks,
                ),
            ] {
                lengths.duration_ticks[usize::from(id & 0x7fff)] = ticks;
            }
            assert_eq!(
                boss_policy(&metadata, &lengths).unwrap(),
                EnemyNativePolicy::BossPresentation {
                    opening: BossVoice {
                        id: opening,
                        ticks: opening_ticks
                    },
                    defeat_ticks,
                }
            );
            metadata.effects.opening_voice = 0;
            assert_eq!(
                boss_policy(&metadata, &lengths).unwrap(),
                EnemyNativePolicy::BossPresentation {
                    opening: BossVoice { id: 0, ticks: 0 },
                    defeat_ticks
                }
            );
            assert!(boss_policy(&metadata, &VoiceDurations::default()).is_err());
            lengths.duration_ticks[usize::from(opening & 0x7fff)] = u16::MAX;
            assert!(lengths.remaining(opening).is_err());
        }
    }
}

#[cfg(test)]
mod boss_dispatch_tests {
    use super::*;

    #[test]
    fn boss_admission_checks_both_native_phases_and_the_independent_entrance_callback() {
        let callbacks = [
            ((5, 0x5558), (1, 0x8aed0)),
            ((5, 0x54e8), (1, 0x8adb4)),
            ((5, 0x62d0), (1, 0x8ae08)),
            ((5, 0x62d4), (1, 0x8ade4)),
        ];
        let mut rel = Rel {
            bytes: vec![],
            sections: vec![],
            pointers: callbacks.into(),
            local_targets: Default::default(),
        };
        let lengths = VoiceDurations {
            duration_ticks: vec![0; 8],
        };
        let metadata = empty_settings();
        assert_eq!(
            native_policy(20, &rel, &metadata, &lengths).unwrap(),
            EnemyNativePolicy::BossPresentation {
                opening: BossVoice { id: 0, ticks: 0 },
                defeat_ticks: 0,
            }
        );
        for (address, target) in callbacks {
            rel.pointers.remove(&address);
            assert!(native_policy(20, &rel, &metadata, &lengths).is_err());
            rel.pointers.insert(address, target);
        }
    }
}

#[cfg(test)]
pub(super) fn casting(
    bytes: &[u8],
    catalogue: &crate::arte::Catalogue,
    usual: &[u8],
    rel: &Rel,
) -> Result<BTreeMap<u8, EnemyCastRecipe>> {
    prepared_casting(
        &binding::Records::read(bytes)?,
        catalogue,
        &VoiceDurations::original(usual)?,
        float(rel.at((4, 0x1c84))?, 0)?,
    )
}

pub(super) fn prepared_casting(
    records: &binding::Records,
    catalogue: &crate::arte::Catalogue,
    lengths: &VoiceDurations,
    stored_release_rate: f32,
) -> Result<BTreeMap<u8, EnemyCastRecipe>> {
    let metadata = &records.settings;
    let animation = |clip, blend, start, looping, rate| AnimationCommand::Play {
        clip,
        blend,
        start,
        end: None,
        layer: 8,
        looping,
        mirror: false,
        resource: -1,
        rate,
    };
    let absent_motions = [
        EnemyCastingMotion::Chant,
        EnemyCastingMotion::Conclusion,
        EnemyCastingMotion::StoredRelease,
    ]
    .into_iter()
    .map(|motion| Ok((motion, records.motion_absent(motion.clip() as u8)?)))
    .collect::<Result<Vec<_>>>()?
    .into_iter()
    .filter_map(|(motion, absent)| absent.then_some(motion))
    .collect::<BTreeSet<_>>();
    records
        .rows
        .iter()
        .enumerate()
        .filter_map(|(id, row)| {
            let native = row.native_technique;
            EnemySpell::from_native(native).map(|spell| (id, row, native, spell))
        })
        .map(|(id, row, native, spell)| {
            ensure!(row.effect == 0, "unsupported enemy periodic casting effect");
            ensure!(
                metadata.casting.stored_recovery_clip == 0,
                "unsupported enemy spell recovery pose"
            );
            let menu = catalogue
                .definitions
                .iter()
                .find(|row| row.native_id as u16 == native)
                .context("enemy spell has no menu record")?;
            let flags = menu.flags;
            let duration = row.duration;
            let duration = if duration != 0 {
                duration
            } else {
                u16::try_from(
                    (i32::from(metadata.casting.base_ticks) + i32::from(menu.cast_time_adjustment))
                        .max(1),
                )?
            };
            let begin = row.cast_voices[0];
            let base = metadata.effects.voice_base;
            let begin = if begin == 0 && base != 0 {
                (base.wrapping_add(7) as u16) | 0x8000
            } else {
                begin
            };
            let voices = CastingVoices {
                begin,
                begin_remaining: if begin == 0 {
                    0
                } else {
                    lengths.remaining(begin)?
                },
                release: row.cast_voices[1],
            };
            let command_index = metadata.casting.command_index;
            let (commands, loop_commands) = if command_index == 0 {
                (vec![], false)
            } else {
                records.commands.select(command_index)?
            };
            let rate = metadata.casting.animation_rate;
            let stored = flags & 1 != 0;
            ensure!(
                stored == spell.stored(),
                "enemy spell storage differs from its native controller"
            );
            ensure!(
                rate.is_finite() && rate > 0.,
                "unsupported enemy casting motion"
            );
            let cast = EnemyCastRecipe {
                absent_motions: absent_motions.clone(),
                duration,
                tp: if row.tp == 0 { menu.tp_cost } else { row.tp },
                voices,
                animation: animation(11, 8, 0, metadata.casting.chant_looping, rate),
                loop_start: metadata.casting.loop_start,
                commands,
                loop_commands,
                pulse: if flags & 0x00400000 != 0 {
                    3
                } else if flags & 0x00800000 != 0 {
                    4
                } else {
                    5
                },
                release: animation(
                    if stored { 13 } else { 12 },
                    4,
                    0,
                    if stored {
                        metadata.casting.stored_release_looping
                    } else {
                        metadata.casting.release_looping
                    },
                    if stored { stored_release_rate } else { rate },
                ),
                release_effect: if flags & 0x00800000 != 0 && flags & 0x00400000 == 0 {
                    8
                } else {
                    7
                },
                resume: animation(
                    12,
                    metadata.casting.resume_blend_ticks,
                    metadata.casting.resume_start,
                    metadata.casting.release_looping,
                    rate,
                ),
                resume_loop_start: metadata.casting.resume_loop_start,
                early_release: (!stored).then_some(CastingConclusion {
                    animation: animation(
                        12,
                        metadata.casting.resume_blend_ticks,
                        4,
                        metadata.casting.release_looping,
                        rate,
                    ),
                    loop_start: metadata.casting.resume_loop_start,
                }),
            };
            Ok((id as u8, cast))
        })
        .collect()
}

#[cfg(test)]
mod casting_tests {
    use super::*;

    fn original_enemy(extracted: &Path, usual: &[u8], monster: u8) -> Vec<u8> {
        let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let table = word(usual, 0x2c).unwrap() as usize;
        let start = word(usual, table + usize::from(monster) * 4).unwrap() as usize;
        let end = word(usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
        compression::decode(&archive[start..end]).unwrap()
    }

    #[test]
    #[ignore = "requires the original extracted disc; no asset encoding"]
    fn original_capture_commands_keep_extension_throw_and_visibility() {
        use resonance_content::battle::effects::{EffectBank, EffectId, ProjectileContactResponse};
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let enemy = enemy_actions(&original_enemy(&extracted, &usual, 197), 197).unwrap();
        assert_eq!(enemy.actions.len(), 8);
        let action = &enemy.actions[2].action;
        assert_eq!(action.duration, 45);
        let commands = action
            .commands
            .iter()
            .filter(|step| {
                matches!(
                    step.command,
                    ActionCommand::ExtendAction(_)
                        | ActionCommand::HeldForwardSpeed(_)
                        | ActionCommand::HeldVerticalSpeed(_)
                        | ActionCommand::ReleaseHeld { .. }
                )
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            commands.as_slice(),
            [
                TimedCommand {
                    tick: 41,
                    command: ActionCommand::ExtendAction(60)
                },
                TimedCommand {
                    tick: 85,
                    command: ActionCommand::HeldForwardSpeed(6.)
                },
                TimedCommand {
                    tick: 85,
                    command: ActionCommand::HeldVerticalSpeed(16.)
                },
                TimedCommand {
                    tick: 85,
                    command: ActionCommand::ReleaseHeld { hitstun: 30 }
                },
            ]
        ));
        assert!(action.hits.iter().any(|hit| hit.rule.flags & 0x10 != 0));
        for monster in [8, 9] {
            let package = original_enemy(&extracted, &usual, monster);
            let enemy = enemy_actions(&package, monster).unwrap();
            let visibility: Vec<_> = enemy
                .actions
                .iter()
                .flat_map(|action| &action.action.commands)
                .filter_map(|step| match step.command {
                    ActionCommand::HeldVisibility(visible) => Some((step.tick, visible)),
                    _ => None,
                })
                .collect();
            assert_eq!(visibility, [(54, false), (140, true)]);
            let at = word(&package, 0x1c8).unwrap() as usize;
            let projectile = crate::battle::effects::projectile(
                &package[at..at + 400],
                EffectId {
                    bank: EffectBank::Enemy(monster),
                    id: 0,
                },
                0.001,
            )
            .unwrap();
            projectile.validate().unwrap();
            assert_eq!(
                projectile.behavior.contact_response,
                ProjectileContactResponse::BounceAwayOnBlock
            );
        }
    }

    #[test]
    #[ignore = "requires the original extracted disc; no asset encoding"]
    fn original_enemy_lightning_keeps_action_costs_voices_and_release_modes() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let mut enemies = Vec::new();
        // Literal original rows include Medusa's two costs and Drake's short override.
        for (monster, expected) in [
            (
                51,
                vec![(4, 217, 230, 26, 0, 0, 0, 0), (5, 219, 230, 28, 0, 0, 0, 0)],
            ),
            (
                52,
                vec![
                    (4, 217, 230, 100, 0, 0, 0, 0),
                    (5, 217, 230, 26, 0, 0, 0, 4),
                ],
            ),
            (73, vec![(3, 216, 180, 9, 0, 0, 0, 0x40)]),
            (101, vec![(6, 219, 120, 28, 34334, 107, 0, 0x40)]),
            (107, vec![(3, 217, 270, 26, 34580, 125, 34586, 0x40)]),
            (172, vec![(4, 217, 20, 30, 0, 0, 0, 0x20040)]),
            (
                247,
                vec![
                    (3, 217, 270, 26, 34580, 125, 34586, 0x40),
                    (4, 219, 270, 28, 34580, 125, 34586, 0x42),
                ],
            ),
        ] {
            let bytes = original_enemy(&extracted, &usual, monster);
            let casts = casting(
                &bytes,
                &crate::arte::read(&executable).unwrap(),
                &usual,
                &rel,
            )
            .unwrap();
            let mut enemy = enemy_actions(&bytes, monster).unwrap();
            assert_eq!(
                casts.keys().copied().collect::<Vec<_>>(),
                expected.iter().map(|r| r.0).collect::<Vec<_>>()
            );
            for (id, native, duration, tp, begin, threshold, release_voice, requirements) in
                expected
            {
                let cast = &casts[&id];
                assert_eq!(
                    cast.absent_motions,
                    if monster == 172 {
                        [
                            EnemyCastingMotion::Chant,
                            EnemyCastingMotion::Conclusion,
                            EnemyCastingMotion::StoredRelease,
                        ]
                        .into()
                    } else {
                        BTreeSet::new()
                    }
                );
                let action = enemy.actions.iter().find(|a| a.id == id).unwrap();
                assert_eq!(
                    (action.technique, action.selection.requirements),
                    (Some(native), requirements)
                );
                assert_eq!(
                    (
                        cast.duration,
                        cast.tp,
                        cast.voices.begin,
                        cast.voices.begin_remaining,
                        cast.voices.release
                    ),
                    (duration, tp, begin, threshold, release_voice)
                );
                assert_eq!(
                    (cast.pulse, cast.release_effect, cast.loop_start),
                    (3, 7, if matches!(monster, 107 | 247) { 20 } else { 0 })
                );
                assert!(cast.commands.is_empty() && !cast.loop_commands);
                assert!(matches!(cast.animation, AnimationCommand::Play {
                    clip: 11, blend: 8, start: 0, looping, rate: 0.5, ..
                } if looping == (monster != 101)));
                assert!(matches!(cast.release, AnimationCommand::Play {
                    clip, blend: 4, start: 0, looping: false, rate: 0.5, ..
                } if clip == if native == 216 { 12 } else { 13 }));
                assert!(matches!(
                    cast.resume,
                    AnimationCommand::Play {
                        clip: 12,
                        blend: 4,
                        start: 2,
                        looping: false,
                        rate: 0.5,
                        ..
                    }
                ));
                assert_eq!(cast.early_release.is_some(), native == 216);
                if let Some(early) = cast.early_release {
                    assert_eq!(early.loop_start, 0);
                    assert!(matches!(
                        early.animation,
                        AnimationCommand::Play {
                            clip: 12,
                            blend: 4,
                            start: 4,
                            looping: false,
                            rate: 0.5,
                            ..
                        }
                    ));
                }
            }
            enemy.casting = casts;
            enemies.push(enemy);
        }
        assert_eq!(
            required_techniques(&crate::arte::read(&executable).unwrap(), &[], &enemies).unwrap(),
            [78, 79, 81]
        );
        // Every existing controller, including the later Indignation owners, must
        // bind only its own recipe, even when another spell is also supported.
        let mut techniques =
            technique_actions(&extracted, &rel, &usual, &[78, 79, 80, 81]).unwrap();
        for technique in &mut techniques {
            let native = technique.native_id;
            let spell = technique.enemy_spell().unwrap();
            assert_eq!(spell as u16, native);
            assert_eq!(spell.stored(), native != 216);
            for wrong in [200, 201, 208, 209, 215, 216, 217, 218, 219, 220, 221, 222] {
                technique.native_id = wrong;
                assert_eq!(technique.enemy_spell().is_some(), wrong == native);
            }
            technique.native_id = native;
        }
        BattleActions {
            party: vec![],
            enemies,
            projectiles: vec![],
            techniques,
            chains: None,
        }
        .validate()
        .unwrap();
        // Newly supported spell dependencies must retain their original menu identity.
        let mut sibling = original_enemy(&extracted, &usual, 73);
        let row = usize::from(half(&sibling, 10).unwrap()) + 3 * ACTION_BYTES;
        assert_eq!(half(&sibling, row + 0x40).unwrap(), 216);
        let menus = dol::slice(
            &executable,
            0x80202f90,
            resonance_content::menu_data::TECHNIQUE_COUNT * 88,
        )
        .unwrap();
        for (native, menu) in [(221u16, 83u16), (222, 84)] {
            assert_eq!(half(menus, usize::from(menu) * 88).unwrap(), native);
            sibling[row + 0x40..row + 0x42].copy_from_slice(&native.to_be_bytes());
            assert_eq!(
                required_techniques(
                    &crate::arte::read(&executable).unwrap(),
                    &[],
                    &[enemy_actions(&sibling, 73).unwrap()]
                )
                .unwrap(),
                [menu]
            );
        }
        // An unknown native cannot disappear from the same complete enemy package.
        assert!(
            menus
                .chunks_exact(88)
                .all(|menu| half(menu, 0).unwrap() != u16::MAX)
        );
        sibling[row + 0x40..row + 0x42].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(
            required_techniques(
                &crate::arte::read(&executable).unwrap(),
                &[],
                &[enemy_actions(&sibling, 73).unwrap()]
            )
            .unwrap_err()
            .to_string(),
            "unsupported enemy native spell binding Chant(65535)"
        );
    }

    #[test]
    #[ignore = "requires the original extracted disc"]
    fn original_enemy_207_wind_chants_preserve_ordinary_and_stored_release() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let bytes = original_enemy(&extracted, &usual, 207);
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let menu = dol::slice(&executable, 0x80202f90, 72 * 88).unwrap();
        for (id, native, flags) in [(70, 208, 0x00444186), (71, 209, 0x0044018b)] {
            let row = &menu[id * 88..];
            assert_eq!(half(row, 0).unwrap(), native);
            assert_eq!(word(row, 0x34).unwrap(), flags);
        }
        let casts = casting(
            &bytes,
            &crate::arte::read(&executable).unwrap(),
            &usual,
            &rel,
        )
        .unwrap();
        assert_eq!(casts.keys().copied().collect::<Vec<_>>(), [2, 3]);
        for (id, duration, tp, release_voice, release_clip) in
            [(2, 60, 8, 34099, 12), (3, 90, 22, 34104, 13)]
        {
            let cast = &casts[&id];
            assert_eq!((cast.duration, cast.tp), (duration, tp));
            assert_eq!(
                (cast.pulse, cast.release_effect, cast.loop_start),
                (3, 7, 24)
            );
            assert_eq!(
                (
                    cast.voices.begin,
                    cast.voices.begin_remaining,
                    cast.voices.release
                ),
                (34099, 95, release_voice)
            );
            assert!(cast.commands.is_empty() && !cast.loop_commands);
            assert!(matches!(cast.animation, AnimationCommand::Play {
                clip: 11, blend: 8, start: 0, end: None, looping: true, rate, ..
            } if rate == 0.5));
            assert!(matches!(cast.release, AnimationCommand::Play {
                clip, blend: 4, start: 0, end: None, looping: false, rate, ..
            } if clip == release_clip && rate == 0.5));
            assert!(matches!(cast.resume, AnimationCommand::Play {
                clip: 12, blend: 4, start: 2, end: None, looping: false, rate, ..
            } if rate == 0.5));
            assert_eq!(cast.early_release.is_some(), id == 2);
            if let Some(early) = cast.early_release {
                assert_eq!(early.loop_start, 0);
                assert!(matches!(early.animation, AnimationCommand::Play {
                    clip: 12, blend: 4, start: 4, end: None, looping: false, rate, ..
                } if rate == 0.5));
            }
        }
    }

    #[test]
    #[ignore = "requires the original extracted disc"]
    fn original_enemy_205_recovers_its_ordinary_release_loop() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let bytes = original_enemy(&extracted, &usual, 205);
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        assert_eq!((metadata[0x30], metadata[0xa8]), (2, 30));
        let row = &bytes[usize::from(half(&bytes, 10).unwrap()) + 6 * ACTION_BYTES..];
        assert_eq!(
            (half(row, 0x40).unwrap(), half(row, 14).unwrap()),
            (208, 45)
        );
        assert_eq!((row[0x22], row[0x34]), (0, 0));
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let casts = casting(
            &bytes,
            &crate::arte::read(&executable).unwrap(),
            &usual,
            &rel,
        )
        .unwrap();
        assert_eq!(casts.keys().copied().collect::<Vec<_>>(), [6]);
        let cast = &casts[&6];
        assert_eq!((cast.duration, cast.tp), (45, 8));
        assert_eq!(
            (
                cast.voices.begin,
                cast.voices.begin_remaining,
                cast.voices.release
            ),
            (34073, 60, 0)
        );
        let early = cast.early_release.unwrap();
        assert_eq!(early.loop_start, 30);
        assert!(matches!(early.animation, AnimationCommand::Play {
            clip: 12, blend: 4, start: 4, end: None, looping: true, rate, ..
        } if rate == 0.5));
    }

    #[test]
    #[ignore = "requires the original extracted disc"]
    fn original_direct_spells_keep_slots_and_route_specific_admission() {
        use resonance_content::battle::action_program::CastSlot;
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        for (monster, bindings) in [
            (56, &[(1, 204, 45), (2, 208, 45), (3, 216, 45)][..]),
            (179, &[(3, 208, 20)][..]),
            (194, &[(1, 228, 25), (2, 228, 25)][..]),
        ] {
            let bytes = original_enemy(&extracted, &usual, monster);
            for &(action, native, tick) in bindings {
                let row = &bytes[usize::from(half(&bytes, 10).unwrap()) + action * ACTION_BYTES..];
                assert_eq!(half(row, 0x40).unwrap(), 0);
                let at = usize::from(half(&bytes, 14).unwrap())
                    + usize::from(half(row, 0x1a).unwrap()) * 2;
                let (steps, looping) = commands(&bytes[at..]).unwrap();
                assert!(!looping);
                let direct = steps
                    .iter()
                    .filter_map(|step| match step.command {
                        ActionCommand::CastNative { native_id, slot } => {
                            Some((step.tick, native_id, slot))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    direct,
                    [(
                        tick,
                        native,
                        if monster == 194 {
                            CastSlot::Primary
                        } else {
                            CastSlot::Secondary
                        }
                    )]
                );
            }
            if monster == 194 {
                let owner = enemy_actions(&bytes, monster).unwrap();
                assert_eq!(
                    owner.native_spells(),
                    BTreeSet::from([
                        EnemySpellBinding::Chant(213),
                        EnemySpellBinding::Chant(215),
                        EnemySpellBinding::Chant(228),
                        EnemySpellBinding::Direct(228),
                    ])
                );
                assert_eq!(
                    required_techniques(&crate::arte::read(&executable).unwrap(), &[], &[owner])
                        .unwrap(),
                    [75, 77, 90]
                );
                let definitions = technique_actions(&extracted, &rel, &usual, &[90]).unwrap();
                assert_eq!(
                    definitions[0].direct_spell(),
                    Some(DirectSpell::GravityWell)
                );
                assert!(EnemySpellBinding::Chant(228).matches(&definitions[0]));
                continue;
            }
            if monster != 56 {
                continue;
            }
            let mut owner = enemy_actions(&bytes, monster).unwrap();
            assert_eq!(
                owner.native_spells(),
                BTreeSet::from([
                    EnemySpellBinding::Chant(265),
                    EnemySpellBinding::Chant(278),
                    EnemySpellBinding::Direct(204),
                    EnemySpellBinding::Direct(208),
                    EnemySpellBinding::Direct(216),
                ])
            );
            assert_eq!(
                required_techniques(
                    &crate::arte::read(&executable).unwrap(),
                    &[],
                    &[owner.clone()]
                )
                .unwrap(),
                [66, 70, 78, 226, 246]
            );
            let definitions = technique_actions(&extracted, &rel, &usual, &[66, 70, 78]).unwrap();
            for (definition, direct) in definitions.iter().zip([
                DirectSpell::FireBall,
                DirectSpell::WindBlade,
                DirectSpell::Lightning,
            ]) {
                assert_eq!(definition.direct_spell(), Some(direct));
                assert!(EnemySpellBinding::Direct(direct as u16).matches(definition));
            }
            assert!(
                definitions[0].enemy_spell().is_none(),
                "direct Fire Ball must not invent a chant binding"
            );
            assert!(!EnemySpellBinding::Direct(208).matches(&definitions[0]));
            owner.actions[1].technique = Some(204);
            assert!(
                required_techniques(
                    &crate::arte::read(&executable).unwrap(),
                    &[],
                    &[owner.clone()]
                )
                .is_err()
            );
            owner.actions[1].technique = None;
            let command = owner.actions[1]
                .action
                .commands
                .iter_mut()
                .find(|step| matches!(step.command, ActionCommand::CastNative { .. }))
                .unwrap();
            command.command = ActionCommand::CastNative {
                native_id: 265,
                slot: CastSlot::Secondary,
            };
            assert!(
                required_techniques(&crate::arte::read(&executable).unwrap(), &[], &[owner])
                    .is_err(),
                "stored direct entry needs its own controller contract"
            );
        }
    }

    #[test]
    fn native_commands_keep_their_independent_slots_and_ignore_the_unused_operand() {
        use resonance_content::battle::action_program::CastSlot;
        let bytes = [
            0, 25, 0, 38, 0, 200, 0, 99, 0, 25, 0, 39, 0, 200, 0, 71, 255, 255,
        ];
        let (steps, looping) = commands(&bytes).unwrap();
        assert!(!looping);
        assert_eq!(steps.len(), 2);
        for (step, slot) in steps.iter().zip([CastSlot::Primary, CastSlot::Secondary]) {
            assert_eq!(step.tick, 25);
            assert!(
                matches!(step.command,ActionCommand::CastNative {native_id:200,slot:found} if found == slot)
            );
        }
        assert!(commands(&bytes[..7]).is_err());
    }

    #[test]
    #[ignore = "requires the original extracted disc"]
    fn original_undine_casting_and_secondary_entry_are_distinct() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        let start = word(&usual, table + 195 * 4).unwrap() as usize;
        let end = word(&usual, table + 196 * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let casts = casting(
            &bytes,
            &crate::arte::read(&executable).unwrap(),
            &usual,
            &rel,
        )
        .unwrap();
        assert_eq!(casts.keys().copied().collect::<Vec<_>>(), [2, 3, 4]);
        for (&id, cast) in &casts {
            assert_eq!(cast.duration, if id == 4 { 120 } else { 110 });
            assert_eq!(
                (cast.tp, cast.pulse, cast.release_effect, cast.loop_start),
                (22, 3, 7, 24)
            );
            assert_eq!(
                (
                    cast.voices.begin,
                    cast.voices.begin_remaining,
                    cast.voices.release
                ),
                (34059, 62, 34065)
            );
            assert!(cast.commands.is_empty() && !cast.loop_commands);
            assert!(
                matches!(cast.animation,AnimationCommand::Play {clip:11,blend:8,start:0,end:None,looping:true,rate,..} if rate == 0.5)
            );
            assert!(
                matches!(cast.release,AnimationCommand::Play {clip:13,blend:4,looping:false,rate,..} if rate == 0.5)
            );
            assert!(
                matches!(cast.resume,AnimationCommand::Play {clip:12,blend:4,start:2,looping:false,rate,..} if rate == 0.5)
            );
        }
        let enemy = enemy_actions(&bytes, 195).unwrap();
        let secondary = &enemy.actions[1];
        assert_eq!(secondary.technique, None);
        assert_eq!(secondary.action.duration, 50);
        assert!(secondary.action.commands.iter().any(|step| step.tick == 25
            && matches!(
                step.command,
                ActionCommand::CastNative {
                    native_id: 200,
                    slot: resonance_content::battle::action_program::CastSlot::Secondary
                }
            )));
        assert_eq!(
            required_techniques(&crate::arte::read(&executable).unwrap(), &[], &[enemy]).unwrap(),
            [62, 63]
        );
    }
}
