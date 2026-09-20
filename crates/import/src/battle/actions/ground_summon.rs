//! Recover each summon controller, captured origin and independent contacts.
use super::*;
use resonance_content::battle::actions::{
    earth_field::EarthFieldPulse,
    ground_summon::{
        GroundSummonKind, GroundSummonOrigin, GroundSummonRecipe, SummonBlessing, SummonLanes,
        SummonSound, SummonVoice, SummonWaves,
    },
    lightning::GroundSpellOrigin,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    kind: GroundSummonKind,
    lifetime: u16,
    origin: GroundSummonOrigin,
    pulses: Vec<stored_parameters::Pulse>,
    waves: Option<SummonWaves>,
    lanes: Option<SummonLanes>,
    sound: Option<SummonSound>,
    extra_voices: Vec<SummonVoice>,
    presentation: StoredSpellPresentation,
    effect_scale: f32,
    title: String,
    focus_distance: f32,
    focus_ticks: u16,
    voice: Option<u16>,
    voice_tick: u16,
    blessing: SummonBlessing,
    blessing_radius: f32,
}

pub(super) fn cook(
    tables: &Tables,
    technique: u16,
    arte: &Definition,
) -> Result<GroundSummonRecipe> {
    let p = tables
        .summons
        .ground
        .iter()
        .find(|p| p.kind.native() == arte.native_id as u16)
        .context("missing cooked ground summon parameters")?;
    ensure!(
        technique == p.kind.menu()
            && arte.flags == 0x20840191
            && arte.element == p.kind.element().map_or(0, |element| element as u8 + 1),
        "unexpected ground summon menu binding"
    );
    let bundle = tables.bundle(p.kind.native())?;
    let rules = match p.kind {
        GroundSummonKind::Sylph => 3,
        GroundSummonKind::Gnome | GroundSummonKind::Celsius => 2,
        _ => 1,
    };
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == rules,
        "unexpected ground summon phases or rule count"
    );
    let recipe = GroundSummonRecipe {
        kind: p.kind,
        lifetime: p.lifetime,
        origin: p.origin,
        pulses: p
            .pulses
            .iter()
            .map(|pulse| {
                Ok(EarthFieldPulse {
                    tick: pulse.tick,
                    projectile: p.kind.effect(pulse.projectile),
                    rule: bundle.phase_rule(0, pulse.rule)?,
                })
            })
            .collect::<Result<_>>()?,
        waves: p.waves.clone(),
        lanes: p.lanes.clone(),
        sound: p.sound,
        extra_voices: p.extra_voices.clone(),
        presentation: p.presentation,
        effect_scale: p.effect_scale,
        title: p.title.clone(),
        focus_distance: p.focus_distance,
        focus_ticks: p.focus_ticks,
        voice: p.voice,
        voice_tick: p.voice_tick,
        blessing: p.blessing,
        blessing_radius: p.blessing_radius,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, init, callback, cleanup, color, camera, scale, title, focus, lifetime, focus_tick) =
        match native {
            284 => (
                GroundSummonKind::Efreet,
                0x8dd4c,
                0x8db7c,
                0x8db0c,
                0x9028,
                0x9060,
                0x9070,
                0x9074,
                0x9080,
                0x8ddc8,
                0x8df50,
            ),
            287 => (
                GroundSummonKind::Gnome,
                0x8ee44,
                0x8ec2c,
                0x8ebbc,
                0x9240,
                0x926c,
                0x9274,
                0x9278,
                0x9280,
                0x8eea8,
                0x8efbc,
            ),
            285 => (
                GroundSummonKind::Undine,
                0x8e4b0,
                0x8e024,
                0x8dfb4,
                0x90d8,
                0x9134,
                0x9130,
                0x913c,
                0x9148,
                0x8e514,
                0x8e5e4,
            ),
            293 => (
                GroundSummonKind::Origin,
                0x90194,
                0x8ffa4,
                0x8ff34,
                0x9550,
                0x957c,
                0x958c,
                0x9590,
                0x959c,
                0x901f8,
                0x90378,
            ),
            289 => (
                GroundSummonKind::Volt,
                0x87d70,
                0x87bc8,
                0x87b58,
                0x8108,
                0x8138,
                0x8144,
                0x8148,
                0x8150,
                0x87dd4,
                0x87ef8,
            ),
            291 => (
                GroundSummonKind::Shadow,
                0x8f7f4,
                0x8f624,
                0x8f5b4,
                0x9370,
                0x93a0,
                0x93b0,
                0x93b4,
                0x93c0,
                0x8f858,
                0x8f9d8,
            ),
            286 => (
                GroundSummonKind::Sylph,
                0x8e974,
                0x8e69c,
                0x8e62c,
                0x91a0,
                0x91cc,
                0x91dc,
                0x91e0,
                0x91e8,
                0x8e9d8,
                0x8eb58,
            ),
            288 => (
                GroundSummonKind::Celsius,
                0x8f444,
                0x8f090,
                0x8f020,
                0x92d8,
                0x9300,
                0x92f0,
                0x930c,
                0x9318,
                0x8f494,
                0x8f550,
            ),
            native => bail!("unsupported ground summon native {native}"),
        };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(kind.native() - 200) * 4)?;
    for (phase, handler) in [init, 0x37e48, cleanup].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected ground summon dispatch phase {phase}"
        );
    }
    let bodies = match kind {
        GroundSummonKind::Sylph => [
            (
                init,
                0x248,
                "e3782f01afd1d22f81498f88f00ce37f66c6af7662497105f22ca76bd9f7ab25",
            ),
            (
                callback,
                0x2d8,
                "f3260352a16246bcf43f59b48c61573c2308ab77e879f3057d73c14d274fe379",
            ),
            (
                cleanup,
                0x70,
                "6ce4e6850cb6576a956a83bc23509d248c52b5acdbef1d4184b0354c99a59473",
            ),
        ],
        GroundSummonKind::Celsius => [
            (
                init,
                0x170,
                "1ced0d6585f6f9224efb3a75713b5cc7ccd6a7ca424871ef240a237b0f58f64a",
            ),
            (
                callback,
                0x3b4,
                "a52170b20c4967bed4b44be846935691c94378dd25ee1f6742e44b06fcde9002",
            ),
            (
                cleanup,
                0x70,
                "5580b415041d652ad9ded2915a479d3dce27f4e2ec2e3e2358d997f567921be1",
            ),
        ],
        GroundSummonKind::Efreet => [
            (
                init,
                0x268,
                "906a259b4bcb5c10cfc664520df34f382095f2baf477f4e4af016fd8268249ae",
            ),
            (
                callback,
                0x1d0,
                "4bd85786089b53c3bb9528b0b17057b70cce1c45160d113a1338f7ad3e949a12",
            ),
            (
                cleanup,
                0x70,
                "21c5ba52e11e081df2928665940dd683b68b02b5e7b11938b8de41caa82ae4dd",
            ),
        ],
        GroundSummonKind::Gnome => [
            (
                init,
                0x1dc,
                "ccd50d3354ba6da582c7b59d6f910af4098dd8f6e5d86ae2412a60412c660823",
            ),
            (
                callback,
                0x218,
                "0696cf71d990e51e38b6f61a7b7c1bbee15e3008a4f96c06b8e8c59731736ae5",
            ),
            (
                cleanup,
                0x70,
                "fe4e671f5f85a6a0827210b29d142c7f6b3d9c2a738eee333d544377f2518e17",
            ),
        ],
        GroundSummonKind::Undine => [
            (
                init,
                0x17c,
                "94ee0e672b5ab672f2c3def9f4f8f40a770a9b3b8c22bbb7f8f48fa2f8bf6b81",
            ),
            (
                callback,
                0x48c,
                "5f5d71d1d2b88f19e882b8f600cef2c1706059e7b0888e8f04b70dab3d47fc63",
            ),
            (
                cleanup,
                0x70,
                "d9a8c942321477e20ae93525c5e48d8c22c3e9b7cfb657b03ecde1ad9c049e56",
            ),
        ],
        GroundSummonKind::Origin => [
            (
                init,
                0x248,
                "929bc4ed32a97a3fdd4e4475bc776fa4c8adb8fbe9de1356b39c529ffe5d5bf3",
            ),
            (
                callback,
                0x1f0,
                "2a912333f93153552a7b1e6e0747ce45b33ea0fe2eeb815553ed7c898d48d403",
            ),
            (
                cleanup,
                0x70,
                "b2cfe06905b9331e508da84270115d0f4f0407bbbe1e8ab4c0c3ad83f785b054",
            ),
        ],
        GroundSummonKind::Volt => [
            (
                init,
                0x1ec,
                "25807977c5f920087df8af441d908a35bbe68d062ba388ef3a4d22b4e8f085fa",
            ),
            (
                callback,
                0x1a8,
                "beb2f05118ac73a264ccec8d16a2ce1b4770ce7c28f2b87d006eef0a7f765c7d",
            ),
            (
                cleanup,
                0x70,
                "e43d50b9467628583f2125309e625d2177ad3663139e5975fb1c6e04d42af897",
            ),
        ],
        GroundSummonKind::Shadow => [
            (
                init,
                0x248,
                "e9180debc4bd41bee70573c8ebb20fccae9f52dd4bd8b742c7c849d31f456eb8",
            ),
            (
                callback,
                0x1d0,
                "d4cb4914d450616ff8d6db1c8e3c4700c2ffbcfc5c4aeaa6c2124cb7cd571dca",
            ),
            (
                cleanup,
                0x70,
                "7fea4d8bf8ade88301d00520449696a2b4a5f7e31fa6e559ad8ace8631271187",
            ),
        ],
    };
    for (start, size, hash) in bodies.into_iter().chain([(
        0x37b10,
        0x194,
        "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
    )]) {
        ensure!(
            crate::digest(
                rel.at((1, start))?
                    .get(..size)
                    .context("truncated ground summon controller")?
            ) == hash,
            "unreviewed ground summon controller {start:#x}"
        );
    }
    if matches!(kind, GroundSummonKind::Celsius | GroundSummonKind::Sylph) {
        return staged_summon(rel, kind);
    }
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset| half(rel.at((1, offset))?, 2);
    let (center, notice, blessing) = match kind {
        GroundSummonKind::Celsius | GroundSummonKind::Sylph => unreachable!(),
        GroundSummonKind::Efreet => (
            0x9044,
            Some(b"ATTACK\0\0"),
            SummonBlessing::Attack(immediate(callback + 0x108)? as i16),
        ),
        GroundSummonKind::Gnome => (
            0x9250,
            Some(b"DEFENSE\0"),
            SummonBlessing::Defense(immediate(callback + 0x108)? as i16),
        ),
        GroundSummonKind::Origin => {
            ensure!(
                immediate(0x900ac)? == immediate(0x900cc)?
                    && immediate(0x902a8)? == 1
                    && immediate(0x9030c)? == 2,
                "unreviewed Origin blessing or ordered roots"
            );
            (
                0x9560,
                Some(b"STATUS\0\0"),
                SummonBlessing::AttackDefense(immediate(0x900ac)? as i16),
            )
        }
        GroundSummonKind::Undine => (0x9118, None, SummonBlessing::Heal(immediate(0x8e1ec)?)),
        GroundSummonKind::Volt => {
            ensure!(
                immediate(0x87ca4)? == 2
                    && immediate(0x87cb4)? == 1
                    && immediate(0x87e1c)? == 1
                    && immediate(0x87e94)? == 2,
                "unreviewed Volt retained blessing or ordered roots"
            );
            (
                0x8118,
                Some(b"STATUS\0\0"),
                SummonBlessing::PhysicalImmunity,
            )
        }
        GroundSummonKind::Shadow => {
            ensure!(
                immediate(0x8f728)? == 4
                    && immediate(0x8f738)? == 1
                    && immediate(0x8f908)? == 1
                    && immediate(0x8f96c)? == 2,
                "unreviewed Shadow retained blessing or ordered roots"
            );
            (0x9380, Some(b"STATUS\0\0"), SummonBlessing::MagicalImmunity)
        }
    };
    let blessing_start = callback
        + match kind {
            GroundSummonKind::Undine => 0xd4,
            GroundSummonKind::Volt => 0x44,
            _ => 0x6c,
        };
    ensure!(
        rel.local_targets().contains(&(1, callback))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0)
            && rel.at((4, color + 4))?[..12].iter().all(|&b| b == 0)
            && rel.at((4, center))?[..12].iter().all(|&b| b == 0)
            && immediate(blessing_start)? == GroundSummonRecipe::BLESSING_START
            && immediate(blessing_start + 8)? == GroundSummonRecipe::BLESSING_END,
        "unreviewed ground summon origins or blessing feedback"
    );
    if let Some(notice) = notice {
        ensure!(
            rel.at((4, center + 16))?[..8] == *notice
                && rel.at((4, center + 24))?[..4]
                    == *if matches!(kind, GroundSummonKind::Volt | GroundSummonKind::Shadow) {
                        b"SET!"
                    } else {
                        b"UP\0\0"
                    },
            "unreviewed summon notice"
        );
    }
    let waves = if kind == GroundSummonKind::Undine {
        // Actor headings are already radians; this is the same positive yaw as Lance.
        ensure!(
            scalar(0x912c)?.to_bits() == 1f32.to_radians().to_bits()
                && scalar(0x9128)? == 0.
                && immediate(0x8e1b8)? == 1
                && immediate(0x8e1bc)? == 20
                && immediate(0x8e558)? == 1,
            "unreviewed Undine rotation or recipient recovery binding"
        );
        let first_tick = immediate(0x8e27c)?;
        let end = immediate(0x8e284)?;
        let contact = immediate(0x8e3a4)?;
        ensure!(
            end > first_tick
                && (end - first_tick).is_multiple_of(4)
                && contact > first_tick
                && immediate(0x8e3ac)? - contact == end - first_tick,
            "unreviewed Undine wave range"
        );
        let mut offsets = [[0.; 3]; 4];
        for (i, offset) in offsets.iter_mut().enumerate() {
            for (axis, value) in offset.iter_mut().enumerate() {
                *value = scalar(0x90e8 + i * 12 + axis * 4)?;
            }
        }
        Some(SummonWaves {
            offsets,
            first_tick,
            interval: (end - first_tick) / 4,
            projectile_delay: contact - first_tick,
        })
    } else {
        None
    };
    let count = if kind == GroundSummonKind::Gnome {
        2
    } else {
        1
    };
    let mut pulses = Vec::new();
    for index in 0..waves.as_ref().map_or(count, |w| w.offsets.len()) {
        pulses.push(stored_parameters::Pulse {
            tick: if let Some(waves) = &waves {
                waves.first_tick + index as u16 * waves.interval + waves.projectile_delay
            } else {
                immediate(
                    callback
                        + match (kind, index) {
                            (GroundSummonKind::Origin, _) => 0x1a0,
                            (GroundSummonKind::Volt, _) => 0x158,
                            (_, 0) => 0x180,
                            _ => 0x1c4,
                        },
                )?
            },
            projectile: if kind == GroundSummonKind::Efreet {
                2
            } else if kind == GroundSummonKind::Gnome {
                index as u8 + 1
            } else {
                1
            },
            rule: if kind == GroundSummonKind::Gnome {
                index
            } else {
                0
            },
        });
    }
    let text = rel.at((4, title))?;
    Ok(Parameters {
        kind,
        lifetime: immediate(lifetime)?,
        origin: match kind {
            GroundSummonKind::Celsius | GroundSummonKind::Sylph => unreachable!(),
            GroundSummonKind::Efreet => GroundSummonOrigin::TargetDirection {
                height: scalar(0x9068)?,
                distance: scalar(0x906c)?,
            },
            GroundSummonKind::Origin => GroundSummonOrigin::TargetDirection {
                height: scalar(0x9584)?,
                distance: scalar(0x9588)?,
            },
            GroundSummonKind::Shadow => GroundSummonOrigin::TargetDirection {
                height: scalar(0x93a8)?,
                distance: scalar(0x93ac)?,
            },
            GroundSummonKind::Gnome | GroundSummonKind::Undine | GroundSummonKind::Volt => {
                GroundSummonOrigin::TargetGround(GroundSpellOrigin {
                    height: scalar(if kind == GroundSummonKind::Volt {
                        0x8140
                    } else {
                        0x1c80
                    })?,
                    nudge: 1.,
                    direction_threshold: scalar(0x2800)?,
                })
            }
        },
        pulses,
        waves,
        lanes: None,
        sound: None,
        extra_voices: Vec::new(),
        presentation: StoredSpellPresentation {
            color: rel.at((4, color))?[..4].try_into()?,
            camera_distance: scalar(camera)?,
            camera_elevation: scalar(camera + 4)?,
        },
        effect_scale: scalar(scale)?,
        title: std::str::from_utf8(crate::read::c_string(text, 0)?)?.to_owned(),
        focus_distance: scalar(focus)?,
        focus_ticks: immediate(focus_tick)?,
        voice: if kind == GroundSummonKind::Volt {
            None
        } else {
            Some(immediate(
                callback
                    + if kind == GroundSummonKind::Undine {
                        0xbc
                    } else {
                        0x54
                    },
            )?)
        },
        voice_tick: if kind == GroundSummonKind::Volt {
            0
        } else {
            immediate(
                callback
                    + if kind == GroundSummonKind::Undine {
                        0x80
                    } else {
                        0x44
                    },
            )?
        },
        blessing,
        blessing_radius: scalar(center + 12)?,
    })
}

/// These controllers add lane contacts or multiple voices to the shared arrival sequence.
fn staged_summon(rel: &Rel, kind: GroundSummonKind) -> Result<Parameters> {
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset| half(rel.at((1, offset))?, 2);
    let sylph = kind == GroundSummonKind::Sylph;
    let (callback, color, camera, scale, title, focus, lifetime, focus_tick, radius) = if sylph {
        (
            0x8e69c, 0x91a0, 0x91cc, 0x91dc, 0x91e0, 0x91e8, 0x8e9d8, 0x8eb58, 0x91bc,
        )
    } else {
        (
            0x8f090, 0x92d8, 0x9300, 0x92f0, 0x930c, 0x9318, 0x8f494, 0x8f550, 0x92dc,
        )
    };
    let blessing_start = if sylph { 0x8e760 } else { 0x8f0f4 };
    ensure!(
        rel.local_targets().contains(&(1, callback))
            && immediate(blessing_start)? == GroundSummonRecipe::BLESSING_START
            && immediate(blessing_start + 8)? == GroundSummonRecipe::BLESSING_END
            && scalar(if sylph { 0x91d4 } else { 0x9308 })? == 0.,
        "unreviewed staged summon origin or blessing window"
    );
    if sylph {
        ensure!(
            rel.at((4, 0x91b0))?[..12].iter().all(|&b| b == 0)
                && rel.at((4, 0x91c0))?[..12] == *b"SPEED\0\0\0UP\0\0"
                && immediate(0x8e7f8)? == 0x100
                && immediate(0x8e808)? == 1
                && immediate(0x8ea88)? == 2
                && immediate(0x8eaec)? == 1,
            "unreviewed Sylph blessing or ordered roots"
        );
    } else {
        ensure!(
            rel.at((4, 0x92e0))?[..8] == *b"ACC\0UP\0\0"
                && immediate(0x8f188)? == 0x4000
                && immediate(0x8f19c)? == 1
                && immediate(0x8f4d0)? == 1
                && immediate(0x8f2f4)? == 4
                && immediate(0x8f384)? == 4
                && immediate(0x8f410)? == 4,
            "unreviewed Celsius blessing or four-lane allocation"
        );
    }
    let schedule = if sylph {
        let first = immediate(0x8e8b8)?;
        let end = immediate(0x8e8c0)?;
        // The guarded callback uses (age - first) % 4 for five middle contacts.
        ensure!(
            end - first == 20,
            "unreviewed Sylph repeated contact window"
        );
        std::iter::once((immediate(0x8e874)?, 1))
            .chain((first..end).step_by(4).map(|tick| (tick, 2)))
            .chain([(immediate(0x8e920)?, 3)])
            .collect::<Vec<_>>()
    } else {
        vec![(immediate(0x8f304)?, 1), (immediate(0x8f390)?, 2)]
    };
    let pulses = schedule
        .into_iter()
        .map(|(tick, id)| stored_parameters::Pulse {
            tick,
            projectile: id,
            rule: usize::from(id - 1),
        })
        .collect();
    let lanes = if sylph {
        None
    } else {
        let first = scalar(0x92e8)?;
        let step = scalar(0x92ec)?;
        Some(SummonLanes {
            visual_tick: immediate(0x8f244)?,
            heading_offsets: std::array::from_fn(|index| {
                (first + index as f32 * step).to_radians()
            }),
        })
    };
    let sound = if sylph {
        None
    } else {
        let first_tick = immediate(0x8f204)?;
        let end = immediate(0x8f20c)?;
        ensure!(
            end - first_tick == 48 && immediate(0x8f238)? == 2,
            "unreviewed Celsius sound loop"
        );
        Some(SummonSound {
            id: immediate(0x8f234)?,
            first_tick,
            interval: 8,
            count: ((end - first_tick) / 8) as u8,
        })
    };
    let text = rel.at((4, title))?;
    Ok(Parameters {
        kind,
        lifetime: immediate(lifetime)?,
        origin: if sylph {
            GroundSummonOrigin::TargetDirection {
                height: scalar(0x91d4)?,
                distance: scalar(0x91d8)?,
            }
        } else {
            GroundSummonOrigin::World
        },
        pulses,
        waves: None,
        lanes,
        sound,
        extra_voices: if sylph {
            [(0x8e708, 0x8e724), (0x8e734, 0x8e750)]
                .into_iter()
                .map(|(tick, voice)| {
                    Ok(SummonVoice {
                        tick: immediate(tick)?,
                        voice: immediate(voice)?,
                    })
                })
                .collect::<Result<_>>()?
        } else {
            Vec::new()
        },
        presentation: StoredSpellPresentation {
            color: rel.at((4, color))?[..4].try_into()?,
            camera_distance: scalar(camera)?,
            camera_elevation: scalar(camera + 4)?,
        },
        effect_scale: scalar(scale)?,
        title: std::str::from_utf8(crate::read::c_string(text, 0)?)?.to_owned(),
        focus_distance: scalar(focus)?,
        focus_ticks: immediate(focus_tick)?,
        voice: Some(immediate(if sylph { 0x8e6f0 } else { 0x8f0dc })?),
        voice_tick: immediate(if sylph { 0x8e6e0 } else { 0x8f0c8 })?,
        blessing: if sylph {
            SummonBlessing::Speed
        } else {
            SummonBlessing::Accuracy(immediate(0x8f190)? as i16)
        },
        blessing_radius: scalar(radius)?,
    })
}
