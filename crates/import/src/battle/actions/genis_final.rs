//! Recover independent contacts and authored positions for Genis's advanced spells.
use super::*;
use resonance_content::battle::actions::{
    absolute::AbsoluteRecipe, earth_bite::EarthBiteRecipe, earth_field::EarthFieldPulse,
    lightning::GroundSpellOrigin, meteor_storm::MeteorStormRecipe, ray::RayBurst,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub native: u16,
    lifetime: u16,
    origin: GroundSpellOrigin,
    presentation: StoredSpellPresentation,
    effect_scale: f32,
    pattern: Pattern,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Pattern {
    Absolute {
        radius: f32,
        select_tick: u16,
        second_tick: u16,
    },
    EarthBite {
        second_height: f32,
        ticks: [u16; 3],
    },
    MeteorStorm {
        bursts: Box<[RayBurst; 14]>,
        heading_offset: f32,
    },
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (initializer, init_size, init_hash, callback, size, hash) = match native {
        230 => (
            0x7cbb4,
            0xf0,
            "ac80453ed113fbf2aaf4683062f27159394d4e8d04e4cde1d9b7ef9bbe444f40",
            0x7c98c,
            0x228,
            "afdb6af38fea7cf74e5a690aa91a806ee064627aa2ad4c92cc6dfbbad656e3d7",
        ),
        231 => (
            0x83ff8,
            0xf8,
            "4f14045f1df612e9e5c2d462d2a57bf05a6ed86614675629881e9c04137a6225",
            0x83ed8,
            0x120,
            "0ea415cd2225bcfc0d42a52962c366cafa9b3fc9b8cecfdeef6937de98d46bf2",
        ),
        233 => (
            0x92dec,
            0x114,
            "864409d334836f9d007b055aa3abc2b40e57f16043c11f013874a1a857b57844",
            0x92c10,
            0x1dc,
            "7b12ea41b12c0e08b47dcd1f87610c9b7eaa3bdb74924d1ed6249dd9825b85ce",
        ),
        _ => bail!("unsupported advanced Genis native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [initializer, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected advanced Genis dispatch phase {phase}"
        );
    }
    for (start, size, hash) in [
        (initializer, init_size, init_hash),
        (callback, size, hash),
        (
            0x37b10,
            0x194,
            "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, start))?
                    .get(..size)
                    .context("truncated Genis controller")?
            ) == hash,
            "unreviewed Genis controller {start:#x}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, callback))
            && rel.at((4, 0x1c4c))?[..12].iter().all(|&b| b == 0),
        "unreviewed Genis callback or origin fallback"
    );
    let scalar = |offset| float(rel.at((4, offset))?, 0);
    let immediate = |offset| half(rel.at((1, offset))?, 2);
    let lifetime = immediate(initializer + 0x50)?;
    let origin = GroundSpellOrigin {
        height: scalar(if native == 231 { 0x76a8 } else { 0x1c80 })?,
        nudge: 1.,
        direction_threshold: scalar(0x2800)?,
    };
    let (color, camera, scale) = match native {
        230 => (0x5c20, 0x5c2c, 0x5c28),
        231 => (0x7698, 0x76a0, 0x76ac),
        _ => (0x9b98, 0x9c4c, 0x9c48),
    };
    let presentation = StoredSpellPresentation {
        color: rel.at((4, color))?[..4].try_into()?,
        camera_distance: scalar(camera)?,
        camera_elevation: scalar(camera + 4)?,
    };
    let effect_scale = scalar(scale)?;
    let pattern = match native {
        230 => Pattern::Absolute {
            radius: scalar(0x5c24)?,
            select_tick: immediate(0x7c9b4)?,
            second_tick: immediate(0x7cae8)?,
        },
        231 => Pattern::EarthBite {
            second_height: scalar(0x769c)?,
            ticks: [
                immediate(0x83ef0)?,
                immediate(0x83f34)?,
                immediate(0x83f90)?,
            ],
        },
        _ => {
            ensure!(
                scalar(0x9c54)? == 0. && scalar(0x9c44)?.to_bits() == 1f32.to_radians().to_bits(),
                "unexpected Meteor Storm world origin or heading conversion"
            );
            let start = immediate(0x92c70)?;
            let end = immediate(0x92c78)?;
            let mut bursts = [RayBurst {
                tick: 0,
                offset: [0.; 3],
            }; 14];
            for (i, burst) in bursts.iter_mut().enumerate() {
                let at = 0x9b9c + i * 12;
                *burst = RayBurst {
                    tick: start + i as u16 * (end - start) / 14,
                    offset: [scalar(at)?, scalar(at + 4)?, scalar(at + 8)?],
                };
            }
            Pattern::MeteorStorm {
                bursts: Box::new(bursts),
                heading_offset: scalar(0x9c58)? * scalar(0x9c44)?,
            }
        }
    };
    Ok(Parameters {
        native,
        lifetime,
        origin,
        presentation,
        effect_scale,
        pattern,
    })
}

pub(super) fn cook(
    catalogue: &crate::arte::Catalogue,
    tables: &Tables,
    technique: u16,
    definition: &crate::arte::Definition,
) -> Result<TechniqueProgram> {
    let native = definition.native_id as u16;
    ensure!(
        technique == native - 138 && definition.flags == 0x00440193,
        "unexpected advanced Genis menu binding"
    );
    let p = tables
        .elemental
        .genis_final
        .iter()
        .find(|p| p.native == native)
        .context("missing advanced Genis parameters")?;
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == if native == 230 { 240 } else { 180 }
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == if native == 233 { 1 } else { 2 },
        "unexpected Genis action phases or rule table"
    );
    let casters = recovery::casters(
        catalogue,
        tables,
        technique,
        definition,
        recovery::Release::Stored,
    )?;
    let resume = stored_resume::shared(tables, &casters)?;
    Ok(match &p.pattern {
        Pattern::Absolute {
            radius,
            select_tick,
            second_tick,
        } => {
            let recipe = AbsoluteRecipe {
                lifetime: p.lifetime,
                origin: p.origin,
                presentation: p.presentation,
                effect_scale: p.effect_scale,
                radius: *radius,
                select_tick: *select_tick,
                second_tick: *second_tick,
                rules: [bundle.rule(0)?, bundle.rule(1)?],
            };
            recipe.validate()?;
            TechniqueProgram::Absolute {
                casters,
                resume,
                recipe,
            }
        }
        Pattern::EarthBite {
            second_height,
            ticks,
        } => {
            let first = bundle.rule(0)?;
            let following = bundle.rule(1)?;
            let recipe = EarthBiteRecipe {
                lifetime: p.lifetime,
                origin: p.origin,
                presentation: p.presentation,
                effect_scale: p.effect_scale,
                second_height: *second_height,
                pulses: std::array::from_fn(|i| EarthFieldPulse {
                    tick: ticks[i],
                    projectile: EarthBiteRecipe::effect(if i == 0 { 1 } else { 2 }),
                    rule: if i == 0 { first } else { following },
                }),
            };
            recipe.validate()?;
            TechniqueProgram::EarthBite {
                casters,
                resume,
                recipe,
            }
        }
        Pattern::MeteorStorm {
            bursts,
            heading_offset,
        } => {
            let recipe = MeteorStormRecipe {
                lifetime: p.lifetime,
                presentation: p.presentation,
                effect_scale: p.effect_scale,
                bursts: **bursts,
                heading_offset: *heading_offset,
                rule: bundle.rule(0)?,
            };
            recipe.validate()?;
            TechniqueProgram::MeteorStorm {
                casters,
                resume,
                recipe,
            }
        }
    })
}
