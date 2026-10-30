//! Copy live technique state only when a Party snapshot is needed.
use super::*;

impl Candidate {
    pub(super) fn project_techniques(
        battle: &Battle,
        roster: &[(ActorId, Character)],
        party: &mut Party,
    ) {
        for &(actor, character) in roster {
            let member = &mut party.members[character.index()];
            if let Some(current) = battle.current_techniques(actor) {
                member.techniques = current.iter().map(|id| id.0).collect();
                member.unison_arte = member
                    .unison_arte
                    .filter(|id| member.techniques.contains(id));
                member
                    .disabled_techniques
                    .retain(|id| member.techniques.contains(id));
            }
            if let Some(shortcuts) = battle.shortcuts(actor) {
                member.shortcuts = shortcuts.map(|id| id.map_or(0, |id| id.0));
            }
            if let Some(counts) = battle.technique_counts(actor) {
                member.technique_uses = counts.iter().map(|(id, &count)| (id.0, count)).collect();
            }
            for technique in battle.prepared_techniques(actor) {
                if member.techniques.contains(&technique.catalogue.0)
                    && !battle.technique_enabled(actor, technique.action)
                {
                    member.disabled_techniques.insert(technique.catalogue.0);
                } else {
                    member.disabled_techniques.remove(&technique.catalogue.0);
                }
            }
        }
    }
}
