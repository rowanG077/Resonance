//! Immutable recipes are validated against one source bank's model library.
//! Encounters select them with their own model, sound and resource bindings.
use super::*;

#[derive(Default)]
pub struct EffectLibrary {
    models: BTreeMap<u8, crate::PreparedEffectModel>,
    definitions: BTreeMap<u16, Recipe>,
    #[cfg(any(test, feature = "test-support"))]
    reuses: std::sync::atomic::AtomicU64,
}

struct Recipe {
    definition: Arc<EffectDefinition>,
    models: Vec<u8>,
}

impl EffectLibrary {
    pub fn new(models: BTreeMap<u8, crate::PreparedEffectModel>) -> Self {
        Self {
            models,
            ..Default::default()
        }
    }
    pub fn insert(&mut self, member: u16, definition: EffectDefinition) -> Result<()> {
        let mut used = std::collections::BTreeSet::new();
        definition.validate(
            &mut |slot| {
                used.insert(slot);
                self.models.get(&slot)
            },
            &|_| true,
        )?;
        self.definitions.insert(
            member,
            Recipe {
                definition: Arc::new(definition),
                models: used.into_iter().collect(),
            },
        );
        Ok(())
    }
    pub fn contains(&self, member: u16) -> bool {
        self.definitions.contains_key(&member)
    }
    pub fn len(&self) -> usize {
        self.definitions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn validation_reuses(&self) -> u64 {
        self.reuses.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn bind(
        &self,
        resource: ResourceId,
        members: &[u16],
        models: BTreeMap<u8, crate::PreparedEffectModel>,
        sound: &mut impl FnMut(u16) -> Result<Option<crate::Sound>>,
        diagnostics: &Diagnostics,
    ) -> Result<EffectBank> {
        let candidates: Vec<_> = members
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|member| {
                let candidate = self
                    .definitions
                    .get(&member)
                    .context("unprepared effect member")
                    .map(|recipe| {
                        let same = recipe.models.iter().all(|slot| {
                            models
                                .get(slot)
                                .is_some_and(|current| self.models[slot].same_definition(current))
                        });
                        #[cfg(any(test, feature = "test-support"))]
                        if same {
                            self.reuses
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        (recipe.definition.clone(), same)
                    });
                (member, candidate)
            })
            .collect();
        EffectBank::select(resource, models, candidates, sound, diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::EffectOperation;
    use resonance_content::{
        animation::{Motion, Skeleton},
        battle_effect::{ModelBinding, ScheduledEvent},
    };

    fn model(clip: Option<u16>) -> Result<crate::PreparedEffectModel> {
        crate::PreparedEffectModel::new(Arc::new(crate::EffectModelDefinition {
            resource: ResourceId(1),
            skeleton: Skeleton::single("root"),
            motions: clip
                .map(|clip| {
                    (
                        clip,
                        Arc::new(Motion {
                            duration_frames: 8.,
                            tracks: vec![],
                        }),
                    )
                })
                .into_iter()
                .collect(),
            secondary_motion: vec![],
        }))
    }
    fn definition(model: bool, child: bool) -> EffectDefinition {
        let mut particle = crate::tests::particle_definition(0);
        if model {
            particle.model = Some(ModelBinding {
                slot: 0,
                animation: Some(7),
                repeat: true,
                reverse_at: None,
                program_animation: false,
                attachment: None,
            });
        }
        if child {
            particle.data.ground_restitution = Some(0.5);
            particle.data.bounce_effect = Some(2);
        }
        EffectDefinition {
            events: vec![ScheduledEvent {
                at: 0,
                operation: EffectOperation::Spawn {
                    particle: 0,
                    blend: None,
                    palette: None,
                    birth: Default::default(),
                },
            }]
            .into(),
            particles: [(0, Arc::new(particle))].into(),
        }
    }
    #[test]
    fn validation_reuse_checks_current_models_while_banks_share_recipes() -> Result<()> {
        let original = model(Some(7))?;
        let mut library = EffectLibrary::new([(0, original.clone()), (1, model(None)?)].into());
        library.insert(0, definition(true, false))?;
        let bank = library.bind(
            ResourceId(99),
            &[0],
            [(0, original.with_resource(ResourceId(22)))].into(),
            &mut |_| unreachable!(),
            &Diagnostics::new(true),
        )?;
        assert_eq!(bank.resource(), ResourceId(99));
        assert_eq!(library.validation_reuses(), 1);
        for models in [BTreeMap::new(), [(0, model(None)?)].into()] {
            assert!(
                library
                    .bind(
                        ResourceId(99),
                        &[0],
                        models,
                        &mut |_| unreachable!(),
                        &Diagnostics::new(true)
                    )
                    .is_err()
            );
        }
        let other = library.bind(
            ResourceId(100),
            &[0],
            [(0, model(Some(7))?)].into(),
            &mut |_| unreachable!(),
            &Diagnostics::new(true),
        )?;
        assert!(Arc::ptr_eq(
            bank.member(0).unwrap(),
            other.member(0).unwrap()
        ));
        assert_eq!(library.validation_reuses(), 1);
        Ok(())
    }
    #[test]
    fn selected_child_closure_is_checked_even_for_a_validated_library() -> Result<()> {
        let mut library = EffectLibrary::default();
        library.insert(0, definition(false, true))?;
        assert!(
            library
                .bind(
                    ResourceId(99),
                    &[0],
                    BTreeMap::new(),
                    &mut |_| unreachable!(),
                    &Diagnostics::new(true)
                )
                .is_err()
        );
        let diagnostics = Diagnostics::default();
        let bank = library.bind(
            ResourceId(99),
            &[0],
            BTreeMap::new(),
            &mut |_| unreachable!(),
            &diagnostics,
        )?;
        assert!(bank.member(0).is_none());
        assert_eq!(diagnostics.entries().len(), 1);
        Ok(())
    }
}
