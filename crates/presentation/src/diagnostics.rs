#[derive(bevy::prelude::Resource, Clone, Default)]
pub(crate) struct Diagnostics(pub resonance_content::diagnostics::Diagnostics);

pub(crate) fn policy(world: &bevy::prelude::World) -> resonance_content::diagnostics::Diagnostics {
    world.get_resource::<Diagnostics>().map_or_else(
        || resonance_content::diagnostics::Diagnostics::new(true),
        |diagnostics| diagnostics.0.clone(),
    )
}
