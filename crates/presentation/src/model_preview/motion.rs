//! Preview dynamics reuse the field solver and share the resulting outline pose.
use super::*;
use crate::secondary_motion::Rig;
use crate::sparse_animation::affine::{Helper as TransformHelper, Locals};

#[allow(clippy::type_complexity)] // Snapshot authored world poses before writing local bones.
pub(super) fn apply(
    mut state: State,
    viewer: Res<Viewer>,
    mut rigs: Query<&mut Rig>,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
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
        .clip
        .map(|index| {
            primary.spec.scene.clips[index]
                .secondary_pose_nodes
                .as_slice()
        })
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
    let (mut transforms, mut affine) = transforms.p1();
    for (entity, local) in locals {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            affine.set(entity, &mut transform, local);
        }
    }
}
