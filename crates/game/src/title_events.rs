//! Title control uses the ordinary field resources without a playable party.
use resonance_content::field::FieldAssets;
use resonance_events::EventRuntime;
use std::sync::Arc;

pub fn start(
    bytes: &[u8],
    assets: &FieldAssets,
    read: impl FnMut(&str) -> anyhow::Result<Arc<[u8]>>,
) -> anyhow::Result<EventRuntime> {
    assets.validate()?;
    let mut resources = crate::field::resources(assets)?;
    crate::field::bind_attachments(
        &mut resources,
        crate::field::attachments::prepare(assets, read)?,
    )?;
    for model in resources.models.values_mut() {
        // The title draws actors before scripts read their bone matrices.
        model.attachment_pose_delay = 1;
    }
    resources
        .particles
        .insert(10, resonance_events::ParticleKind::Glow);
    EventRuntime::new(
        Arc::new(symphonia_script::Program::decode(bytes)?),
        Arc::new(resources),
    )
}
