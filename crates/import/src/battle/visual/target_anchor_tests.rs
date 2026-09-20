use super::*;
use resonance_content::battle::pose::{Bone, Skeleton};

#[test]
fn target_anchor_distinguishes_world_axes_from_the_rotated_joint_offset() {
    let rig = Rig {
        skeleton: Skeleton {
            bones: (0..2)
                .map(|i| Bone {
                    bind_channels: Default::default(),
                    name: i.to_string(),
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
    let mut row = vec![0; 0x88];
    for (at, value) in [
        (0x60, 3f32),
        (0x64, 5.),
        (0x68, 7.),
        (0x78, 11.),
        (0x7c, 13.),
        (0x80, 17.),
        (0x84, 0.5),
    ] {
        row[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    let root = target_anchor(&row, &rig).unwrap();
    assert_eq!((root.bone, root.offset), (None, [3., 5., 7.]));
    row[0x51] = 1;
    let joint = target_anchor(&row, &rig).unwrap();
    assert_eq!((joint.bone, joint.offset), (Some(1), [5.5, 6.5, 8.5]));
    row[0x51] = 2;
    assert_eq!(target_anchor(&row, &rig).unwrap().bone, Some(0));
}

#[test]
#[ignore = "requires original extracted US battle packages; parses metadata and rigs without texture cooking"]
fn original_battle_target_anchors_preserve_source_joint_selection_and_offsets() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
    let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
    let section_table = word(&rel, 16).unwrap() as usize;
    let constants = word(&rel, section_table + 4 * 8).unwrap() as usize & !3;
    assert_eq!(float(&rel, constants + 0x15b8).unwrap(), 2.);
    assert_eq!(float(&rel, constants + 0x4558).unwrap(), 50.);
    assert_eq!(
        &rel[constants + 0x4584..constants + 0x4592],
        b"Cancel orders\0"
    );
    assert_eq!(&rel[constants + 0x457c..constants + 0x4583], b"    %d\0");
    let bank = &usual[sections(&usual).unwrap()[4].clone().unwrap()];
    let atlas = &bank[sections(bank).unwrap()[3].clone().unwrap()];
    let mut texture = crate::tpl::parse_tpl(atlas).unwrap().remove(0);
    assert_eq!(
        (texture.width, texture.height, texture.palette_entries),
        (512, 512, 96 * 16)
    );
    texture.palette_offset = Some(texture.palette_offset.unwrap() + 17 * 16 * 2);
    texture.palette_entries = 16;
    let pixels = crate::tpl::decode_texture(atlas, &texture).unwrap();
    let mut frames = std::collections::BTreeSet::new();
    for frame in 0..3 {
        let x = 48 + frame * 48;
        let rgba: Vec<_> = (256..320)
            .flat_map(|y| {
                pixels[(y * 512 + x) * 4..(y * 512 + x + 48) * 4]
                    .iter()
                    .copied()
            })
            .collect();
        assert!(
            rgba.chunks_exact(4).any(|pixel| pixel[3] != 0),
            "empty pointer animation frame {frame}"
        );
        frames.insert(digest(&rgba));
    }
    assert_eq!(
        frames.len(),
        3,
        "all three source pointer frames must be recovered"
    );
    let table = word(&usual, 0x2c).unwrap() as usize;
    for id in [36, 74, 182, 183, 195] {
        let start = word(&usual, table + id * 4).unwrap() as usize;
        let end = word(&usual, table + (id + 1) * 4).unwrap() as usize;
        let package = compression::decode(&archive[start..end]).unwrap();
        let metadata = &package[half(&package, 4).unwrap() as usize..];
        let rig = rig(
            &package[word(&package, 0x18).unwrap() as usize..],
            &[],
            RigKind::Actor,
        )
        .unwrap();
        let actual = target_anchor(metadata, &rig).unwrap();
        let joint = metadata[0x51];
        let selected = if usize::from(joint) < rig.skeleton.bones.len() {
            u16::from(joint)
        } else {
            0
        };
        assert_eq!(actual.bone, (joint != 0).then_some(selected));
        let offset = if joint == 0 { 0x60 } else { 0x78 };
        let scale = float(metadata, 0x84).unwrap() * if joint == 0 { 2. } else { 1. };
        assert_eq!(
            actual.offset,
            std::array::from_fn(|axis| float(metadata, offset + axis * 4).unwrap() * scale)
        );
    }
}
