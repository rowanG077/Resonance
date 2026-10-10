//! Copy live technique state only when a Party snapshot is needed.
use super::*;

impl Candidate {
    pub(super) fn project_techniques(
        battle: &Battle,
        party_members: &BTreeMap<ActorId, usize>,
        party: &mut Party,
    ) {
        for (&actor, &index) in party_members {
            let member = &mut party.members[index];
            if let Some(current) = battle.current_techniques(actor) {
                member.techniques.clone_from(current);
                member.disabled_techniques.retain(|id| current.contains(id));
            }
            if let Some(shortcuts) = battle.shortcuts(actor) {
                member.shortcuts = *shortcuts;
            }
            if let Some(counts) = battle.technique_counts(actor) {
                member.technique_uses.clone_from(counts);
            }
            for technique in battle.prepared_techniques(actor) {
                if member.techniques.contains(&technique.catalogue)
                    && !battle.technique_enabled(actor, technique.action)
                {
                    member.disabled_techniques.insert(technique.catalogue);
                } else {
                    member.disabled_techniques.remove(&technique.catalogue);
                }
            }
        }
    }
}
