//! Immutable startup definitions. Misses use the ordinary encounter reader.
use super::*;
use resonance_battle::EffectLibrary;
use std::sync::Weak;

#[derive(Default)]
struct Bank {
    source: Weak<SourceBank>,
    particles: BTreeMap<u8, Arc<ParticleDefinition>>,
    programs: EffectLibrary,
}

#[derive(Default)]
pub(crate) struct Cache {
    banks: BTreeMap<usize, Bank>,
    #[cfg(any(test, feature = "test-support"))]
    hits: std::sync::atomic::AtomicU64,
}

impl Cache {
    fn get(&self, source: &Arc<SourceBank>, members: &[u16]) -> Option<&EffectLibrary> {
        let bank = self.banks.get(&(Arc::as_ptr(source) as usize))?;
        if !bank
            .source
            .upgrade()
            .is_some_and(|previous| Arc::ptr_eq(&previous, source))
            || !members.iter().all(|&member| bank.programs.contains(member))
        {
            return None;
        }
        #[cfg(any(test, feature = "test-support"))]
        self.hits.fetch_add(
            members.iter().copied().collect::<BTreeSet<_>>().len() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        Some(&bank.programs)
    }

    pub(crate) fn warm(
        &mut self,
        source: &Arc<SourceBank>,
        models: &BTreeMap<u8, resonance_battle::PreparedEffectModel>,
        cancelled: &impl Fn() -> bool,
    ) {
        let bank = self
            .banks
            .entry(Arc::as_ptr(source) as usize)
            .or_insert_with(|| Bank {
                source: Arc::downgrade(source),
                programs: EffectLibrary::new(models.clone()),
                ..Default::default()
            });
        for member in 0..source.programs.len() {
            if cancelled() {
                return;
            }
            let Ok(member) = u16::try_from(member) else {
                break;
            };
            if bank.programs.contains(member) {
                continue;
            }
            // Failed/unused recipes retain normal selected-encounter diagnostics.
            let _ = super::definition(source, member, &mut bank.particles)
                .and_then(|definition| bank.programs.insert(member, definition));
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn hits(&self) -> u64 {
        self.hits.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.banks.values().map(|bank| bank.programs.len()).sum()
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn validations(&self) -> u64 {
        self.banks
            .values()
            .map(|bank| bank.programs.validation_reuses())
            .sum()
    }
}

pub(crate) type Programs = Arc<Cache>;

impl Cache {
    pub(crate) fn prepare(
        &self,
        source: &Arc<SourceBank>,
        resource: ResourceId,
        members: &[u16],
        models: BTreeMap<u8, resonance_battle::PreparedEffectModel>,
        sound: &mut impl FnMut(u16) -> Result<Option<Sound>>,
        diagnostics: &Diagnostics,
    ) -> Result<EffectBank> {
        match self.get(source, members) {
            Some(programs) => programs.bind(resource, members, models, sound, diagnostics),
            None => super::prepare(source, resource, members, models, sound, diagnostics),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_effect::ScheduledEvent;

    fn source() -> Result<Arc<SourceBank>> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../battle/tests/fixtures/stun-particle-source.json"
        )))?;
        let declaration = serde_json::from_value(fixture["declaration"].clone())?;
        let mut events = vec![ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 0,
                blend: None,
                palette: None,
                birth: Default::default(),
            },
        }];
        events.extend([9, 5, 9, 0].map(|id| ScheduledEvent {
            at: 0,
            operation: EffectOperation::Sound { id, priority: 0 },
        }));
        Ok(Arc::new(SourceBank {
            document: None,
            template_names: vec![],
            programs: vec![events.clone(), events],
            actors: vec![declaration],
            art: None,
        }))
    }

    #[test]
    fn cached_programs_share_timelines_and_bind_audio_per_bank() -> Result<()> {
        let mut source = source()?;
        let mut cache = Cache::default();
        cache.warm(&source, &BTreeMap::new(), &|| false);
        assert_eq!(cache.len(), 2);
        let diagnostics = Diagnostics::new(true);
        let mut calls = Vec::new();
        let first = cache.prepare(
            &source,
            ResourceId(7),
            &[1, 0, 1],
            BTreeMap::new(),
            &mut |id| {
                calls.push(id);
                Ok((id == 5).then_some(Sound::Cue(105)))
            },
            &diagnostics,
        )?;
        assert_eq!(calls, [9, 5], "resolve in first-use order, once per bank");
        assert_eq!(
            (first.sound(9), first.sound(5)),
            (None, Some(Sound::Cue(105)))
        );
        let second = cache.prepare(
            &source,
            ResourceId(99),
            &[0, 1],
            BTreeMap::new(),
            &mut |id| Ok(Some(Sound::Stream(id + 100))),
            &diagnostics,
        )?;
        assert_eq!(second.sound(9), Some(Sound::Stream(109)));
        assert!(Arc::ptr_eq(
            &first.member(0).unwrap().events,
            &second.member(0).unwrap().events
        ));
        assert_eq!(cache.hits(), 4);

        // An edited source is prepared afresh, with ordinary diagnostics.
        Arc::make_mut(&mut source).programs[0].push(ScheduledEvent {
            at: 1,
            operation: EffectOperation::Notice {
                text: String::new(),
            },
        });
        let tolerant = Diagnostics::default();
        let edited = cache.prepare(
            &source,
            ResourceId(7),
            &[0, 1],
            BTreeMap::new(),
            &mut |id| Ok(Some(Sound::Cue(id))),
            &tolerant,
        )?;
        assert!(edited.member(0).is_none() && edited.member(1).is_some());
        assert_eq!(tolerant.entries().len(), 1);
        assert_eq!((cache.hits(), cache.len()), (4, 2));
        Ok(())
    }
}
