use std::{fs, path::Path, process::Command};

fn import(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_resonance-import"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn previous_cook(root: &Path) {
    fs::create_dir_all(root.join("local/extracted-discs/disc1/sys")).unwrap();
    fs::create_dir_all(root.join("local/all-assets")).unwrap();
    fs::write(
        root.join("local/extracted-discs/disc1/sys/main.dol"),
        b"extracted source fixture",
    )
    .unwrap();
    fs::write(root.join("local/all-assets/obsolete.ktx2"), b"old texture").unwrap();
    fs::write(root.join("local/disc1.rvz"), b"original disc fixture").unwrap();
}

#[test]
fn default_cook_reuses_extraction_and_clears_only_the_asset_library() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    previous_cook(root);

    let result = import(root, &[]);

    // The deliberately incomplete executable stops conversion after cleanup.
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("game and interface"));
    assert!(!root.join("local/all-assets/obsolete.ktx2").exists());
    assert_eq!(
        fs::read(root.join("local/extracted-discs/disc1/sys/main.dol")).unwrap(),
        b"extracted source fixture"
    );
    assert_eq!(
        fs::read(root.join("local/disc1.rvz")).unwrap(),
        b"original disc fixture"
    );
}

#[test]
fn supplying_both_discs_replaces_extraction_before_cooking() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    previous_cook(root);
    fs::write(root.join("local/disc2.rvz"), b"original disc 2 fixture").unwrap();

    let result = import(root, &["local/disc1.rvz", "local/disc2.rvz"]);

    // Invalid image fixtures stop extraction, without falling back to old files.
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("open disc image"));
    assert!(!root.join("local/extracted-discs").exists());
    assert_eq!(
        fs::read(root.join("local/all-assets/obsolete.ktx2")).unwrap(),
        b"old texture"
    );
    assert_eq!(
        fs::read(root.join("local/disc1.rvz")).unwrap(),
        b"original disc fixture"
    );
    assert_eq!(
        fs::read(root.join("local/disc2.rvz")).unwrap(),
        b"original disc 2 fixture"
    );
}

#[test]
fn disc_images_inside_extraction_are_rejected_before_cleanup() {
    for inside in 0..2 {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        previous_cook(root);
        fs::write(root.join("local/disc2.rvz"), b"original disc 2 fixture").unwrap();
        let mut discs = ["local/disc1.rvz", "local/disc2.rvz"];
        let original = fs::read(root.join(discs[inside])).unwrap();
        discs[inside] = "local/extracted-discs/original.rvz";
        fs::write(root.join(discs[inside]), &original).unwrap();

        let result = import(root, &discs);

        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("must be outside extraction directory")
        );
        assert_eq!(fs::read(root.join(discs[inside])).unwrap(), original);
        assert_eq!(
            fs::read(root.join("local/extracted-discs/disc1/sys/main.dol")).unwrap(),
            b"extracted source fixture"
        );
        assert_eq!(
            fs::read(root.join("local/all-assets/obsolete.ktx2")).unwrap(),
            b"old texture"
        );
    }
}

#[test]
fn missing_disc_image_leaves_extraction_and_assets_alone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    previous_cook(root);

    let result = import(root, &["local/disc1.rvz", "local/missing-disc2.rvz"]);

    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("open disc image local/missing-disc2.rvz")
    );
    assert!(
        root.join("local/extracted-discs/disc1/sys/main.dol")
            .is_file()
    );
    assert!(root.join("local/all-assets/obsolete.ktx2").is_file());
}

#[test]
fn supplying_only_one_disc_leaves_extraction_and_assets_alone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    previous_cook(root);

    let result = import(root, &["local/disc1.rvz"]);

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("<DISC_2>"));
    assert!(
        root.join("local/extracted-discs/disc1/sys/main.dol")
            .is_file()
    );
    assert!(root.join("local/all-assets/obsolete.ktx2").is_file());
}

#[test]
fn missing_extraction_leaves_the_previous_asset_library_alone() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("local/all-assets")).unwrap();
    fs::write(root.join("local/all-assets/obsolete.ktx2"), b"old texture").unwrap();

    let result = import(root, &[]);

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing extracted GameCube input"));
    assert!(root.join("local/all-assets/obsolete.ktx2").is_file());
}
