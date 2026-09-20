//! Shared-runner spell callbacks are checked once when their parameters are cooked.
use super::*;
use resonance_content::battle::{
    actions::lightning::GroundSpellOrigin,
    effects::{EffectBank, EffectId},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub fire_ball: FireBall,
    pub wind_blade: WindBlade,
    pub stone_blast: GroundContact,
    pub icicle: GroundContact,
}

#[derive(Serialize, Deserialize)]
pub(super) struct FireBall {
    pub height_scale: f32,
    pub height_offset: f32,
    pub emissions: [SpellProjectile; 3],
}

#[derive(Serialize, Deserialize)]
pub(super) struct WindBlade {
    pub effect_tick: u16,
    pub effect: EffectId,
    pub contact_size: f32,
}

#[derive(Serialize, Deserialize)]
pub(super) struct GroundContact {
    pub origin: GroundSpellOrigin,
    pub radius: f32,
    pub height: f32,
    pub effect_scale: f32,
}

/// Callback order is Aqua Edge, Fire Ball, Wind Blade, Stone Blast, Icicle.
#[derive(Clone, Copy, Serialize)]
struct Layout {
    dispatch: usize,
    initializers: [usize; 5],
    callbacks: [usize; 5],
    runner: usize,
    fire_ball: [usize; 3], // velocity table, height offset, height scale
    wind_size: usize,
    ground: [usize; 2],      // ground height, normalization threshold
    stone_blast: [usize; 3], // radius, height, effect scale
    icicle: [usize; 3],
    aqua_edge: usize,
    debug: bool,
}

impl Layout {
    const RETAIL: Self = Self {
        dispatch: 0x1238,
        initializers: [0x7587c, 0x60cc4, 0x64660, 0x60f78, 0x721e0],
        callbacks: [0x75608, 0x60b3c, 0x64528, 0x60ecc, 0x72100],
        runner: 0x37e48,
        fire_ball: [0x3928, 0x394c, 0x3950],
        wind_size: 0x3c68,
        ground: [0x1c80, 0x2800],
        stone_blast: [0x39f8, 0x39fc, 0x3a00],
        icicle: [0x46d0, 0x46d4, 0x46d8],
        aqua_edge: 0x5200,
        debug: false,
    };

    fn module(name: &str) -> Option<Self> {
        Some(match name {
            "US_r_Top2Btl.rel" => Self::RETAIL,
            "r_Top2Btl.rel" => Self {
                initializers: [0x75b68, 0x60fb8, 0x64954, 0x6126c, 0x724cc],
                callbacks: [0x758f4, 0x60e30, 0x6481c, 0x611c0, 0x723ec],
                runner: 0x37dd8,
                fire_ball: [0x38d8, 0x38fc, 0x3900],
                wind_size: 0x3c10,
                ground: [0x1c58, 0x27f8],
                stone_blast: [0x39a8, 0x39ac, 0x39b0],
                icicle: [0x4678, 0x467c, 0x4680],
                aqua_edge: 0x51c0,
                ..Self::RETAIL
            },
            "US_Top2Btl.rel" | "US_m_Top2Btl.rel" => Self {
                dispatch: 0x16d0,
                initializers: [0x80298, 0x6b3d4, 0x6ee38, 0x6b688, 0x7cb7c],
                callbacks: [0x80024, 0x6b24c, 0x6ed00, 0x6b5dc, 0x7ca9c],
                runner: 0x4038c,
                fire_ball: [0x8170, 0x8194, 0x8198],
                wind_size: 0x8508,
                ground: [0x4188, 0x6370],
                stone_blast: [0x8240, 0x8244, 0x8248],
                icicle: [0x8fd0, 0x8fd4, 0x8fd8],
                aqua_edge: 0x9b38,
                debug: false,
            },
            "Top2Btl.rel" | "m_Top2Btl.rel" => Self {
                dispatch: 0x16d0,
                initializers: [0x804d4, 0x6b618, 0x6f07c, 0x6b8cc, 0x7cdb8],
                callbacks: [0x80260, 0x6b490, 0x6ef44, 0x6b820, 0x7ccd8],
                runner: 0x40304,
                fire_ball: [0x8130, 0x8154, 0x8158],
                wind_size: 0x84c0,
                ground: [0x4170, 0x6378],
                stone_blast: [0x8200, 0x8204, 0x8208],
                icicle: [0x8f88, 0x8f8c, 0x8f90],
                aqua_edge: 0x9b08,
                debug: false,
            },
            "Top2BtlD.rel" => Self {
                dispatch: 0x3ad0,
                initializers: [0x81b88, 0x6aef4, 0x6f2cc, 0x6b33c, 0x7db70],
                callbacks: [0x81bfc, 0x6aff8, 0x6f32c, 0x6b3fc, 0x7dc30],
                runner: 0x4e8b8,
                fire_ball: [0x3208, 0x3200, 0x3204],
                wind_size: 0x34d8,
                ground: [0x20fc, 0xc3c],
                stone_blast: [0x32d4, 0x32d8, 0x32d0],
                icicle: [0x3d74, 0x3d78, 0x3d70],
                aqua_edge: 0x4820,
                debug: true,
            },
            _ => return None,
        })
    }

    fn validate(&self, rel: &Rel) -> Result<()> {
        for (index, native) in [200, 204, 208, 212, 220].into_iter().enumerate() {
            let dispatch = rel.pointer(DATA, self.dispatch + (native - 200) * 4)?;
            ensure!(
                rel.pointer(dispatch.0, dispatch.1)? == (1, self.initializers[index])
                    && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, self.runner)
                    && rel.local_targets().contains(&(1, self.callbacks[index])),
                "unexpected ordinary spell {native} dispatch"
            );
        }
        let check = |root, release: &[(usize, u32)], debug: &[(usize, u32)]| -> Result<()> {
            for &(offset, expected) in if self.debug { debug } else { release } {
                ensure!(
                    word(rel.at((1, root + offset))?, 0)? == expected,
                    "unexpected ordinary spell operation at {:#x}",
                    root + offset
                );
            }
            Ok(())
        };
        check(
            self.callbacks[0],
            &[
                (0x118, 0x2c1b0003),
                (0x130, 0x2c030078),
                (0x178, 0x2c03001e),
            ],
            &[
                (0x138, 0x2c1e0003),
                (0x160, 0x2c000078),
                (0x1b0, 0x2c00001e),
            ],
        )?;
        check(
            self.callbacks[1],
            &[
                (0x38, 0x2c1c0002),
                (0x88, 0x2c1c0014),
                (0xb0, 0x38800051),
                (0xf8, 0x38e00003),
            ],
            &[
                (0x7c, 0x2c000002),
                (0x88, 0x2c000014),
                (0xb8, 0x38800051),
                (0x154, 0x38e00003),
            ],
        )?;
        check(
            self.callbacks[2],
            &[
                (0x28, 0x2c000005),
                (0x60, 0x38a0001b),
                (0x94, 0x2c04000a),
                (0x9c, 0x2c040023),
            ],
            &[
                (0x28, 0x2c000005),
                (0x6c, 0x38a0001b),
                (0x98, 0x2c00000a),
                (0xa4, 0x2c000023),
            ],
        )?;
        check(
            self.callbacks[3],
            &[(0x14, 0x2c07001e), (0x20, 0x2c07002d), (0x44, 0x1c000005)],
            &[(0x28, 0x2c00001e), (0x34, 0x2c00002d), (0x5c, 0x1c000005)],
        )?;
        check(
            self.callbacks[4],
            &[(0x14, 0x2c000002), (0x74, 0x2c00001a), (0xa0, 0x38c6001c)],
            &[(0x2c, 0x2c000002), (0x9c, 0x2c00001a), (0xd0, 0x38dd001c)],
        )?;
        aqua_edge::validate_parameters(rel, self.aqua_edge, self.ground[1])?;
        if self.initializers == Self::RETAIL.initializers {
            wind_blade::validate_controller(rel)?;
            stone_blast::validate_controller(rel)?;
            icicle::validate_controller(rel)?;
        }
        Ok(())
    }
}

impl Parameters {
    #[cfg(test)]
    pub fn read(rel: &Rel) -> Result<Self> {
        Self::read_at(rel, Layout::RETAIL)
    }

    fn read_at(rel: &Rel, layout: Layout) -> Result<Self> {
        layout.validate(rel)?;
        let scalar = |offset| float(rel.at((4, offset))?, 0);
        let spread = rel.at((4, layout.fire_ball[0]))?;
        let mut emissions = [SpellProjectile {
            tick: 0,
            effect: 3,
            sound: 81,
            velocity: [0.; 3],
        }; 3];
        for (i, emission) in emissions.iter_mut().enumerate() {
            emission.tick = 2 + i as u16 * 8;
            emission.velocity = [
                float(spread, i * 12)?,
                float(spread, i * 12 + 4)?,
                float(spread, i * 12 + 8)?,
            ];
        }
        let ground = |offsets: [usize; 3]| -> Result<GroundContact> {
            Ok(GroundContact {
                origin: GroundSpellOrigin {
                    height: scalar(layout.ground[0])?,
                    nudge: 1.,
                    direction_threshold: scalar(layout.ground[1])?,
                },
                radius: scalar(offsets[0])?,
                height: scalar(offsets[1])?,
                effect_scale: scalar(offsets[2])?,
            })
        };
        Ok(Self {
            fire_ball: FireBall {
                height_scale: scalar(layout.fire_ball[2])?,
                height_offset: scalar(layout.fire_ball[1])?,
                emissions,
            },
            wind_blade: WindBlade {
                effect_tick: 5,
                effect: EffectId {
                    bank: EffectBank::Techniques,
                    id: 27,
                },
                contact_size: scalar(layout.wind_size)?,
            },
            stone_blast: ground(layout.stone_blast)?,
            icicle: ground(layout.icicle)?,
        })
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(layout) = file
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(Layout::module)
    else {
        return Ok(None);
    };
    crate::battle::embedded::write(
        file,
        output,
        "battle-ordinary-spell-parameters",
        &Parameters::read_at(&Rel::read(file)?, layout)?,
        serde_json::json!({"dispatch":{"section":5,"offset":layout.dispatch},"parameters_section":4,"layout":layout}),
    )
    .map(Some)
}

#[test]
#[ignore = "requires original battle modules from both discs; no media conversion"]
fn original_elemental_spell_parameters_publish_checked_controllers() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = crate::temporary_path(&std::env::temp_dir().join("ordinary-spell-parameters"));
    for disc in ["disc1", "disc2"] {
        let file = local.join(disc).join("files/US_r_Top2Btl.rel");
        let mut rel = Rel::read(&file)?;
        let original = Parameters::read(&rel)?;
        for module in [
            "US_r_Top2Btl.rel",
            "r_Top2Btl.rel",
            "US_Top2Btl.rel",
            "US_m_Top2Btl.rel",
            "Top2Btl.rel",
            "m_Top2Btl.rel",
            "Top2BtlD.rel",
        ] {
            let layout = Layout::module(module).unwrap();
            let path = local.join(disc).join("files").join(module);
            let mut source = Rel::read(&path)?;
            let parameters = Parameters::read_at(&source, layout)?;
            let paths = cook(&path, &output)?.context("missing ordinary spell publication")?;
            let cooked: Parameters =
                crate::embedded::read(&output, "battle-ordinary-spell-parameters", module)?;
            assert_eq!(
                serde_json::to_vec(&cooked)?,
                serde_json::to_vec(&parameters)?
            );
            assert_eq!(
                serde_json::to_vec(&cooked)?,
                serde_json::to_vec(&original)?,
                "{disc}/{module}"
            );
            let receipt: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join(&paths[1]))?)?;
            assert_eq!(receipt["layout"], serde_json::to_value(layout)?);
            let dispatch = source.pointer(DATA, layout.dispatch + 4 * 4)?;
            source.pointers.insert(dispatch, (1, 0));
            assert!(
                Parameters::read_at(&source, layout).is_err(),
                "{module} wrong initializer"
            );
            source
                .pointers
                .insert(dispatch, (1, layout.initializers[1]));
            let at =
                source.sections[1].0 + layout.callbacks[1] + if layout.debug { 0x7c } else { 0x38 };
            source.bytes[at + 3] ^= 1;
            assert!(
                Parameters::read_at(&source, layout).is_err(),
                "{module} changed emission schedule"
            );
        }
        let cooked: Parameters = crate::embedded::read(
            &output,
            "battle-ordinary-spell-parameters",
            "US_r_Top2Btl.rel",
        )?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&original)?);
        assert_eq!(
            cooked.fire_ball.emissions.map(|shot| shot.tick),
            [2, 10, 18]
        );
        assert_eq!(
            (cooked.wind_blade.effect_tick, cooked.wind_blade.effect.id),
            (5, 27)
        );
        assert_eq!(
            (cooked.stone_blast.radius, cooked.stone_blast.height),
            (85., 250.)
        );
        assert_eq!((cooked.icicle.radius, cooked.icicle.height), (90., 180.));
        let stored = stored_parameters::Parameters::read(&rel)?;
        stored_parameters::cook(&file, &output)?.context("missing stored spell publication")?;
        let cooked: stored_parameters::Parameters = crate::embedded::read(
            &output,
            "battle-stored-spell-parameters",
            "US_r_Top2Btl.rel",
        )?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&stored)?);
        assert_eq!(
            [
                cooked.eruption.lifetime,
                cooked.explosion.lifetime,
                cooked.flame_lance.lifetime,
                cooked.stalagmite.lifetime,
                cooked.ground_dasher.lifetime,
                cooked.grave.lifetime
            ],
            [210, 190, 190, 245, 190, 215]
        );
        let elemental = elemental_parameters::Parameters::read(&rel)?;
        elemental_parameters::cook(&file, &output)?
            .context("missing elemental spell publication")?;
        let cooked: elemental_parameters::Parameters = crate::embedded::read(
            &output,
            "battle-elemental-spell-parameters",
            "US_r_Top2Btl.rel",
        )?;
        assert_eq!(
            serde_json::to_vec(&cooked)?,
            serde_json::to_vec(&elemental)?
        );
        assert_eq!(
            (cooked.spread.lifetime, cooked.spread.single_pulse()?.tick),
            (170, 45)
        );
        assert_eq!(
            (
                cooked.air_thrust.lifetime,
                cooked.air_thrust.projectile_tick
            ),
            (150, 30)
        );
        assert_eq!(
            (
                cooked.ice_tornado.lifetime,
                cooked.ice_tornado.single_pulse()?.tick
            ),
            (200, 45)
        );
        assert_eq!(
            (cooked.acid_rain.lifetime, cooked.acid_rain.application_tick),
            (240, 120)
        );
        let recovery = recovery_parameters::Parameters::read(&rel)?;
        recovery_parameters::cook(&file, &output)?.context("missing recovery publication")?;
        let cooked: recovery_parameters::Parameters =
            crate::embedded::read(&output, "battle-recovery-parameters", "US_r_Top2Btl.rel")?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&recovery)?);
        assert_eq!((cooked.first_aid.tick, cooked.first_aid.percent), (10, 30));
        assert_eq!((cooked.heal.lifetime, cooked.heal.percent), (165, 60));
        assert_eq!((cooked.cure.lifetime, cooked.cure.percent), (165, 100));
        assert_eq!(cooked.nurse.lifetime, 250);
        assert!(cooked.stored_release_rate.is_finite() && cooked.stored_release_rate > 0.);
        let mut actor = crate::battle::all::ActorSettings::read(
            rel.at((DATA, crate::battle::embedded::Layout::RETAIL.party_settings))?,
        )?;
        actor.casting.stored_recovery_clip = 12;
        assert!(matches!(cooked.recovery_pose.animation(&actor)?,
            Some(AnimationCommand::Play { clip: 12, rate, .. }) if rate.is_finite() && rate > 0.));
        let summons = summon_parameters::Parameters::read(&rel)?;
        summon_parameters::cook(&file, &output)?.context("missing summon publication")?;
        let cooked: summon_parameters::Parameters =
            crate::embedded::read(&output, "battle-summon-parameters", "US_r_Top2Btl.rel")?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&summons)?);
        assert_eq!(
            cooked
                .fixed
                .each_ref()
                .map(|p| (p.kind.native(), p.offsets.len())),
            [(290, 17), (292, 14)]
        );
        assert_eq!(
            cooked.fixed.each_ref().map(|p| p.title.as_str()),
            ["-Luna-", "-Maxwell-"]
        );
        let entry = rel.pointer(DATA, 0x1238 + 4 * 4)?;
        let martial = martial::Parameters::read(&rel)?;
        martial_parameters::cook(&file, &output)?.context("missing martial publication")?;
        let cooked: martial::Parameters =
            crate::embedded::read(&output, "battle-martial-parameters", "US_r_Top2Btl.rel")?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&martial)?);
        rel.pointers.insert(entry, (1, 0));
        assert!(
            Parameters::read(&rel).is_err(),
            "wrong callback must fail during cooking"
        );
    }
    std::fs::remove_dir_all(output)?;
    Ok(())
}
