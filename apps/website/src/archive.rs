//! ZIP integrity validation. Package contents are opaque to the website.
use crate::error::{Result, bad, fail};
use crate::model::UploadLimits;
use axum::http::StatusCode;
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Copy, Debug)]
pub struct Usage {
    pub expanded: u64,
    pub entries: usize,
}

/// Check the ZIP and upload limits without extracting or interpreting its files.
pub fn validate(mut reader: impl Read + Seek, limits: UploadLimits) -> Result<Usage> {
    limits.check_size(reader.seek(SeekFrom::End(0))?)?;
    reader.rewind()?;
    let mut zip = zip::ZipArchive::new(reader).map_err(|_| bad("Upload a valid ZIP archive."))?;
    if zip.len() > limits.entries() {
        return Err(bad(format!(
            "Your archive may contain at most {} ZIP entries.",
            limits.entries()
        )));
    }
    let mut usage = Usage {
        expanded: 0,
        entries: zip.len(),
    };
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|_| bad("Unsupported, encrypted or invalid ZIP entry."))?;
        let size = file.size();
        usage.expanded = usage
            .expanded
            .checked_add(size)
            .filter(|total| *total <= limits.expanded_bytes())
            .ok_or_else(|| {
                fail(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!(
                        "Your expanded file limit is {} MiB. Request a higher limit before uploading this archive.",
                        limits.expanded_mib
                    ),
                )
            })?;
        // Reading to EOF verifies decompression and CRCs. Bound the read even
        // when an entry understates its expanded size.
        let copied = std::io::copy(&mut (&mut file).take(size + 1), &mut std::io::sink())
            .map_err(|_| bad("The ZIP contains corrupt data."))?;
        if copied != size {
            return Err(bad("ZIP entry size does not match its data."));
        }
    }
    Ok(usage)
}
