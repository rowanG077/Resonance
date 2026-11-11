//! Keep validation and reading on the same file handle. In particular, Windows
//! canonicalize opens a handle itself; do not call it before opening for data.
use super::*;

pub(super) struct Opened {
    pub file: File,
    pub path: PathBuf,
    pub metadata: fs::Metadata,
}

impl Layer {
    pub(super) fn open(&self, logical: &str) -> Result<Option<Opened>> {
        #[cfg(not(windows))]
        let path = match self.path(logical)? {
            Some(path) => path,
            None => return Ok(None),
        };
        #[cfg(windows)]
        let path = self.root.join(logical);
        let file = match File::open(&path) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(error) if error.kind() == ErrorKind::NotFound => {
                ensure!(
                    !fs::symlink_metadata(&path).is_ok(),
                    "dangling content link: {}",
                    path.display()
                );
                return Ok(None);
            }
            Err(error) => return Err(error).with_context(|| path.display().to_string()),
        };
        #[cfg(windows)]
        let path = final_path(&file).with_context(|| format!("resolve open content {logical}"))?;
        ensure!(
            path.starts_with(&self.root),
            "content path escapes layer {}: {logical}",
            self.name
        );
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file(),
            "content resource is not a file in {}: {logical}",
            self.name
        );
        Ok(Some(Opened {
            file,
            path,
            metadata,
        }))
    }
}

#[cfg(windows)]
fn final_path(file: &File) -> std::io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };
    let mut buffer = vec![0u16; 256];
    loop {
        // SAFETY: the borrowed handle stays open, and the writable UTF-16
        // buffer has the declared capacity throughout this synchronous call.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len())
                    .map_err(|_| std::io::Error::other("content path too long"))?,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if (length as usize) < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(
                &buffer[..length as usize],
            )));
        }
        buffer.resize(length as usize + 1, 0);
    }
}
