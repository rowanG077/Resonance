use super::*;
use crate::texture::{ColorSpace, Definition, Mips, VERSION};
use resonance_asset_writer::texture::Pixels;

#[test]
fn mip_dependencies_follow_mods_and_stay_with_the_preparation() -> Result<()> {
    let fixture = Fixture::new();
    let path = "meshes/sign/wood.texture.json";
    let definition = Definition {
        version: VERSION,
        image: "meshes/sign/wood.png".into(),
        color_space: ColorSpace::Linear,
        mips: Mips::Authored {
            levels: vec!["meshes/sign/wood-mip-1.png".into()],
        },
    };
    fixture.write(&format!("base/{path}"), &serde_json::to_vec(&definition)?);
    let base = Pixels {
        width: 2,
        height: 2,
        rgba: vec![17; 16],
    };
    let original = Pixels {
        width: 1,
        height: 1,
        rgba: vec![7, 13, 31, 255],
    };
    fixture.write("base/meshes/sign/wood.png", &base.png()?);
    fixture.write("base/meshes/sign/wood-mip-1.png", &original.png()?);
    let replacement = Pixels {
        width: 1,
        height: 1,
        rgba: vec![3, 61, 97, 127],
    };
    fixture.write("mod/meshes/sign/wood-mip-1.png", &replacement.png()?);
    let library = fixture.open(&["mod"])?;
    let mut retained = library.snapshot();
    let captured: Definition = retained.json(path, 4096)?;
    assert!(retained.digest(&definition.image).is_ok());
    assert!(retained.digest("meshes/sign/wood-mip-1.png").is_ok());
    let decode = |snapshot: &mut Snapshot| captured.decode(|image| snapshot.read(image, 4096));
    assert_eq!(decode(&mut retained)?, [base.clone(), replacement]);
    let changed = Pixels {
        width: 1,
        height: 1,
        rgba: vec![241, 29, 73, 0],
    };
    fixture.write("mod/meshes/sign/wood-mip-1.png", &changed.png()?);
    assert_eq!(decode(&mut library.snapshot())?, [base.clone(), changed]);
    assert_ne!(decode(&mut retained)?, decode(&mut library.snapshot())?);
    assert_eq!(
        decode(&mut fixture.open(&[])?.snapshot())?,
        [base, original]
    );

    let files = crate::prepared::Files::from_snapshot(
        retained,
        &mut Default::default(),
        || false,
        crate::diagnostics::Diagnostics::new(true),
    )?;
    fs::remove_file(fixture.0.join("mod/meshes/sign/wood-mip-1.png"))?;
    fs::remove_file(fixture.0.join("base/meshes/sign/wood-mip-1.png"))?;
    assert_eq!(
        captured.decode(|image| files.read(image))?[1].rgba,
        [3, 61, 97, 127]
    );
    assert!(library.snapshot().read(path, 4096).is_err());
    Ok(())
}

#[test]
fn generated_policy_needs_only_the_base_and_rejects_invalid_authored_levels() -> Result<()> {
    let base = Pixels {
        width: 4,
        height: 2,
        rgba: vec![51; 32],
    }
    .png()?;
    let mut definition = Definition {
        version: VERSION,
        image: "wood.png".into(),
        color_space: ColorSpace::Srgb,
        mips: Mips::Generate,
    };
    let mut reads = Vec::new();
    let levels = definition.decode(|path| {
        reads.push(path.to_owned());
        Ok(base.as_slice())
    })?;
    assert_eq!(reads, ["wood.png"]);
    assert_eq!(
        levels
            .iter()
            .map(|level| (level.width, level.height))
            .collect::<Vec<_>>(),
        [(4, 2), (2, 1), (1, 1)]
    );
    definition.mips = Mips::Authored {
        levels: vec!["wrong.png".into()],
    };
    assert!(definition.decode(|_| Ok(base.as_slice())).is_err());
    definition.image = "../wood.png".into();
    assert!(definition.validate().is_err());
    Ok(())
}
