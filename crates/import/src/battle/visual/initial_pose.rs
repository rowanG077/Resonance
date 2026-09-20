use super::*;
use resonance_content::battle::visual::{InitialPose, PairedBody};

pub(super) fn metadata(bytes: &[u8], rig: &Rig) -> Result<InitialPose> {
    ensure!(bytes.len() > 0xc1, "truncated initial actor metadata");
    let clip = u16::from(if bytes[0xc1] == 0 {
        bytes[0x97]
    } else {
        bytes[0xc1]
    });
    let period = rig
        .motions
        .get(&clip)
        .context("missing initial actor clip")?
        .duration_frames;
    ensure!(
        period.is_finite() && period >= 1. && period <= f32::from(i16::MAX),
        "invalid initial actor motion period"
    );
    Ok(InitialPose {
        clip,
        blink: word(bytes, 0x5c)? & 0x8000 != 0,
        fixed_motion: word(bytes, 0x5c)? & 0x0800_0000 != 0,
    })
}

pub(super) fn paired(
    bytes: &[u8],
    metadata: &[u8],
    clips: &[SourceClip<'_>],
    body: &Rig,
) -> Result<Option<PairedBody>> {
    if half(metadata, 0xb4)? & 0x400 == 0 {
        return Ok(None);
    }
    // The separate secondary body is indexed independently of KK carried packages.
    let index = usize::from(metadata[0xbc]);
    ensure!(
        index < usize::from(metadata[0x1e8]),
        "paired body index exceeds secondary models"
    );
    ensure!(index == 0, "unrecovered paired body instance");
    let bone = (half(metadata, 0xb4)? & 0x8000 != 0)
        .then(|| {
            body.skeleton
                .bones
                .iter()
                .position(|bone| bone.name.contains("pa00"))
                .map(|index| index as u16)
                .context("missing paired body pa00 anchor")
        })
        .transpose()?;
    let offset = word(bytes, 0x180 + index * 4)? as usize;
    ensure!(offset != 0, "missing paired body resource");
    let rig = rig(
        bytes.get(offset..).context("paired body exceeds package")?,
        clips,
        RigKind::Actor,
    )?;
    let start = 1 + usize::from(word(bytes, 0x1c)? != 0);
    let count = 1 + usize::from(word(bytes, 0x198 + index * 4)? != 0);
    let initial_clip = u16::from(if metadata[0xc1] == 0 {
        metadata[0xbd]
    } else {
        metadata[0xc1]
    });
    ensure!(
        rig.motions
            .get(&initial_clip)
            .is_some_and(|motion| motion.duration_frames >= 1.
                && motion.duration_frames <= f32::from(i16::MAX)),
        "missing paired initial motion"
    );
    Ok(Some(PairedBody {
        bone,
        parts: (start..start + count).map(|index| index as u16).collect(),
        rig,
        initial_clip,
        clip_offset: u16::from(metadata[0xbd]),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The motion descriptor stores the maximum period in its raw B table.
    fn raw_period(anm: &[u8]) -> f32 {
        assert_eq!(word(anm, 0).unwrap(), 0x007b7960);
        let count = half(anm, 32).unwrap();
        (0..usize::from(count))
            .map(|i| float(anm, 36 + i * 16).unwrap())
            .fold(0., f32::max)
    }

    #[test]
    #[ignore = "requires original actor packages; validates every owner and costume alias without cooking"]
    fn original_initial_actor_clips_periods_and_blink_cover_all_owners_and_aliases() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let files = extracted.join("files");
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
        let section_table = word(&rel, 16).unwrap() as usize;
        let text = word(&rel, section_table + 8).unwrap() as usize & !3;
        let ro = word(&rel, section_table + 4 * 8).unwrap() as usize & !3;
        assert_eq!(
            digest(&rel[text + 0x52668..text + 0x52934]),
            "0f2f039e532f9713504385ca45a462154c286088ccd241f97961faaa396cccee"
        );
        assert_eq!(&rel[ro + 0x2a88..ro + 0x2a8d], b"pa00\0");
        assert_eq!(float(&rel, ro + 0x2a4c).unwrap(), -90.);
        // Classification reads case-folded prefixes and raw tag bytes. Its
        // effect consumers test flag0x80 and the low nibble, not the label.
        assert_eq!(
            digest(&rel[text + 0x1bd18..text + 0x1c330]),
            "5bbfdcf8ddd69b2f9c3a2377f102f87b960610f0b7e3a97d86dadb6a85af3a93"
        );
        assert_eq!(float(&rel, ro + 0x2a04).unwrap(), 0.5);
        assert_eq!(
            crate::read::f32(crate::dol::slice(&executable, 0x8035b8ac, 4).unwrap(), 0).unwrap(),
            0.5
        );
        // Fixed source call-site registers: clips26/24, flags1/8, zero start/blend.
        for (at, instruction) in [
            (0x52e50, 0x3880001a),
            (0x52e58, 0x38a00000),
            (0x52e5c, 0x38c00000),
            (0x52e68, 0x39200001),
            (0x52eac, 0x38800018),
            (0x52eb4, 0x38a00000),
            (0x52eb8, 0x38c00000),
            (0x52ec4, 0x39200008),
        ] {
            assert_eq!(word(&rel, text + at).unwrap(), instruction);
        }
        let data = super::super::rel_data(&rel).unwrap();
        let fst_bytes = fs::read(extracted.join("sys/fst.bin")).unwrap();
        let fst = nod::disc::fst::Fst::new(&fst_bytes).unwrap();
        let disc_files = fst
            .iter()
            .filter(|(_, node, _)| node.is_file())
            .map(|(_, _, name)| name.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut party_periods = Vec::new();
        let mut motion_aliases = std::collections::BTreeMap::new();
        let mut body_aliases = std::collections::BTreeMap::new();
        let mut absent_bodies = Vec::new();
        for character in 1..=9 {
            let row = &data[0x3d30 + usize::from(character - 1) * 496..];
            for costume in 0..5 {
                let package = party::archive(&executable, &files, character, costume).unwrap();
                let clip = if row[0xc1] == 0 { row[0x97] } else { row[0xc1] };
                let anm = package.section(usize::from(clip) + 2).unwrap();
                let period = raw_period(anm);
                assert!(period.is_finite() && period >= 1. && period <= f32::from(i16::MAX));
                motion_aliases.insert((character, costume), package.source_sha256.clone());
                // The body and motion tables are independent. Classify every declaration
                // against the original FST before attempting to decode its body.
                let index = u32::from(character - 1) * 5 + u32::from(costume);
                let pointer = word(
                    crate::dol::slice(&executable, 0x801face4 + index * 4, 4).unwrap(),
                    0,
                )
                .unwrap();
                let body_name = crate::dol::text(&executable, pointer).unwrap();
                let matches = disc_files
                    .iter()
                    .filter(|name| name.eq_ignore_ascii_case(&body_name))
                    .count();
                if matches == 0 {
                    // Disc1 really declares this absent body; do not substitute the
                    // motion archive's low-detail model or claim the body was decoded.
                    assert!(party::body(&executable, &files, character, costume).is_err());
                    absent_bodies.push((character, costume, body_name));
                    continue;
                }
                assert_eq!(
                    matches, 1,
                    "ambiguous original body declaration {body_name}"
                );
                let model = party::body(&executable, &files, character, costume).unwrap();
                body_aliases.insert((character, costume), digest(&model));
                let primary = &model[sections(&model).unwrap()[0].clone().unwrap()];
                let clips = [SourceClip {
                    slot: u16::from(clip),
                    bytes: anm,
                    resource: None,
                }];
                let rig = rig(primary, &clips, RigKind::Actor).unwrap_or_else(|error| {
                    panic!("party {character} costume {costume} body {body_name}: {error:#}")
                });
                let initial = metadata(row, &rig).unwrap();
                assert_eq!(initial.clip, u16::from(clip));
                assert_eq!(rig.motions[&initial.clip].duration_frames, raw_period(anm));
                assert_eq!(initial.blink, word(row, 0x5c).unwrap() & 0x8000 != 0);
                assert_eq!(half(row, 0xb4).unwrap() & 0x400, 0);
                if costume == 0 {
                    party_periods.push(raw_period(anm));
                }
            }
        }
        assert_eq!(motion_aliases.len(), 45);
        assert_eq!(body_aliases.len(), 44);
        assert_eq!(absent_bodies, [(9, 4, "kratos003.bin".to_owned())]);
        assert_eq!(
            body_aliases.len() + absent_bodies.len(),
            motion_aliases.len()
        );
        // Kratos's last two motion selections alias the ordinary archive; the
        // fourth body aliases costume2, while the fifth body is unavailable.
        for costume in [3, 4] {
            assert_eq!(motion_aliases[&(9, costume)], motion_aliases[&(9, 0)]);
        }
        assert_eq!(body_aliases[&(9, 3)], body_aliases[&(9, 2)]);
        assert_eq!(&party_periods[..3], &[60., 30., 40.]);
        let usual = fs::read(files.join("BTL/BTLusual.dat")).unwrap();
        let archive = fs::read(files.join("BTL/BTLenemy.dat")).unwrap();
        let table = word(&usual, 0x2c).unwrap() as usize;
        let mut paired_owners = Vec::new();
        let mut blink_owners = Vec::new();
        for owner in 0..251 {
            let start = word(&usual, table + owner * 4).unwrap() as usize;
            let end = word(&usual, table + (owner + 1) * 4).unwrap() as usize;
            let bytes = compression::decode(&archive[start..end]).unwrap();
            let row = &bytes[usize::from(half(&bytes, 4).unwrap())..];
            let clips = enemy_clips(&bytes)
                .unwrap_or_else(|error| panic!("enemy {owner} clips: {error:#}"));
            let rig = rig(
                &bytes[word(&bytes, 0x18).unwrap() as usize..],
                &clips,
                RigKind::Actor,
            )
            .unwrap_or_else(|error| panic!("enemy {owner} primary rig: {error:#}"));
            match owner {
                77 => assert_eq!(rig.effect_groups[&2], [3, 4, 5, 10, 16]),
                117 | 118 => assert_eq!(rig.attack_groups[&47], [0]),
                195 => assert_eq!(
                    rig.effect_groups[&14],
                    [
                        7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 33, 50, 58,
                        59, 60, 61, 69, 70, 71, 72
                    ]
                ),
                _ => {}
            }
            let initial = metadata(row, &rig)
                .unwrap_or_else(|error| panic!("enemy {owner} initial pose: {error:#}"));
            let raw = clips.iter().find(|clip| clip.slot == initial.clip).unwrap();
            assert_eq!(
                rig.motions[&initial.clip].duration_frames,
                raw_period(raw.bytes)
            );
            if initial.blink {
                blink_owners.push(owner);
            }
            let paired = paired(&bytes, row, &clips, &rig)
                .unwrap_or_else(|error| panic!("enemy {owner} paired body: {error:#}"));
            assert_eq!(paired.is_some(), half(row, 0xb4).unwrap() & 0x400 != 0);
            if let Some(paired) = paired {
                paired_owners.push((owner, paired.bone));
                assert_eq!(row[0xbc], 0);
                assert_eq!(row[0x1e8], 1);
                let clip = u16::from(if row[0xc1] == 0 { row[0xbd] } else { row[0xc1] });
                assert_eq!(paired.initial_clip, clip);
                assert_eq!(paired.clip_offset, u16::from(row[0xbd]));
                let paired_rig = &paired.rig;
                assert_eq!(
                    paired.parts,
                    if owner == 195 { vec![2] } else { vec![2, 3] }
                );
                if owner == 195 {
                    assert_eq!(paired_rig.effect_groups[&14], (1..13).collect::<Vec<_>>());
                    assert_eq!(rig.skeleton.bones[73].name, "pa00_be07_00_cape");
                    let mut missing_anchor = rig.clone();
                    missing_anchor.skeleton.bones[73].name = "unbound".into();
                    assert!(super::paired(&bytes, row, &clips, &missing_anchor).is_err());
                }
                let raw = clips.iter().find(|source| source.slot == clip).unwrap();
                assert_eq!(
                    paired_rig.motions[&clip].duration_frames,
                    raw_period(raw.bytes)
                );
                assert!(!paired_rig.motions[&clip].tracks.is_empty());
            }
        }
        assert_eq!(paired_owners, [(54, None), (55, None), (195, Some(73))]);
        assert_eq!(
            blink_owners,
            [
                3, 6, 7, 21, 22, 24, 25, 30, 31, 35, 84, 85, 89, 91, 92, 108, 109, 113, 114, 115,
                116, 136, 137, 153, 156, 183, 245, 248, 249
            ]
        );
    }
}
