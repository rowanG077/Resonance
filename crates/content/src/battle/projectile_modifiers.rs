//! Per-emission projectile overrides, expressed as fields rather than memory writes.
use super::effects::{EffectBank, EffectId, ProjectileMovement, ProjectileRecipe};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModifierId {
    pub monster: u8,
    pub offset: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileModifiers {
    pub programs: Vec<ModifierProgram>,
    pub uses: Vec<ModifierUse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierUse {
    pub monster: u8,
    pub action: u8,
    pub hit: u16,
    pub projectile: EffectId,
    pub modifier: ModifierId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierProgram {
    pub id: ModifierId,
    pub operations: Vec<ProjectileOverride>,
    /// A partially recovered program cannot be applied at runtime.
    pub issues: Vec<ModifierIssue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectileOverride {
    ActiveWindow {
        start: u16,
        duration: u16,
    },
    Vector {
        field: ProjectileVector,
        value: [f32; 3],
    },
    Component {
        field: ProjectileVector,
        axis: Axis,
        value: f32,
    },
    Reaction {
        value: u8,
    },
    BirthEffect {
        id: u8,
    },
    HitClassification {
        damage_kind: u8,
        hit_class: u8,
    },
    AddComponent {
        field: ProjectileVector,
        axis: Axis,
        value: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileVector {
    Velocity,
    Acceleration,
    SpawnOffset,
    VelocityJitter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierIssue {
    pub source_offset: u32,
    pub problem: ModifierProblem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModifierProblem {
    Source {
        reason: String,
    },
    UnsupportedDestination {
        store: ProjectileStore,
        destination: u16,
    },
}

/// The value written by a modifier; byte-store instruction aliases are equivalent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileStore {
    Vector,
    Byte,
    Halfword,
    Float,
}

impl ProjectileModifiers {
    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        for program in &self.programs {
            ensure!(
                program.id.offset != 0 && ids.insert(program.id),
                "invalid projectile modifier identity"
            );
            for operation in &program.operations {
                operation.validate()?;
            }
        }
        ensure!(
            self.uses
                .iter()
                .all(|usage| usage.monster == usage.modifier.monster
                    && ids.contains(&usage.modifier)),
            "missing projectile modifier program"
        );
        Ok(())
    }
}

impl ModifierProgram {
    /// Apply before spawn rotation, velocity jitter, homing, and birth effects.
    pub fn apply(&self, recipe: &mut ProjectileRecipe) -> Result<()> {
        ensure!(
            self.issues.is_empty(),
            "projectile modifier has unresolved source instructions"
        );
        ensure!(
            recipe
                .id
                .is_some_and(|id| id.bank == EffectBank::Enemy(self.id.monster)),
            "projectile modifier owner mismatch"
        );
        apply_overrides(&self.operations, recipe)
    }
}

impl ProjectileOverride {
    pub fn validate(self) -> Result<()> {
        ensure!(
            match self {
                Self::ActiveWindow { start, duration } => start.checked_add(duration).is_some(),
                Self::Vector { value, .. } => value.iter().all(|v| v.is_finite()),
                Self::Component { value, .. } | Self::AddComponent { value, .. } =>
                    value.is_finite(),
                _ => true,
            },
            "invalid projectile modifier"
        );
        Ok(())
    }
}

/// Apply atomically before initialization, including native callback customizations.
pub fn apply_overrides(
    operations: &[ProjectileOverride],
    recipe: &mut ProjectileRecipe,
) -> Result<()> {
    let mut result = recipe.clone();
    for operation in operations {
        operation.validate()?;
        match *operation {
            ProjectileOverride::ActiveWindow { start, duration } => {
                result.active = (duration != 0).then_some([start, start + duration]);
            }
            ProjectileOverride::Vector { field, value } => *vector(&mut result, field)? = value,
            ProjectileOverride::Component { field, axis, value } => {
                vector(&mut result, field)?[axis as usize] = value
            }
            ProjectileOverride::AddComponent { field, axis, value } => {
                vector(&mut result, field)?[axis as usize] += value
            }
            ProjectileOverride::HitClassification {
                damage_kind,
                hit_class,
            } => {
                result.shape.damage_kind = damage_kind;
                result.shape.hit_class = hit_class;
            }
            ProjectileOverride::Reaction { value } => result.shape.reaction = value,
            ProjectileOverride::BirthEffect { id } => {
                result.spawn_effect = if id == 0 {
                    None
                } else {
                    Some(EffectId {
                        bank: result.birth_bank.context(
                            "cannot enable projectile birth with an unbound effect bank",
                        )?,
                        id,
                    })
                }
            }
        }
    }
    result.validate()?;
    *recipe = result;
    Ok(())
}

fn vector(recipe: &mut ProjectileRecipe, field: ProjectileVector) -> Result<&mut [f32; 3]> {
    Ok(match field {
        ProjectileVector::Velocity => match &mut recipe.movement {
            ProjectileMovement::Ballistic { velocity, .. } => velocity,
            ProjectileMovement::Homing { direction, .. }
            | ProjectileMovement::Directed { direction, .. } => direction,
        },
        ProjectileVector::Acceleration => match &mut recipe.movement {
            ProjectileMovement::Ballistic { acceleration, .. } => acceleration,
            ProjectileMovement::Homing { .. } | ProjectileMovement::Directed { .. } => {
                bail!("acceleration override on homing projectile needs its authored motion policy")
            }
        },
        ProjectileVector::SpawnOffset => &mut recipe.spawn_offset,
        ProjectileVector::VelocityJitter => &mut recipe.velocity_jitter,
    })
}

#[test]
fn native_strike_overrides_add_to_local_offset_and_preserve_the_shared_recipe() {
    use super::{
        actions::{HitShape, HitShapeKind},
        effects::KnockbackDirection,
    };
    let id = EffectId {
        bank: EffectBank::Techniques,
        id: 4,
    };
    let original = ProjectileRecipe {
        id: Some(id),
        lifetime: 20,
        movement: ProjectileMovement::Ballistic {
            velocity: [0.; 3],
            acceleration: [0.; 3],
            steering: None,
        },
        behavior: Default::default(),
        velocity_jitter: [0.; 3],
        spawn_offset: [1., 2., 10.],
        hit_offset: [0.; 3],
        shape: HitShape {
            radius: 40.,
            height: 400.,
            kind: HitShapeKind::Cylinder,
            inner_radius: 0.,
            damage_kind: 2,
            hit_class: 0,
            reaction: 1,
        },
        knockback: KnockbackDirection::Velocity,
        active: None,
        persist_after_hit: true,
        clashable: true,
        birth_bank: Some(EffectBank::Techniques),
        spawn_effect: Some(EffectId {
            bank: EffectBank::Techniques,
            id: 28,
        }),
        trail_effect: None,
        trail_interval: 1,
        ground_effect: None,
        shadow: None,
    };
    let mut customized = original.clone();
    apply_overrides(
        &[
            ProjectileOverride::ActiveWindow {
                start: 0,
                duration: 1,
            },
            ProjectileOverride::BirthEffect { id: 74 },
            ProjectileOverride::AddComponent {
                field: ProjectileVector::SpawnOffset,
                axis: Axis::Z,
                value: 120.,
            },
            ProjectileOverride::HitClassification {
                damage_kind: 0,
                hit_class: 1,
            },
        ],
        &mut customized,
    )
    .unwrap();
    assert_eq!(customized.active, Some([0, 1]));
    assert_eq!(original.active, None);
    assert_eq!(customized.spawn_offset, [1., 2., 130.]);
    assert_eq!(
        (customized.shape.damage_kind, customized.shape.hit_class),
        (0, 1)
    );
    assert_eq!(customized.spawn_effect.unwrap().id, 74);
    assert_eq!(original.spawn_offset, [1., 2., 10.]);
    assert_eq!(
        (original.shape.damage_kind, original.shape.hit_class),
        (2, 0)
    );
    assert_eq!(original.spawn_effect.unwrap().id, 28);
    assert!(
        apply_overrides(
            &[
                ProjectileOverride::HitClassification {
                    damage_kind: 2,
                    hit_class: 0
                },
                ProjectileOverride::AddComponent {
                    field: ProjectileVector::SpawnOffset,
                    axis: Axis::Z,
                    value: f32::INFINITY
                },
            ],
            &mut customized
        )
        .is_err()
    );
    assert_eq!(
        (customized.shape.damage_kind, customized.shape.hit_class),
        (0, 1)
    );
    apply_overrides(
        &[ProjectileOverride::ActiveWindow {
            start: 0,
            duration: 0,
        }],
        &mut customized,
    )
    .unwrap();
    assert_eq!(customized.active, None);
    customized.birth_bank = None;
    customized.spawn_effect = None;
    apply_overrides(
        &[ProjectileOverride::BirthEffect { id: 0 }],
        &mut customized,
    )
    .unwrap();
    assert!(
        apply_overrides(
            &[
                ProjectileOverride::Component {
                    field: ProjectileVector::SpawnOffset,
                    axis: Axis::X,
                    value: 99.
                },
                ProjectileOverride::BirthEffect { id: 74 },
            ],
            &mut customized
        )
        .is_err()
    );
    assert_eq!(customized.spawn_offset, [1., 2., 130.]);
    assert!(customized.spawn_effect.is_none());
}
