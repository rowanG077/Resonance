use crate::{archive::validate, model::*};
use std::io::{Cursor, Write};

fn zip(files: &[(&str, &[u8])], options: zip::write::SimpleFileOptions) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn zip_contents_have_no_format_or_layout_requirements() {
    let files: &[(&str, &[u8])] = &[
        ("package/content.json", b"not JSON"),
        ("notes.txt", b"arbitrary documentation"),
        ("assets/unknown.bin", &[0, 255, 1]),
        ("scripts/scenario.ssb", &[0; 20]),
    ];
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let data = zip(
            files,
            zip::write::SimpleFileOptions::default().compression_method(method),
        );
        let usage = validate(Cursor::new(data), UploadLimits::default()).unwrap();
        assert_eq!(usage.entries, files.len());
        assert_eq!(
            usage.expanded,
            files.iter().map(|(_, data)| data.len() as u64).sum::<u64>()
        );
    }
    let empty = zip(&[], zip::write::SimpleFileOptions::default());
    assert_eq!(
        validate(Cursor::new(empty), UploadLimits::default())
            .unwrap()
            .entries,
        0
    );
}

#[test]
fn zip_validation_rejects_invalid_and_corrupt_archives() {
    let original = zip(
        &[("file.bin", b"unique payload")],
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    );
    let mut corrupt = original.clone();
    let at = corrupt
        .windows(14)
        .position(|bytes| bytes == b"unique payload")
        .unwrap();
    corrupt[at] ^= 1;
    for bytes in [
        b"not a zip".as_slice(),
        &original[..original.len() - 1],
        &corrupt,
    ] {
        assert!(validate(Cursor::new(bytes), UploadLimits::default()).is_err());
    }
}

#[test]
fn zip64_and_streamed_archives_are_supported() {
    let data = zip(
        &[("file.bin", b"payload")],
        zip::write::SimpleFileOptions::default().large_file(true),
    );
    assert_eq!(
        validate(Cursor::new(data), UploadLimits::default())
            .unwrap()
            .expanded,
        7
    );
    let mut data = Vec::new();
    let mut writer = zip::ZipWriter::new_stream(&mut data);
    writer
        .start_file("file.bin", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"payload").unwrap();
    writer.finish().unwrap();
    assert_eq!(
        validate(Cursor::new(data), UploadLimits::default())
            .unwrap()
            .expanded,
        7
    );
}

#[test]
fn upload_limits_and_part_sizes_keep_large_sizes_in_u64() {
    let limits = UploadLimits {
        zip_mib: 16384,
        expanded_mib: 65536,
    }
    .validate()
    .unwrap();
    assert_eq!(limits.zip_bytes(), 16 * 1024 * 1024 * 1024u64);
    assert_eq!(
        part_length(limits.zip_bytes(), 2048).unwrap(),
        PART_BYTES as usize
    );
    assert!(part_length(limits.zip_bytes(), 2049).is_err());
    assert!(part_length(1, 0).is_err());
    assert_eq!(part_length(PART_BYTES + 3, 2).unwrap(), 3);
    assert!(newer("1.0.0+new", "1.0.0").is_err());
    assert!(newer("1.0.1", "1.0.0").is_ok());
}
