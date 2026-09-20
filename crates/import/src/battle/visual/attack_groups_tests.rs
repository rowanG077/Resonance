use super::*;
use crate::battle::actions::{Rel, hits};
use resonance_content::battle::actions::{
    AnimationCommand, AnimationInstruction, AnimationTrigger, HitAttachment, HitEmission,
};

fn named_rig(names: &[&[u8]]) -> Vec<u8> {
    let names_offset = 32 + names.len() * 28;
    let mut model = vec![0; names_offset];
    for (at, value) in [
        (0, 0x007b7960u32),
        (12, 32),
        (24, 1),
        (28, names_offset as u32),
    ] {
        model[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    model[6..8].copy_from_slice(&(names.len() as u16).to_be_bytes());
    for (index, name) in names.iter().enumerate() {
        if index + 1 < names.len() {
            let next = 32 + (index + 1) * 28;
            let at = 32 + index * 28 + 8;
            model[at..at + 4].copy_from_slice(&(next as u32).to_be_bytes());
        }
        model.extend_from_slice(name);
        model.push(0);
    }
    let mut resource = vec![0; 32];
    resource[4..8].copy_from_slice(&32u32.to_be_bytes());
    resource[8..12].copy_from_slice(&(model.len() as u32).to_be_bytes());
    resource.extend_from_slice(&model);
    resource
}

#[test]
fn rig_tags_keep_byte_arithmetic_and_effect_flag_membership() {
    let names: &[&[u8]] = &[
        b"Ef_Bone01",
        b"ef02",
        b"ef0/",
        b"at8_Bird06",
        b"mon_hair01",
        b"dm96",
        b"kk00",
    ];
    let bytes = named_rig(names);
    let actor = rig(&bytes, &[], RigKind::Actor).unwrap();
    assert_eq!(
        actor.effect_groups,
        [(0, vec![5]), (2, vec![0, 1]), (14, vec![4])].into()
    );
    assert_eq!(actor.attack_groups[&47], [3]);
    assert_eq!(actor.weapon_bones[&0], 6);
    assert_eq!(actor.skeleton.bones.len(), names.len());
    let weapon = rig(&bytes, &[], RigKind::Weapon).unwrap();
    assert_eq!(weapon.attack_groups[&8], [3]);
    assert!(weapon.effect_groups.is_empty());
    for name in ["ef", r"ef0\xb5"] {
        assert!(rig_tag_byte(name, 3).is_err());
    }
}

#[test]
fn neither_body_nor_carried_rigs_can_turn_missing_names_into_empty_contacts() {
    // Two identity nodes: the first has a source byte name, the second is AT0.
    let bytes = named_rig(&[b"\xb5", b"at00"]);
    for kind in [RigKind::Actor, RigKind::Weapon] {
        let parsed = rig(&bytes, &[], kind).unwrap();
        assert_eq!(parsed.attack_groups[&0], [1]);
        assert_eq!(parsed.skeleton.bones[0].name, "\\xb5");
        assert_eq!(parsed.skeleton.bones[1].name, "at00");
    }
    for malformed in [
        {
            let mut missing = bytes.clone();
            missing[60..64].fill(0);
            missing
        },
        {
            let mut truncated = bytes[..bytes.len() - 1].to_vec();
            let size = (truncated.len() - 32) as u32;
            truncated[8..12].copy_from_slice(&size.to_be_bytes());
            truncated
        },
    ] {
        assert!(rig(&malformed, &[], RigKind::Actor).is_err());
        assert!(rig(&malformed, &[], RigKind::Weapon).is_err());
    }
}

#[test]
#[ignore = "requires original extracted US weapon archive; parses source rig without cooking"]
fn original_long_sword_keeps_attack_and_trail_indices_after_non_utf8_names() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
    let weapons = fs::read(files.join("BTL/BTLwepon.dat")).unwrap();
    let range = indexed_range(&rel, 0x6e8, 158, 228 - 135).unwrap();
    let archive = MapArchive::decode(&weapons[range]).unwrap();
    let resource = archive.section(1).unwrap();
    assert_eq!(
        digest(resource),
        "b017c9d2fe2c8302170501803455e76a785cc50a6c9c3b3b8082dae35c7edaef"
    );
    let parsed = rig(resource, &[], RigKind::Weapon).unwrap();
    let mesh = super::super::pose::skeleton(resource).unwrap();
    assert!(
        parsed
            .skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(mesh.bones.iter().map(|bone| &bone.name))
    );
    assert_eq!(parsed.skeleton.bones.len(), 6);
    assert_eq!(parsed.attack_groups[&0], [2, 3]);
    assert_eq!(attachments::trail_bones(&parsed).unwrap(), [5, 4]);
    assert_eq!(
        parsed.skeleton.bones[0].name,
        "\\xb5\\xcc\\xde\\xbc\\xde\\xaa\\xb8\\xc401"
    );
    assert_eq!(
        parsed.skeleton.bones[1].name,
        "\\xb5\\xcc\\xde\\xbc\\xde\\xaa\\xb8\\xc402"
    );
}

#[test]
#[ignore = "requires original extracted US assets; parses source rig, CAB and hit rows without cooking"]
fn original_starfish_keeps_its_empty_second_strike_and_both_arm_motions() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let rel = Rel::read(&files.join("US_r_Top2Btl.rel")).unwrap();
    // Body initialization clears twelve records; contact emission loops each record's count.
    for (offset, size, hash) in [
        (
            0x1bd18,
            0x618,
            "5bbfdcf8ddd69b2f9c3a2377f102f87b960610f0b7e3a97d86dadb6a85af3a93",
        ),
        (
            0x2d564,
            0x568,
            "29d07012ac5e48874896d7c8a690d5b9262ecabf9c552ae598a23bbfc5515716",
        ),
    ] {
        assert_eq!(digest(&rel.at((1, offset)).unwrap()[..size]), hash);
    }
    let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let start = word(&usual, table + 141 * 4).unwrap() as usize;
    let end = word(&usual, table + 142 * 4).unwrap() as usize;
    assert_eq!((start, end), (36074272, 36137024));
    let package = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        digest(&package),
        "b98842b51339e5102684804cb5730ac4025e4b2d02c26cbc0e439d8bb49fa12c"
    );
    let metadata = half(&package, 4).unwrap() as usize;
    assert_eq!(
        (package[metadata + 0x1ad], package[metadata + 0x1e4]),
        (0, 0)
    );
    let model = &package[word(&package, 0x18).unwrap() as usize..];
    let body = rig(model, &enemy_clips(&package).unwrap(), RigKind::Actor).unwrap();
    assert_eq!(
        body.attack_groups.keys().copied().collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );
    assert_eq!(body.attack_groups[&0], [10, 20]);
    assert_eq!(body.attack_groups[&2], [15, 25]);
    for group in (0..12).filter(|&group| group != 0 && group != 2) {
        assert!(body.attack_groups[&group].is_empty());
    }
    assert_eq!(body.skeleton.bones[10].name, "at00_Bone_ude05_L");
    assert_eq!(body.skeleton.bones[20].name, "at00_Bone_ude05_R");
    // Carried models classify a different label column and overwrite only their own slots.
    let carried = rig(model, &[], RigKind::Weapon).unwrap();
    assert_eq!(carried.attack_groups.len(), 1);
    assert_eq!(carried.attack_groups[&0], [10, 15, 20, 25]);

    let action = half(&package, 10).unwrap() as usize;
    assert_eq!(half(&package, action + 14).unwrap(), 40);
    let hit =
        half(&package, 16).unwrap() as usize + half(&package, action + 28).unwrap() as usize * 32;
    let windows = hits(
        &package[hit..],
        &package[half(&package, 8).unwrap() as usize..],
    )
    .unwrap();
    assert_eq!(windows.len(), 2);
    for (window, (tick, group, count)) in windows.iter().zip([(4, 0, 2), (14, 1, 0)]) {
        assert_eq!(window.start, tick);
        assert!(
            matches!(&window.emission, HitEmission::Contact { duration: 4, attachment: HitAttachment::Groups(groups) } if groups == &[group])
        );
        assert_eq!(body.attack_groups[&group].len(), count);
    }
    let cab =
        half(&package, 18).unwrap() as usize + half(&package, action + 24).unwrap() as usize * 12;
    let program = crate::battle::animation_table::selected_at(&package, cab).unwrap();
    assert_eq!(program.commands().count(), 3);
    for (index, (tick, clip)) in [(0, 30), (10, 31), (35, 0)].into_iter().enumerate() {
        let command = if index == 0 {
            program.initial.unwrap()
        } else {
            let AnimationInstruction::Step(step) = &program.instructions[&(index as i16)] else {
                panic!("expected motion")
            };
            assert!(matches!(step.trigger, AnimationTrigger::Tick(t) if t == tick));
            step.command
        };
        assert!(matches!(command, AnimationCommand::Play { clip: c, rate: 0.5, .. } if c == clip));
        let motion = &body.motions[&u16::from(clip)];
        for time in [0., motion.duration_frames * 0.5, motion.duration_frames] {
            let pose = body.skeleton.sample(motion, time).unwrap();
            for bone in [10, 15, 20, 25] {
                assert!(
                    pose.point(bone, [0.; 3])
                        .unwrap()
                        .iter()
                        .all(|v| v.is_finite())
                );
            }
        }
    }
}
