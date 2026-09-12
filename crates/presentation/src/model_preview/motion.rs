//! Preview dynamics reuse the field solver and share the resulting outline pose.
use super::*;
use crate::secondary_motion::Rig;
use bevy::transform::helper::TransformHelper;

pub(super) fn apply(
    mut state: State,
    viewer: Res<Viewer>,
    mut rigs: Query<&mut Rig>,
    mut transforms: ParamSet<(TransformHelper, Query<&mut Transform>)>,
) {
    let Some(menu) = state.menu() else { return };
    let Some(preview) = menu.preview() else {
        return;
    };
    let Some(primary) = viewer.parts.first().filter(|p| p.ready) else {
        return;
    };
    let Some(root) = primary.root else { return };
    let Ok(mut rig) = rigs.get_mut(root) else {
        return;
    };
    let roots = primary
        .spec
        .scene
        .clips
        .first()
        .map(|c| c.secondary_pose_nodes.as_slice())
        .unwrap_or_default();
    let helper = transforms.p0();
    let Some(pose) = rig.advance(&helper, preview.yaw, menu.tick, true, roots, None) else {
        return;
    };
    let mut locals = Vec::new();
    for part in viewer
        .parts
        .iter()
        .filter(|p| p.ready && p.spec.attached_to.is_none())
    {
        if let Some(root) = part.root
            && let Ok(rig) = rigs.get(root)
        {
            locals.extend(rig.locals(&helper, &pose));
        }
    }
    for (entity, local) in locals {
        if let Ok(mut transform) = transforms.p1().get_mut(entity) {
            *transform = local;
        }
    }
}
