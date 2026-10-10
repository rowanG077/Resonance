//! A shared native priority pool with authored per-source voice limits.
//! Startup protects equal-priority layers until their first macro pass. Child allocation
//! also protects its parent. External sample-stream reservations are separate.
use crate::{data::VoiceSource, sequence::VOICE_BUDGET};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lease {
    pub slot: usize,
    generation: u64,
}

struct Occupant {
    lease: Lease,
    source: VoiceSource,
    priority: u16,
    age: u32,
    initialized: bool,
    handle: u32,
    group_handle: Option<u32>,
    older: Option<Lease>,
    newer: Option<Lease>,
}

pub(super) struct Pool {
    slots: [Option<Occupant>; VOICE_BUDGET],
    available: VecDeque<usize>,
    order: u64,
    next_handle: u32,
    handles: BTreeMap<u32, Lease>,
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            available: (0..VOICE_BUDGET).collect(),
            order: 0,
            next_handle: 0,
            handles: BTreeMap::new(),
        }
    }
}

impl Pool {
    pub fn owns(&self, lease: Lease) -> bool {
        self.slots[lease.slot]
            .as_ref()
            .is_some_and(|slot| slot.lease == lease)
    }

    pub fn allocate(
        &mut self,
        source: VoiceSource,
        priority: u16,
        max_voices: u8,
    ) -> Option<Lease> {
        self.allocate_excluding(source, priority, max_voices, None)
    }

    pub fn child(&mut self, parent: Lease, priority: u16, max_voices: u8) -> Option<Lease> {
        let voice = self.slots[parent.slot]
            .as_ref()
            .filter(|voice| voice.lease == parent)?;
        let result = self.allocate_excluding(voice.source, priority, max_voices, Some(parent))?;
        let older = self.slots[parent.slot]
            .as_mut()
            .unwrap()
            .older
            .replace(result);
        if let Some(older) = older {
            self.slots[older.slot].as_mut().unwrap().newer = Some(result);
        }
        let child = self.slots[result.slot].as_mut().unwrap();
        child.group_handle = None;
        child.older = older;
        child.newer = Some(parent);
        Some(result)
    }

    pub fn handle(&self, lease: Lease) -> u32 {
        self.slots[lease.slot]
            .as_ref()
            .filter(|voice| voice.lease == lease)
            .map_or(u32::MAX, |voice| voice.handle)
    }

    pub fn resolve(&self, handle: u32) -> Option<Lease> {
        self.handles
            .get(&handle)
            .copied()
            .filter(|&lease| self.owns(lease))
    }

    fn unlink(&mut self, lease: Lease) {
        let voice = self.slots[lease.slot].as_ref().unwrap();
        let (group, older, newer) = (voice.group_handle, voice.older, voice.newer);
        self.handles.retain(|_, owner| *owner != lease);
        if let Some(newer) = newer {
            self.slots[newer.slot].as_mut().unwrap().older = older;
        } else if let Some(group) = group {
            if let Some(older) = older {
                self.handles.insert(group, older);
                self.slots[older.slot].as_mut().unwrap().group_handle = Some(group);
            } else {
                self.handles.remove(&group);
            }
        }
        if let Some(older) = older {
            self.slots[older.slot].as_mut().unwrap().newer = newer;
        }
    }

    fn allocate_excluding(
        &mut self,
        source: VoiceSource,
        priority: u16,
        max_voices: u8,
        blocked: Option<Lease>,
    ) -> Option<Lease> {
        let occupants = || self.slots.iter().flatten();
        let own_source =
            occupants().filter(|voice| voice.source == source).count() >= usize::from(max_voices);
        let candidate = if !own_source && !self.available.is_empty() {
            self.available.pop_front()
        } else {
            occupants()
                .filter(|voice| {
                    (voice.initialized || voice.priority < priority)
                        && Some(voice.lease) != blocked
                        && voice.priority <= priority
                        && (!own_source || voice.source == source)
                })
                .min_by_key(|voice| (voice.priority, voice.age, voice.lease.generation))
                .map(|voice| voice.lease.slot)
        }?;
        self.order += 1;
        if let Some(previous) = &self.slots[candidate] {
            self.unlink(previous.lease);
        }
        let lease = Lease {
            slot: candidate,
            generation: self.order,
        };
        while self.next_handle == u32::MAX || self.handles.contains_key(&self.next_handle) {
            self.next_handle = self.next_handle.wrapping_add(1);
        }
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        self.handles.insert(handle, lease);
        self.slots[candidate] = Some(Occupant {
            lease,
            source,
            priority,
            age: 0,
            initialized: false,
            handle,
            group_handle: Some(handle),
            older: None,
            newer: None,
        });
        Some(lease)
    }

    pub fn update(&mut self, lease: Lease, priority: (u16, u32), initialized: bool) {
        let Some(voice) = &mut self.slots[lease.slot] else {
            return;
        };
        if voice.lease != lease {
            return;
        }
        (voice.priority, voice.age) = priority;
        voice.initialized |= initialized;
    }

    pub fn free(&mut self, lease: Lease) {
        if self.owns(lease) {
            self.unlink(lease);
            self.slots[lease.slot] = None;
            self.available.push_back(lease.slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEQUENCE: VoiceSource = VoiceSource::Sequence {
        group: 7,
        program: 1,
        drums: false,
    };
    const SOUND: VoiceSource = VoiceSource::SoundEffect { id: 7 };

    #[test]
    fn handles_survive_root_retirement_but_not_child_retirement_or_slot_reuse() {
        let mut pool = Pool {
            next_handle: 0xffff_fffe,
            ..Default::default()
        };
        let root = pool.allocate(SOUND, 8, 255).unwrap();
        let first = pool.child(root, 8, 255).unwrap();
        let second = pool.child(root, 8, 255).unwrap();
        let (root_key, first_key, second_key) =
            (pool.handle(root), pool.handle(first), pool.handle(second));
        assert_eq!((root_key, first_key, second_key), (0xffff_fffe, 0, 1));
        pool.free(first);
        assert_eq!(pool.resolve(first_key), None);
        pool.free(root);
        assert_eq!(pool.resolve(root_key), Some(second));
        assert_eq!(pool.resolve(second_key), Some(second));
        pool.update(second, (8, 0), true);
        let replacement = pool.allocate(SOUND, 8, 1).unwrap();
        assert_eq!(replacement.slot, second.slot);
        assert_eq!(pool.resolve(root_key), None);
        assert_eq!(pool.resolve(second_key), None);
        assert_eq!(pool.resolve(pool.handle(replacement)), Some(replacement));
        pool.free(second);
        assert!(pool.owns(replacement));
        let child = pool.child(replacement, 8, 255).unwrap();
        let grandchild = pool.child(child, 8, 255).unwrap();
        let key = pool.handle(replacement);
        pool.free(replacement);
        assert_eq!(pool.resolve(key), Some(child));
        pool.free(child);
        assert_eq!(pool.resolve(key), Some(grandchild));
        pool.free(grandchild);
        assert_eq!(pool.resolve(key), None);
    }

    #[test]
    fn source_limits_protect_startup_and_stale_owners_cannot_release_replacements() {
        let mut pool = Pool::default();
        let first = pool.allocate(SOUND, 8, 1).unwrap();
        assert!(pool.allocate(SOUND, 8, 1).is_none());
        pool.update(first, (8, 100), true);
        assert!(pool.child(first, 256, 1).is_none());
        assert!(pool.allocate(SOUND, 7, 1).is_none());
        let replacement = pool.allocate(SOUND, 8, 1).unwrap();
        assert_eq!(replacement.slot, first.slot);
        assert_ne!(replacement, first);
        pool.free(first);
        pool.update(first, (0, 0), true);
        assert!(pool.owns(replacement));
        assert!(pool.allocate(SOUND, 8, 1).is_none());
        let important = pool.allocate(SOUND, 256, 1).unwrap();
        assert!(!pool.owns(replacement));
        assert!(pool.child(important, u16::MAX, 1).is_none());
        // Program/drum identity is independent of the shared macro number.
        for source in [
            SEQUENCE,
            VoiceSource::Sequence {
                group: 7,
                program: 1,
                drums: true,
            },
        ] {
            assert!(pool.allocate(source, 8, 1).is_some());
        }
    }

    #[test]
    fn music_and_sounds_share_the_budget_and_replace_by_priority_then_age() {
        let mut pool = Pool::default();
        let sounds: Vec<_> = (0..VOICE_BUDGET)
            .map(|_| pool.allocate(SOUND, 9, u8::MAX).unwrap())
            .collect();
        assert!(
            pool.allocate(SEQUENCE, 9, u8::MAX).is_none(),
            "equal-priority starting layers are protected until initialized"
        );
        for (age, &lease) in sounds.iter().enumerate() {
            pool.update(lease, (9, age as u32), true);
        }
        assert!(pool.allocate(SEQUENCE, 8, u8::MAX).is_none());
        let replacement = pool.allocate(SEQUENCE, 10, u8::MAX).unwrap();
        assert_eq!(replacement.slot, sounds[0].slot);
        assert!(!pool.owns(sounds[0]));
        assert!(pool.owns(replacement));
        pool.free(replacement);
        let reused = pool.allocate(SOUND, 0, u8::MAX).unwrap();
        assert_eq!(reused.slot, replacement.slot);
    }
}
