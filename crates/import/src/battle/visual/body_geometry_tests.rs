use super::*;
use resonance_content::battle::pose::{Bone, Skeleton};

#[test]
fn body_geometry_uses_rig_identity_without_shifting_escaped_names() {
    let rig = Rig {
        skeleton: Skeleton {
            bones: [r"\xb5", r"dm05_tail\r\n", "mo07_body"]
                .into_iter()
                .map(|name| Bone {
                    bind_channels: Default::default(),
                    name: name.into(),
                    parent: None,
                    bind: Default::default(),
                })
                .collect(),
        },
        motions: Default::default(),
        attack_groups: Default::default(),
        effect_groups: Default::default(),
        weapon_bones: Default::default(),
    };
    let (volumes, bounds) = body_geometry(&rig, 1., &[]).unwrap();
    assert_eq!(
        volumes
            .iter()
            .map(|v| (rig.skeleton.bone(&v.bone).unwrap(), v.kind, v.radius))
            .collect::<Vec<_>>(),
        [
            (1, VolumeKind::Hurt, 50.),
            (2, VolumeKind::BodyAndHurt, 70.)
        ]
    );
    assert_eq!(bounds, [0, 2]);
    assert_eq!(rig.skeleton.bone("dm05_tail\r\n"), None);
    // An escaped numeric prefix must not be mistaken for source byte '\'.
    assert!(super::volumes(&[r"mo\xb5".into()], 1.).is_err());
}

#[test]
#[ignore = "requires original extracted US enemy packages; no texture cooking"]
fn original_adulocia_and_amphitra_body_volumes_bind_every_source_node() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for (monster, hash, count, expected) in [
        (
            182,
            "2c1b7c507c1f4a2dfeb75a725e7c9a722099666d1d9e02b35dc1894d0d4d72a7",
            54,
            vec![
                (3, VolumeKind::BodyAndHurt, 70.),
                (46, VolumeKind::BodyAndHurt, 70.),
                (53, VolumeKind::Hurt, 50.),
            ],
        ),
        (
            183,
            "e335b6b57a06739d0902ed15bd59d6b6448e5de3781f9d73451053b22b252bce",
            55,
            vec![
                (41, VolumeKind::BodyAndHurt, 60.),
                (46, VolumeKind::BodyAndHurt, 40.),
            ],
        ),
        (
            74,
            "2c61c033e0fc4285954005c37063de22e7432e67a0f9c445401e95bf8974ac02",
            21,
            vec![(1, VolumeKind::BodyAndHurt, 63.)],
        ),
    ] {
        let start = word(&usual, table + monster * 4).unwrap() as usize;
        let end = word(&usual, table + (monster + 1) * 4).unwrap() as usize;
        let package = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(digest(&package), hash);
        let metadata = half(&package, 4).unwrap() as usize;
        assert_eq!(package[metadata + 0x1e4], 0);
        let resource = &package[word(&package, 0x18).unwrap() as usize..];
        let rig = rig(resource, &enemy_clips(&package).unwrap(), RigKind::Actor).unwrap();
        let mesh = super::super::pose::skeleton(resource).unwrap();
        assert!(
            rig.skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(mesh.bones.iter().map(|bone| &bone.name))
        );
        assert_eq!(rig.skeleton.bones.len(), count);
        assert_eq!(
            rig.attack_groups.keys().copied().collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        let (volumes, bounds) =
            body_geometry(&rig, float(&package, metadata + 0x84).unwrap(), &[]).unwrap();
        assert_eq!(
            volumes
                .iter()
                .map(|v| (rig.skeleton.bone(&v.bone).unwrap(), v.kind, v.radius))
                .collect::<Vec<_>>(),
            expected
        );
        if monster == 182 {
            let source_name = b"dm05Bone_tail11_R\r\n\0";
            assert!(
                package
                    .windows(source_name.len())
                    .any(|bytes| bytes == source_name)
            );
            assert_eq!(rig.skeleton.bones[53].name, r"dm05Bone_tail11_R\r\n");
            assert!(!bounds.contains(&53));
            assert_eq!(rig.attack_groups[&0], [33]);
            assert_eq!(rig.attack_groups[&1], [41]);
            assert_eq!(rig.attack_groups[&2], [49]);
        } else if monster == 183 {
            assert_eq!(rig.attack_groups[&0], [9]);
            assert_eq!(rig.attack_groups[&2], [47]);
            assert_eq!(rig.attack_groups[&3], [48]);
        } else {
            assert_eq!(rig.attack_groups[&0], [6, 8, 11, 20]);
        }
    }
}
