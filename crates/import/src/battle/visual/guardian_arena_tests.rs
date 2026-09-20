use super::*;

#[test]
#[ignore = "requires original extracted US battle backgrounds; no texture cooking or rendering"]
fn original_guardian_arena_keeps_both_models_and_its_animation() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
    let backgrounds = fs::read(files.join("BTL/BTLbg.dat")).unwrap();
    let archive = MapArchive::decode(&backgrounds[arena_range(&rel, 71).unwrap()]).unwrap();
    assert_eq!(
        archive.source_sha256,
        "8bb420021b1cdd5e240563d1157008a624f56908d7d571ef4b6665ae999fec96"
    );
    assert_eq!(
        archive
            .sections
            .iter()
            .enumerate()
            .filter_map(|(index, section)| section.as_ref().map(|_| index))
            .collect::<Vec<_>>(),
        [0, 1, 3, 5]
    );
    assert_eq!(archive.section(0).unwrap().len(), 0x320);
    assert_eq!(archive.section(5).unwrap().len(), 192);
    for index in [1, 3] {
        crate::geometry::preflight_section(archive.section(index).unwrap()).unwrap();
    }
}
