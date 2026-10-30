//! Commit complete weapon slots without exposing staging or generic geometry.
use super::*;

pub(crate) fn publish(
    state: State,
    cache: Res<EquipmentCache>,
    actors: Query<(Entity, &ActorPart)>,
    mut roots: Query<(&mut Instance, &mut Visibility)>,
    mut visibility: Query<&mut Visibility, Without<Instance>>,
) {
    let world = &state.get().events.world;
    let layers: Vec<_> = roots.iter().map(|(root, _)| root.state()).collect();
    let mut selected = BTreeMap::new();
    for (owner, part) in &actors {
        if part.part != 0 || part.pass != 0 || part.disabled {
            continue;
        }
        let Some((binding, item)) = world
            .actors
            .get(&part.actor)
            .and_then(|actor| selection(actor, world))
        else {
            continue;
        };
        let failed = cache.failed.contains(&item);
        let model = cache.equipment.get(&item).filter(|model| model.ready);
        let empty = item == 0 || model.is_some_and(|model| model.part(binding).is_none());
        let next = if empty || failed {
            None
        } else if model.is_some_and(|model| model.complete(owner, item, binding, &layers)) {
            Some(item)
        } else {
            // Active layers were committed together in an earlier frame.
            let mut previous = layers
                .iter()
                .filter(|layer| layer.key.0 == owner && layer.active)
                .peekable();
            let item = previous.peek().map(|layer| layer.key.1);
            item.filter(|_| previous.all(|layer| layer.ready))
        };
        let visible = visibility
            .get(owner)
            .is_ok_and(|v| *v != Visibility::Hidden)
            && part.geometry.iter().any(|(_, entity)| {
                visibility
                    .get(*entity)
                    .is_ok_and(|v| *v != Visibility::Hidden)
            });
        selected.insert(owner, (next, visible));
        // A pending item has no generic fallback. An already drawn slot keeps
        // its previous model; a new draw holds at its birth tick until ready.
        if !failed {
            for (_, source) in actors
                .iter()
                .filter(|(_, source)| source.actor == part.actor && source.pass == part.pass)
            {
                for &(_, mesh) in &source.geometry {
                    if let Ok(mut visible) = visibility.get_mut(mesh) {
                        *visible = Visibility::Hidden;
                    }
                }
            }
        }
    }
    for (mut root, mut visible) in &mut roots {
        let selection = selected.get(&root.owner);
        root.active = selection.is_some_and(|(item, _)| *item == Some(root.item));
        *visible = if root.active && selection.is_some_and(|(_, visible)| *visible) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
