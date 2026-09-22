//! Shared slot ownership and the game's sequence/SFX allocation limits.
//! Startup protects layer siblings until their first macro pass. Child allocation
//! also protects its parent. External sample-stream reservations are separate.
use crate::data::{ScoreOrigin, VoiceSource};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, VecDeque},
};

pub(crate) const VOICES: usize = 64;
const SEQUENCE_LIMIT: usize = 42;
const SOUND_EFFECT_LIMIT: usize = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lease {
    pub slot: usize,
    generation: u64,
}

struct Occupant {
    lease: Lease,
    source: VoiceSource,
    priority: u8,
    age: u32,
    revision: u64,
    order: u64,
    initialized: bool,
    lfo: u32,
    handle: u32,
    group_handle: Option<u32>,
    older: Option<Lease>,
    newer: Option<Lease>,
}

pub(super) struct Pool {
    slots: [Option<Occupant>; VOICES],
    available: VecDeque<usize>,
    lfo: [u32; VOICES],
    order: u64,
    next_handle: u32,
    handles: BTreeMap<u32, Lease>,
    dormant_mailboxes: [(u8, bool); VOICES],
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            available: (0..VOICES).collect(),
            lfo: [0; VOICES],
            order: 0,
            next_handle: 0,
            handles: BTreeMap::new(),
            dormant_mailboxes: [(0, false); VOICES],
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
        priority: u8,
        max_voices: u8,
    ) -> Option<(Lease, u32)> {
        self.allocate_excluding(source, priority, max_voices, None)
    }

    pub fn child(&mut self, parent: Lease, priority: u8, max_voices: u8) -> Option<(Lease, u32)> {
        let voice = self.slots[parent.slot]
            .as_ref()
            .filter(|voice| voice.lease == parent)?;
        let result = self.allocate_excluding(voice.source, priority, max_voices, Some(parent))?;
        let older = self.slots[parent.slot]
            .as_mut()
            .unwrap()
            .older
            .replace(result.0);
        if let Some(older) = older {
            self.slots[older.slot].as_mut().unwrap().newer = Some(result.0);
        }
        let child = self.slots[result.0.slot].as_mut().unwrap();
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

    pub fn resolve(&mut self, handle: u32) -> anyhow::Result<Option<Lease>> {
        let Some(owner) = self.handles.get(&handle) else {
            return Ok(None);
        };
        if let Some(voice) = &self.slots[owner.slot] {
            return Ok(Some(voice.lease));
        }
        // A retained alias can address a freed slot. Allocation later resets
        // its queue, so only capacity and the possibility of a trap are live.
        let (count, trap) = &mut self.dormant_mailboxes[owner.slot];
        if *count < 4 {
            *count += 1;
            anyhow::ensure!(
                !*trap,
                "message trap resurrection of a freed native voice slot is not implemented"
            );
        }
        Ok(None)
    }

    pub fn retain_mailbox(&mut self, lease: Lease, mailbox: (u8, bool)) {
        if self.owns(lease) {
            self.dormant_mailboxes[lease.slot] = mailbox;
        }
    }

    fn unlink(&mut self, lease: Lease) {
        let voice = self.slots[lease.slot].as_ref().unwrap();
        let (handle, group, older, newer) =
            (voice.handle, voice.group_handle, voice.older, voice.newer);
        self.handles.remove(&handle);
        if let Some(newer) = newer {
            self.slots[newer.slot].as_mut().unwrap().older = older;
        } else if let Some(group) = group {
            if let Some(older) = older {
                // Only the individual node is rewritten by native teardown.
                // A promoted root's distinct group alias retains its prior owner.
                if handle == group {
                    self.handles.insert(group, older);
                }
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
        priority: u8,
        max_voices: u8,
        blocked: Option<Lease>,
    ) -> Option<(Lease, u32)> {
        let limit = match source.origin() {
            ScoreOrigin::Sequence => SEQUENCE_LIMIT,
            ScoreOrigin::SoundEffect => SOUND_EFFECT_LIMIT,
        };
        let occupants = || self.slots.iter().flatten();
        let own_class = occupants()
            .filter(|voice| voice.source.origin() == source.origin())
            .count()
            >= limit;
        let own_source = usize::from(max_voices) < limit
            && occupants().filter(|voice| voice.source == source).count()
                >= usize::from(max_voices);
        let candidate = if !own_class && !own_source && !self.available.is_empty() {
            self.available.pop_front()
        } else {
            occupants()
                .filter(|voice| {
                    voice.initialized
                        && Some(voice.lease) != blocked
                        && voice.priority <= priority
                        && (!own_class || voice.source.origin() == source.origin())
                        && (!own_source || voice.source == source)
                })
                .min_by_key(|voice| (voice.priority, voice.age, Reverse(voice.order)))
                .map(|voice| voice.lease.slot)
        }?;
        if let Some(previous) = &self.slots[candidate] {
            self.lfo[candidate] = previous.lfo;
        }
        self.order += 1;
        // Same-priority replacement keeps its existing bucket position while
        // receiving a new ownership generation.
        let order = self.slots[candidate]
            .as_ref()
            .filter(|previous| previous.priority == priority)
            .map_or(self.order, |previous| previous.order);
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
        self.dormant_mailboxes[candidate] = (0, false);
        self.slots[candidate] = Some(Occupant {
            lease,
            source,
            priority,
            age: 60000 << 15,
            revision: 0,
            order,
            initialized: false,
            lfo: self.lfo[candidate],
            handle,
            group_handle: Some(handle),
            older: None,
            newer: None,
        });
        Some((lease, self.lfo[candidate]))
    }

    pub fn update(&mut self, lease: Lease, priority: (u8, u32, u64), lfo: u32, initialized: bool) {
        let Some(voice) = &mut self.slots[lease.slot] else {
            return;
        };
        if voice.lease != lease {
            return;
        }
        if voice.revision != priority.2 {
            self.order += 1;
            voice.order = self.order;
        }
        (voice.priority, voice.age, voice.revision) = priority;
        voice.lfo = lfo;
        voice.initialized |= initialized;
    }

    pub fn free(&mut self, lease: Lease, lfo: u32) {
        if self.owns(lease) {
            self.unlink(lease);
            self.slots[lease.slot] = None;
            self.lfo[lease.slot] = lfo;
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
        let root = pool.allocate(SOUND, 8, 255).unwrap().0;
        let first = pool.child(root, 8, 255).unwrap().0;
        let second = pool.child(root, 8, 255).unwrap().0;
        let (root_key, first_key, second_key) =
            (pool.handle(root), pool.handle(first), pool.handle(second));
        assert_eq!((root_key, first_key, second_key), (0xffff_fffe, 0, 1));
        pool.free(first, 0);
        assert_eq!(pool.resolve(first_key).unwrap(), None);
        pool.free(root, 0);
        assert_eq!(pool.resolve(root_key).unwrap(), Some(second));
        assert_eq!(pool.resolve(second_key).unwrap(), Some(second));
        pool.update(second, (8, 0, 0), 0, true);
        let replacement = pool.allocate(SOUND, 8, 1).unwrap().0;
        assert_eq!(replacement.slot, second.slot);
        assert_eq!(pool.resolve(root_key).unwrap(), None);
        assert_eq!(pool.resolve(second_key).unwrap(), None);
        assert_eq!(
            pool.resolve(pool.handle(replacement)).unwrap(),
            Some(replacement)
        );
        pool.free(second, 0);
        assert!(pool.owns(replacement));
        let child = pool.child(replacement, 8, 255).unwrap().0;
        let grandchild = pool.child(child, 8, 255).unwrap().0;
        let key = pool.handle(replacement);
        pool.free(replacement, 0);
        assert_eq!(pool.resolve(key).unwrap(), Some(child));
        pool.free(child, 0);
        assert_eq!(pool.resolve(key).unwrap(), None);
        // SendMessage follows the retained low-byte slot even after reuse.
        let offset = pool
            .available
            .iter()
            .position(|&slot| slot == child.slot)
            .unwrap();
        pool.available.rotate_left(offset);
        let reused = pool.allocate(SOUND, 8, 255).unwrap().0;
        assert_eq!(reused.slot, child.slot);
        assert_eq!(pool.resolve(key).unwrap(), Some(reused));
        pool.free(grandchild, 0);
        assert_eq!(pool.resolve(key).unwrap(), None);
    }

    #[test]
    fn source_limits_protect_startup_and_stale_owners_cannot_release_replacements() {
        let mut pool = Pool::default();
        let first = pool.allocate(SOUND, 8, 1).unwrap().0;
        assert!(pool.allocate(SOUND, 255, 1).is_none());
        pool.update(first, (8, 100 << 15, 0), 123, true);
        assert!(pool.child(first, 255, 1).is_none());
        assert!(pool.allocate(SOUND, 7, 1).is_none());
        let (replacement, lfo) = pool.allocate(SOUND, 8, 1).unwrap();
        assert_eq!((replacement.slot, lfo), (first.slot, 123));
        assert_ne!(replacement, first);
        pool.free(first, 999);
        pool.update(first, (0, 0, 0), 999, true);
        assert!(pool.owns(replacement));
        assert!(pool.allocate(SOUND, 255, 1).is_none());
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
    fn full_pool_steals_by_class_priority_fractional_age_and_registration_order() {
        let mut pool = Pool::default();
        let sequence: Vec<_> = (0..SEQUENCE_LIMIT)
            .map(|_| pool.allocate(SEQUENCE, 1, 255).unwrap().0)
            .collect();
        let sounds: Vec<_> = (0..SOUND_EFFECT_LIMIT)
            .map(|_| pool.allocate(SOUND, 9, 255).unwrap().0)
            .collect();
        assert!(pool.allocate(SOUND, 255, 255).is_none());
        for &lease in sequence.iter().chain(&sounds) {
            pool.update(
                lease,
                (if lease.slot < SEQUENCE_LIMIT { 1 } else { 9 }, 2 << 15, 0),
                0,
                true,
            );
        }
        assert!(pool.allocate(SOUND, 8, 255).is_none());
        // A child's caller remains protected even when it is the oldest voice
        // in the only eligible class; another matching voice may be replaced.
        pool.update(sounds[0], (9, 0, 0), 0, true);
        let child = pool.child(sounds[0], 9, 255).unwrap().0;
        assert_eq!(child.slot, sounds.last().unwrap().slot);
        assert!(pool.owns(sounds[0]));
        pool.update(child, (9, 2 << 15, 0), 0, true);
        // Fractional age beats list order even when the DSP integer priorities match.
        pool.update(sounds[0], (9, (1 << 15) + 3, 0), 0, true);
        pool.update(sounds[1], (9, (1 << 15) + 2, 0), 0, true);
        let first = pool.allocate(SOUND, 9, 255).unwrap().0;
        assert_eq!(first.slot, sounds[1].slot);
        pool.update(first, (9, 2 << 15, 0), 0, true);
        pool.update(sounds[0], (9, 2 << 15, 0), 0, true);
        assert_eq!(
            pool.allocate(SOUND, 9, 255).unwrap().0.slot,
            sounds.last().unwrap().slot
        );
        // Re-registering priority moves a voice to the bucket head; an unchanged
        // priority write leaves its previous order intact.
        pool.update(sounds[0], (9, 2 << 15, 2), 0, true);
        pool.update(sounds[2], (9, 2 << 15, 0), 0, true);
        assert_eq!(pool.allocate(SOUND, 9, 255).unwrap().0.slot, sounds[0].slot);
        assert!(sequence.iter().all(|&lease| pool.owns(lease)));
        // A freed slot returns to the FIFO without crossing a class quota.
        pool.free(sequence[0], 456);
        let (lease, lfo) = pool.allocate(SEQUENCE, 0, 255).unwrap();
        assert_eq!((lease.slot, lfo), (sequence[0].slot, 456));
    }
}
