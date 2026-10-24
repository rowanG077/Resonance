//! Independent cursors over one admitted file. Positional reads also isolate
//! separate decoders receiving cloned handles from the same content snapshot.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

#[derive(Clone)]
pub(crate) struct Source {
    file: Arc<File>,
    position: u64,
}

impl Source {
    pub fn new(file: File) -> io::Result<Self> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("movie input must be a regular local file"));
        }
        Ok(Self {
            file: Arc::new(file),
            position: 0,
        })
    }
}

impl Read for Source {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let count = {
            use std::os::unix::fs::FileExt;
            self.file.read_at(bytes, self.position)?
        };
        #[cfg(windows)]
        let count = {
            use std::os::windows::fs::FileExt;
            self.file.seek_read(bytes, self.position)?
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for Source {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.position = match from {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(offset) => self
                .position
                .checked_add_signed(offset)
                .ok_or_else(|| io::Error::other("invalid movie seek"))?,
            SeekFrom::End(offset) => self
                .file
                .metadata()?
                .len()
                .checked_add_signed(offset)
                .ok_or_else(|| io::Error::other("invalid movie seek"))?,
        };
        Ok(self.position)
    }
}
