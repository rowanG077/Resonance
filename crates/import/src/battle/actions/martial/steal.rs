//! Item Thief/Rover share tracks; success resumes beyond their first terminators.
use super::{
    contact::{call, instructions},
    *,
};
use crate::battle::action_program::HIT_BYTES;
use resonance_content::battle::projectile_modifiers::{Axis, ProjectileOverride, ProjectileVector};
use serde::{Deserialize, Serialize};

const INITIALIZER: usize = 0x6005c;
const UPDATE: usize = 0x5fd60;

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    projectile: EffectId,
    overrides: [ProjectileOverride; 4],
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(variant == 0, "unexpected steal phase");
    Ok(MartialCallback::Steal {
        recipe: StealRecipe {
            success_hit: source.hits(0)?.len().try_into()?,
            projectile: p.projectile,
            rule: source.phase_rule(0, 2)?,
            overrides: p.overrides,
        },
    })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    for native in [43, 44] {
        let dispatch = rel.pointer(DATA, 0xd60 + native * 4)?;
        ensure!(
            rel.pointer(dispatch.0, dispatch.1)? == (1, INITIALIZER)
                && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4)
                && rel.local_targets().contains(&(1, UPDATE)),
            "changed steal callback entry {native}"
        );
    }
    let entry = rel.at((1, INITIALIZER))?;
    instructions(
        entry,
        &[
            (0x20, 0x388000e0),
            (0x30, 0x388000df),
            (0x40, 0x2c000190),
            (0x48, 0x38600002),
            (0x4c, 0x38800110),
            (0x64, 0x901e0018),
        ],
    )?;
    for (at, target) in [
        (0x18, 0x3823c),
        (0x24, 0x218e4),
        (0x34, 0x218e4),
        (0x58, 0xa8a8),
    ] {
        call(entry, INITIALIZER, at, target)?;
    }
    let code = rel.at((1, UPDATE))?;
    instructions(
        code,
        &[
            (0x24, 0xa80301be),
            (0x28, 0x2c000028),
            (0xa8, 0x2c0000c8),
            (0xb4, 0x2c00000a),
            (0xd4, 0x1c000003),
            (0xdc, 0x3bc30001),
            (0xf4, 0x38600001),
            (0xf8, 0x38e00007),
            (0x114, 0x8108000c),
            (0x11c, 0x39080038),
            (0x130, 0x3800000e),
            (0x138, 0x9803005d),
            (0x148, 0xd0230060),
            (0x158, 0xd0230064),
            (0x164, 0xd0030068),
            (0x16c, 0xd023002c),
            (0x170, 0xd003006c),
            (0x174, 0xd0030070),
            (0x178, 0xd0030074),
            (0x18c, 0x2c00003c),
            (0x210, 0x38800060),
            (0x224, 0x2c0000e0),
            (0x234, 0x38a00001),
            (0x248, 0x38a00000),
            (0x26c, 0x38000082),
            (0x290, 0x38080024),
            (0x29c, 0x38080020),
        ],
    )?;
    for (at, target) in [
        (0x120, 0x205ac),
        (0x214, 0x1c74c),
        (0x238, 0xb5e8),
        (0x24c, 0xb5e8),
        (0x268, 0x1f794),
    ] {
        call(code, UPDATE, at, target)?;
    }
    let constants = rel.at((4, 0x3540))?;
    Ok(Parameters {
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: half(code, 0xfa)?.try_into()?,
        },
        overrides: [
            ProjectileOverride::BirthEffect {
                id: half(code, 0x132)?.try_into()?,
            },
            ProjectileOverride::Vector {
                field: ProjectileVector::SpawnOffset,
                value: [
                    float(constants, 0)?,
                    float(constants, 0)?,
                    float(constants, 4)?,
                ],
            },
            ProjectileOverride::Component {
                field: ProjectileVector::Velocity,
                axis: Axis::Z,
                value: float(constants, 8)?,
            },
            ProjectileOverride::Vector {
                field: ProjectileVector::VelocityJitter,
                value: [float(constants, 12)?; 3],
            },
        ],
    })
}

pub(crate) fn continuations(source: &bundle::Bundle, phases: &mut [TechniquePhase]) -> Result<()> {
    let [phase] = phases else {
        bail!("steal requires one authored phase")
    };
    let Some(MartialCallback::Steal { recipe }) = phase.callback else {
        unreachable!()
    };
    let variant = usize::from(phase.variant);
    let cursor = phase.action.animations.instructions.get(&1);
    ensure!(
        matches!(cursor, Some(AnimationInstruction::Step(AnimationStep {
        trigger: AnimationTrigger::Tick(time), command: AnimationCommand::Play { .. }
    })) if *time > 60),
        "steal callback can reach an unexpected animation cursor"
    );
    phase.action.animations = source.animations_with_continuations(variant, &[(3, 1)])?;
    let hit_start = source
        .phase(variant)?
        .hit_root
        .context("missing steal hit root")?;
    let continuation = hit_start + (usize::from(recipe.success_hit) + 1) * HIT_BYTES;
    ensure!(
        matches!(
            source.hit_record(continuation - HIT_BYTES)?,
            HitRecord::End { .. }
        ),
        "steal hit continuation does not follow a terminator"
    );
    phase
        .action
        .hits
        .extend(source.hits_at(variant, continuation)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn original_steal_tracks_retain_failure_and_success_continuations() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let extracted = root.join(disc);
            let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
            let p = read_parameters(&rel)?;
            let p: Parameters = serde_json::from_value(serde_json::to_value(p)?)?;
            assert_eq!(p.projectile.id, 7);
            assert!(matches!(
                p.overrides[0],
                ProjectileOverride::BirthEffect { id: 14 }
            ));
            let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
            let actions = technique_actions(&extracted, &rel, &usual, &[223, 224])?;
            rel.pointers.remove(&(DATA, 0xd60 + 44 * 4));
            assert!(read_parameters(&rel).is_err());
            let catalogue = BattleActions {
                party: vec![],
                enemies: vec![],
                projectiles: vec![],
                techniques: actions.clone(),
                chains: None,
            };
            catalogue.validate()?;
            let dependencies = crate::battle::selection::Dependencies::actions(&catalogue)?;
            assert!(dependencies.projectiles.contains(&EffectId {
                bank: EffectBank::Techniques,
                id: 7
            }));
            assert!(dependencies.programs.contains(&EffectId {
                bank: EffectBank::Techniques,
                id: 14
            }));
            let (_, voices) = catalogue.audio_ids();
            assert!(voices.contains(&0x80b3) && voices.contains(&0x80b4));
            for technique in actions {
                assert!(matches!(technique.native_id, 43 | 44));
                let TechniqueProgram::Martial { variants } = technique.program else {
                    panic!()
                };
                let [phase] = variants.as_slice() else {
                    panic!()
                };
                assert_eq!(
                    phase
                        .action
                        .hits
                        .iter()
                        .map(|hit| hit.start)
                        .collect::<Vec<_>>(),
                    [32, 95]
                );
                assert!(matches!(
                    phase.callback,
                    Some(MartialCallback::Steal {
                        recipe: StealRecipe { success_hit: 1, .. }
                    })
                ));
                for (row, time, clip) in [(1, 70, 54), (2, 170, 0), (4, 65, 55), (5, 145, 0)] {
                    assert!(matches!(&phase.action.animations.instructions[&row],
                        AnimationInstruction::Step(AnimationStep { trigger: AnimationTrigger::Tick(t),
                        command: AnimationCommand::Play { clip: c, .. } }) if *t == time && *c == clip));
                }
                for row in [3, 6] {
                    assert!(matches!(
                        phase.action.animations.instructions[&row],
                        AnimationInstruction::End
                    ));
                }
            }
        }
        Ok(())
    }
}
