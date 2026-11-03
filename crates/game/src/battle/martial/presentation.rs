//! Resolve optional authored presentation into the existing action event stream.
use super::Technique;
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, ensure};
use resonance_battle::{AttackEvent, AttackOrigin, ModelDefinition, SpellCamera, StageTint};
use resonance_content::battle_martial::Presentation;

impl Technique {
    pub(super) fn prepare_presentation(
        self,
        resources: &mut Resources<'_>,
        model: &ModelDefinition,
        source: Vec<(u16, Presentation)>,
        end_at: u16,
    ) -> Result<Vec<(u16, AttackEvent)>> {
        let mut events = Vec::new();
        for (at, event) in source {
            ensure!(
                at <= end_at,
                "authored presentation exceeds action duration"
            );
            let event = match event {
                Presentation::SkillEffect { member, bone } => {
                    let bank = resources
                        .skill_scenes
                        .get(&self.catalogue)
                        .context("authored presentation has no stored skill artwork")?
                        .clone();
                    let appearance = resources.prepared.effect_member(&bank, member)?;
                    if let Some(name) = bone {
                        let bone = model
                            .skeleton
                            .bones
                            .iter()
                            .position(|bone| bone.name == name)
                            .with_context(|| format!("authored effect is missing bone {name}"))?;
                        AttackEvent::EffectAt {
                            appearance,
                            origin: AttackOrigin::Bone(resonance_battle::MeleeAnchor {
                                resource: model.resource,
                                bone: bone.try_into()?,
                            }),
                            heading: None,
                            scale: Some(1.),
                            follow: false,
                        }
                    } else {
                        AttackEvent::InitializedActionEffect {
                            appearance,
                            origin: AttackOrigin::ActorRoot,
                        }
                    }
                }
                Presentation::StageTint { color, duration } => {
                    ensure!(duration > 0, "empty authored stage tint");
                    AttackEvent::StageTint(StageTint { color, duration })
                }
                Presentation::Camera { bounds, duration } => {
                    bounds.validate()?;
                    ensure!(duration > 0, "empty authored scene camera");
                    AttackEvent::SceneCamera(SpellCamera { bounds, duration })
                }
                Presentation::Cutin { duration } => {
                    ensure!(duration > 0, "empty authored cut-in");
                    AttackEvent::Cutin {
                        catalogue: resonance_battle::TechniqueId(self.catalogue),
                        duration,
                    }
                }
                Presentation::EquipSkillWeapon => AttackEvent::TemporaryWeapons(
                    resources
                        .skill_weapons
                        .get(&(self.character, self.catalogue))
                        .context("authored presentation has no stored skill weapon")?
                        .clone(),
                ),
                Presentation::RestoreWeapons => AttackEvent::RestoreWeapons,
                Presentation::WeaponVisibility { slot, visible } => AttackEvent::WeaponVisibility(
                    vec![(Some(resonance_battle::WeaponSlot(slot.0)), visible)],
                ),
            };
            events.push((at, event));
        }
        Ok(events)
    }
}
