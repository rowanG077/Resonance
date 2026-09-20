//! Caster recovery motions are independent of the spell's release and concluding poses.
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Parameters {
    rate: f32,
    blend: u8,
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        let rate = float(rel.at((4, 0x19a0))?, 0)?;
        let blend = word(rel.at((1, 0x302a0))?, 0)?;
        ensure!(
            rate.is_finite() && rate > 0. && blend >> 16 == 0x38a0,
            "unexpected spell recovery playback"
        );
        Ok(Self {
            rate,
            blend: u8::try_from(blend & 0xffff)?,
        })
    }

    pub fn animation(
        &self,
        metadata: &crate::battle::all::ActorSettings,
    ) -> Result<Option<AnimationCommand>> {
        let clip = metadata.casting.stored_recovery_clip;
        if clip == 0 {
            return Ok(None);
        }
        ensure!(clip <= i8::MAX as u8, "unsupported spell recovery clip");
        Ok(Some(AnimationCommand::Play {
            clip,
            blend: self.blend,
            start: 0,
            end: None,
            layer: 8,
            looping: false,
            mirror: false,
            resource: -1,
            rate: self.rate,
        }))
    }
}

#[test]
#[ignore = "requires original extracted disc; parses metadata and motion without encoding assets"]
fn original_recovery_dispatch_all_casters_and_kratos_motion() {
    use sha2::{Digest, Sha256};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let tables = Tables::original(&extracted, &rel, &usual, &[]).unwrap();
    assert_eq!(rel.pointer(DATA, 0xbd4 + 8 * 4).unwrap(), (1, 0x301a4));
    // Complete recovery, release-completion, transition, dispatcher and compound-EX bodies.
    for (offset, size, digest) in [
        (
            0x301a4,
            0x4bc,
            "df4df99a9fc555fd6b26f0538e483ad24cffe3331a81c5b29c3792df4abaa604",
        ),
        (
            0x385a0,
            0x3ec,
            "dc722968cb68f432e06f9e521f924e026001d7b1f32633515015bd94d746fc87",
        ),
        (
            0x295b8,
            0x1b8,
            "dbfa435270c47568230b3ca7cba378f9e933b5b0237130be0423394d0d532cea",
        ),
        (
            0x31c88,
            0x2e0,
            "428981b4fba5f56ffc9dbcc8b04620516397097832c0e232d374532445ca180e",
        ),
        (
            0x1c74c,
            0x120,
            "6700b117d28b108b63f50d3af1f32c309e7c9b3297e96e9d4b87fd673efcf538",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, offset)).unwrap()[..size])
            ),
            digest
        );
    }
    for character in 1..=9 {
        let metadata = tables.actor(character).unwrap();
        let pose = tables.recovery.recovery_pose.animation(metadata).unwrap();
        assert_eq!(
            metadata.casting.stored_recovery_clip,
            if character == 9 { 34 } else { 0 }
        );
        assert_eq!(pose.is_some(), character == 9);
        if let Some(pose) = pose {
            assert!(matches!(
                pose,
                AnimationCommand::Play {
                    clip: 34,
                    blend: 4,
                    start: 0,
                    end: None,
                    layer: 8,
                    looping: false,
                    mirror: false,
                    resource: -1,
                    rate: 0.5,
                }
            ));
        }
    }
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let archive =
        crate::battle::visual::party::archive(&executable, &extracted.join("files"), 9, 0).unwrap();
    // The visual cooker keeps every populated slot; member36 is motion34.
    let clip = archive.section(36).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(clip)),
        "f57e254e4393cba75a94f3a6b13e6919eef2f477358f2260a8237856f131e5b6"
    );
    assert_eq!(word(clip, 0).unwrap(), 0x007b7960);
    let tracks = 24 + usize::from(half(clip, 10).unwrap()) * 12;
    let duration = (0..usize::from(half(clip, 12).unwrap()))
        .map(|index| float(clip, tracks + index * 16).unwrap())
        .fold(0f32, f32::max);
    assert_eq!((duration, (duration / 0.5).trunc() + 4.), (22., 48.));
    let recipes = technique_actions(&extracted, &rel, &usual, &[67, 68, 69, 214]).unwrap();
    for (recipe, recovery_ticks) in recipes.iter().zip([90, 150, 90, 60]) {
        assert_eq!(recipe.properties.recovery_ticks, recovery_ticks);
        let TechniqueProgram::FireField { casters, .. } = &recipe.program else {
            panic!("fire spell")
        };
        for caster in casters {
            assert_eq!(caster.recovery_pose.is_some(), caster.character == 9);
        }
    }
    // All original enemies using the admitted native-cast family leave A6 absent.
    let enemy_archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let mut casters = Vec::new();
    for enemy in 0..251 {
        let start = word(&usual, table + enemy * 4).unwrap() as usize;
        let end = word(&usual, table + (enemy + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&enemy_archive[start..end]).unwrap();
        let rows =
            &bytes[usize::from(half(&bytes, 10).unwrap())..usize::from(half(&bytes, 12).unwrap())];
        if rows
            .chunks_exact(68)
            .any(|row| matches!(half(row, 0x40).unwrap(), 200 | 201 | 208 | 209))
        {
            casters.push(enemy);
            assert_eq!(bytes[usize::from(half(&bytes, 4).unwrap()) + 0xa6], 0);
        }
    }
    assert_eq!(
        casters,
        [
            75, 92, 156, 179, 182, 184, 195, 205, 207, 208, 210, 220, 221, 223, 233, 234, 242
        ]
    );
}
