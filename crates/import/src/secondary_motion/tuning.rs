//! Visual tuning resolved against the model's skeleton during cooking.
use anyhow::{Context, Result};
use resonance_content::secondary_motion::{Chain, CollisionPlane};

#[derive(Clone, Copy)]
enum Dynamics {
    Authored,
    WeightedHair,
    Hair,
    Uniform([f32; 3]),
    Tapered([f32; 3]),
}
use Dynamics::*;

type Plane = (&'static str, f32, f32, f32);
type Rule = (
    &'static str,
    &'static str,
    Dynamics,
    Option<usize>,
    Option<Plane>,
);

// Model tag, chain name fragment, dynamics, locked rotation axis, body plane.
// Plane values are anchor bone, normal Y, offset and strength. {side} selects
// the chain's left or right limb when a garment is attached to both sides.
const RULES: &[Rule] = &[
    (
        "llo00",
        "AB_ROOT_NR_FP_01_kami",
        Uniform([0.4165, -0.7333, 0.7666]),
        None,
        None,
    ),
    (
        "llo00",
        "manto_",
        Authored,
        None,
        Some(("Bone_sebone02", -1., 0., 1.)),
    ),
    (
        "col00",
        "_kami01",
        WeightedHair,
        Some(1),
        Some(("Bone_sebone02", -1., 1., 0.2)),
    ),
    (
        "col00",
        "_manto02_",
        Authored,
        Some(1),
        Some(("Bone_sebone02", -1., 1., 0.2)),
    ),
    (
        "col00",
        "_manto01_",
        Authored,
        Some(1),
        Some(("Bone_sebone02", 1., 0., 1.)),
    ),
    (
        "col00",
        "_kata_",
        Authored,
        Some(0),
        Some(("Bone_ude01_{side}", 1., 0., 1.)),
    ),
    (
        "ref00",
        "_manto01_",
        Authored,
        None,
        Some(("Bone_sebone03", 1., -1., 0.4)),
    ),
    (
        "ref00",
        "_manto02_",
        Tapered([0.008167, 1.6, 0.633]),
        None,
        Some(("Bone_sebone03", -1., 0., 1.)),
    ),
    (
        "ref00",
        "_manto03_",
        Tapered([0.008167, 1.6, 0.633]),
        Some(1),
        Some(("Bone_ashi01_{side}", 1., 0., 1.)),
    ),
    (
        "ref00",
        "_manto04_",
        Uniform([0.04, 3.933, 0.666]),
        Some(1),
        Some(("Bone_ashi01_{side}", -1., -1., 1.)),
    ),
    (
        "shi00",
        "AB_ROOT_FP_01_nuno02_",
        Tapered([0.008167, 1.6, 0.633]),
        Some(1),
        Some(("Bone_sebone01", -1., 0., 1.)),
    ),
    (
        "zel00",
        "AB_ROOT_FP_01_kami03",
        Hair,
        None,
        Some(("Bone_sebone02", -1., 0., 1.)),
    ),
    (
        "zel00",
        "AB_ROOT_FP_01_koshi_",
        Tapered([0.0665, 1.6, 0.633]),
        Some(1),
        Some(("Bone_sebone01", -1., 0., 1.)),
    ),
    (
        "pre00",
        "AB_ROOT_FP_01_osage_",
        Uniform([0.04, 1.266, 0.366]),
        Some(1),
        None,
    ),
    (
        "reg00",
        "AB_ROOT_FP_01_kami",
        Hair,
        None,
        Some(("Bone_kubi", -1., -10., 1.)),
    ),
    (
        "reg00",
        "AB_ROOT_FP_NR_01_momi",
        Uniform([0.158167, 2.566, 0.4]),
        None,
        None,
    ),
    (
        "kra00",
        "AB_ROOT_FP_01_manto02_",
        Tapered([0.0665, 1.6, 0.633]),
        Some(1),
        None,
    ),
];

pub(super) fn apply(model: &str, chain: &mut Chain, names: &[String]) -> Result<()> {
    chain.validate(names.len())?;
    let name = &names[usize::from(chain.joints[0].node)];
    let Some((_, _, dynamics, lock, plane)) = RULES
        .iter()
        .find(|(tag, fragment, ..)| model.contains(tag) && name.contains(fragment))
    else {
        return Ok(());
    };
    let uniform = match *dynamics {
        Hair => Some([0.116, 0.966, 0.633]),
        Uniform(values) | Tapered(values) => Some(values),
        _ => None,
    };
    if let Some([attraction, gravity, damping]) = uniform {
        chain.attraction = attraction;
        for joint in &mut chain.joints {
            joint.gravity = gravity;
            joint.damping = damping;
        }
    }
    if matches!(dynamics, Hair | WeightedHair) {
        chain.joints[0].gravity *= 1.66;
        chain.joints[0].damping *= 1.66;
    }
    if matches!(dynamics, Hair | WeightedHair | Tapered(_)) {
        for i in 1..chain.joints.len() {
            chain.joints[i].gravity = chain.joints[i - 1].gravity / 1.2;
            chain.joints[i].damping = chain.joints[i - 1].damping / 1.2;
        }
    }
    if let Some(axis) = lock {
        chain.rotation_locks[*axis] = true;
    }
    chain.collision_plane = plane
        .map(|(anchor, y, offset, strength)| {
            let anchor = anchor.replace("{side}", if name.contains("_L_") { "L" } else { "R" });
            let node = names
                .iter()
                .position(|name| name.starts_with(&anchor))
                .with_context(|| format!("{model} chain collision anchor {anchor}"))?;
            Ok::<_, anyhow::Error>(CollisionPlane {
                anchor: node.try_into()?,
                normal: [0., y, 0.],
                offset,
                strength,
            })
        })
        .transpose()?;
    chain.validate(names.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::secondary_motion::Joint;

    #[test]
    fn tuned_chains_bind_only_required_anchors_and_preserve_unrelated_chains() -> Result<()> {
        let mut chain = Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.5,
                    damping: 0.1,
                })
                .collect(),
            attraction: 0.019,
            preserve_rotation: true,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        let mut names = ["AB_ROOT_FP_01_kami", "joint", "tip", "Bone_kubi"].map(str::to_owned);
        apply("reg001", &mut chain, &names)?;
        assert_eq!(chain.collision_plane.as_ref().unwrap().anchor, 3);
        assert!(chain.joints.windows(2).all(|j| j[1].gravity < j[0].gravity));
        assert!(chain.preserve_rotation);
        assert!(apply("reg001", &mut chain, &names[..3]).is_err());
        names[0] = "unrelated_chain".into();
        let before = serde_json::to_vec(&chain)?;
        apply("reg001", &mut chain, &names)?;
        assert_eq!(serde_json::to_vec(&chain)?, before);
        Ok(())
    }
}
