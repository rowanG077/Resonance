//! Windows named-pipe transport. Proton does not implement Winsock AF_UNIX.
//! Only fixed-size context messages cross the pipe; the healthy monitor owns
//! dump creation, and acknowledges completion before the faulting thread exits.
use anyhow::{Context, ensure};
use minidumper::ServerHandler as _;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};

/// Read the linked executable's CodeView record without loading its debug data.
fn executable_codeview() -> anyhow::Result<Vec<u8>> {
    use object::{
        Object,
        read::{ReadCache, pe::PeFile64},
    };
    let cache = ReadCache::new(File::open(std::env::current_exe()?)?);
    let pe = PeFile64::parse(&cache)?;
    let pdb = pe.pdb_info()?.context("executable has no PDB identity")?;
    let filename = pdb
        .path()
        .rsplit(|byte| matches!(byte, b'/' | b'\\'))
        .next()
        .context("missing PDB filename")?;
    ensure!(
        !filename.is_empty() && filename.len() <= 16384,
        "invalid PDB filename"
    );
    let mut cv = b"RSDS".to_vec();
    cv.extend_from_slice(&pdb.guid());
    cv.extend_from_slice(&pdb.age().to_le_bytes());
    cv.extend_from_slice(filename);
    cv.push(0);
    Ok(cv)
}

pub(super) fn debug_id() -> anyhow::Result<String> {
    let cv = executable_codeview()?;
    let age = u32::from_le_bytes(cv[20..24].try_into()?);
    Ok(debugid::DebugId::from_guid_age(&cv[4..20], age)?
        .breakpad()
        .to_string())
}

/// Wine's MiniDumpWriteDump can omit CodeView records. Recover only our own
/// executable's missing record from the exact PE used by this monitor, so the
/// retained PDB/portable symbols can be matched without shipping a PDB to users.
/// Remove when Wine/Proton preserves the PE's CodeView identity in both the
/// access-violation and stack-overflow checks without normalization.
pub(super) fn normalize(dump: &mut minidumper::MinidumpBinary) -> anyhow::Result<bool> {
    use minidump_common::format::{
        MINIDUMP_DIRECTORY, MINIDUMP_HEADER, MINIDUMP_LOCATION_DESCRIPTOR, MINIDUMP_MODULE,
        MINIDUMP_STREAM_TYPE,
    };
    use scroll::{LE, Pread, Pwrite, ctx::SizeWith};
    use std::io::{Seek, SeekFrom};

    let mut bytes = std::fs::read(&dump.path)?;
    let header: MINIDUMP_HEADER = bytes.pread_with(0, LE)?;
    ensure!(header.stream_count <= 4096, "invalid dump stream count");
    let executable = std::env::current_exe()?;
    for index in 0..header.stream_count as usize {
        let stream: MINIDUMP_DIRECTORY = bytes.pread_with(
            header.stream_directory_rva as usize + index * MINIDUMP_DIRECTORY::size_with(&LE),
            LE,
        )?;
        if stream.stream_type != MINIDUMP_STREAM_TYPE::ModuleListStream as u32 {
            continue;
        }
        let count: u32 = bytes.pread_with(stream.location.rva as usize, LE)?;
        ensure!(count <= 4096, "invalid dump module count");
        for index in 0..count as usize {
            let offset = stream.location.rva as usize + 4 + index * MINIDUMP_MODULE::size_with(&LE);
            let mut module: MINIDUMP_MODULE = bytes.pread_with(offset, LE)?;
            if module.cv_record.data_size != 0 {
                continue;
            }
            let name_offset = module.module_name_rva as usize;
            let length: u32 = bytes.pread_with(name_offset, LE)?;
            ensure!(
                length <= 65536 && length.is_multiple_of(2),
                "invalid module name length"
            );
            let name = bytes
                .get(name_offset + 4..name_offset + 4 + length as usize)
                .context("module name outside dump")?;
            let name = String::from_utf16(
                &name
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            if Path::new(&name).file_name() != executable.file_name() {
                continue;
            }
            let cv = executable_codeview()?;
            module.cv_record = MINIDUMP_LOCATION_DESCRIPTOR {
                data_size: cv.len().try_into()?,
                rva: bytes.len().try_into()?,
            };
            bytes.extend_from_slice(&cv);
            bytes.pwrite_with(module, offset, LE)?;
            dump.file.seek(SeekFrom::Start(0))?;
            dump.file.write_all(&bytes)?;
            dump.file.set_len(bytes.len() as u64)?;
            return Ok(true);
        }
    }
    Ok(false)
}

type Handle = *mut std::ffi::c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        mode: u32,
        kind: u32,
        instances: u32,
        output: u32,
        input: u32,
        timeout: u32,
        security: *const std::ffi::c_void,
    ) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut std::ffi::c_void) -> i32;
}

fn name(socket: &Path) -> io::Result<std::ffi::OsString> {
    let directory = socket.parent().and_then(Path::file_name).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "missing private pipe identity")
    })?;
    let mut name = std::ffi::OsString::from(r"\\.\pipe\resonance-");
    name.push(directory);
    Ok(name)
}

pub struct Client(File);
impl Client {
    pub fn connect(socket: &Path) -> io::Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(name(socket)?)
            .map(Self)
    }

    pub fn send_message(&self, _kind: u32, bytes: [u8; 64]) -> io::Result<()> {
        (&self.0).write_all(&bytes)
    }

    pub fn request_dump(&self, context: &crash_handler::CrashContext) -> io::Result<()> {
        let values = [
            context.exception_pointers as u64,
            context.exception_code as u32 as u64,
            context.process_id as u64,
            context.thread_id as u64,
        ];
        let mut bytes = [0u8; 32];
        for (chunk, value) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(values) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        (&self.0).write_all(&bytes)?;
        let mut ack = [0];
        (&self.0).read_exact(&mut ack)?;
        if ack == [1] {
            Ok(())
        } else {
            Err(io::Error::other("crash monitor could not write dump"))
        }
    }
}

pub struct Server(File);
impl Server {
    pub fn bind(socket: &Path) -> io::Result<Self> {
        let name: Vec<_> = name(socket)?.encode_wide().chain(Some(0)).collect();
        // SAFETY: A terminated UTF-16 name and default user security descriptor.
        // First-instance and local-only flags prevent pipe replacement or remote
        // clients. The owning File closes the pipe on every exit path.
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                3 | 0x0008_0000,
                0x8,
                1,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        if pipe == -1isize as Handle {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW returned a new, valid owning handle.
        Ok(Self(unsafe { File::from_raw_handle(pipe) }))
    }

    pub fn run(&mut self, handler: super::native::Monitor) -> Result<(), minidumper::Error> {
        // SAFETY: This is our synchronous pipe handle. The monitor watchdog
        // closes the process if connect or dump writing stalls.
        if unsafe { ConnectNamedPipe(self.0.as_raw_handle(), std::ptr::null_mut()) } == 0
            && io::Error::last_os_error().raw_os_error() != Some(535)
        {
            // ERROR_PIPE_CONNECTED
            return Err(io::Error::last_os_error().into());
        }
        handler.on_client_connected(1);
        let mut metadata = [0; 64];
        if let Err(error) = self.0.read_exact(&mut metadata) {
            // A normal exit closes the connected pipe without a crash request.
            return if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
            ) {
                Ok(())
            } else {
                Err(error.into())
            };
        }
        handler.on_message(0, metadata.to_vec());
        let mut context = [0; 32];
        self.0.read_exact(&mut context)?;
        let chunks = context.as_chunks::<8>().0;
        let values: [u64; 4] = std::array::from_fn(|index| u64::from_le_bytes(chunks[index]));
        let context = crash_handler::CrashContext {
            exception_pointers: values[0] as *const _,
            exception_code: values[1] as i32,
            process_id: values[2] as u32,
            thread_id: values[3] as u32,
        };
        let (mut file, path) = handler.create_minidump_file()?;
        // The crashing thread remains blocked on the acknowledgement, keeping
        // its exception pointers valid in the target process until dumping ends.
        let result = minidump_writer::minidump_writer::MinidumpWriter::dump_crash_context(
            &context, None, &mut file,
        )
        .map(|()| minidumper::MinidumpBinary {
            file,
            path,
            contents: None,
        })
        .map_err(minidumper::Error::from);
        let success = handler.complete_dump(result);
        self.0.write_all(&[u8::from(success)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn restores_missing_pdb_identity_and_preserves_existing_records() {
        use minidump_common::format::*;
        use scroll::{LE, Pread, Pwrite};
        use std::{io::Write, os::windows::ffi::OsStrExt};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("crash.dmp");
        let mut bytes = vec![0u8; 512];
        bytes
            .pwrite_with(
                MINIDUMP_HEADER {
                    signature: MINIDUMP_SIGNATURE,
                    version: MINIDUMP_VERSION,
                    stream_count: 1,
                    stream_directory_rva: 32,
                    checksum: 0,
                    time_date_stamp: 0,
                    flags: 0,
                },
                0,
                LE,
            )
            .unwrap();
        bytes
            .pwrite_with(
                MINIDUMP_DIRECTORY {
                    stream_type: MINIDUMP_STREAM_TYPE::ModuleListStream as u32,
                    location: MINIDUMP_LOCATION_DESCRIPTOR {
                        data_size: 112,
                        rva: 44,
                    },
                },
                32,
                LE,
            )
            .unwrap();
        bytes.pwrite_with(1u32, 44, LE).unwrap();
        let mut module = MINIDUMP_MODULE {
            base_of_image: 0x140000000,
            size_of_image: 0x1234,
            module_name_rva: 160,
            ..Default::default()
        };
        bytes.pwrite_with(module.clone(), 48, LE).unwrap();
        let executable = std::env::current_exe().unwrap();
        let name: Vec<u8> = executable
            .file_name()
            .unwrap()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.pwrite_with(name.len() as u32, 160, LE).unwrap();
        bytes[164..164 + name.len()].copy_from_slice(&name);
        let mut dump = minidumper::MinidumpBinary {
            file: crate::logging::private_file(&path).unwrap(),
            path,
            contents: None,
        };
        dump.file.write_all(&bytes).unwrap();
        assert!(super::normalize(&mut dump).unwrap());
        let repaired = std::fs::read(&dump.path).unwrap();
        let actual: MINIDUMP_MODULE = repaired.pread_with(48, LE).unwrap();
        let offset = actual.cv_record.rva as usize;
        assert_eq!(&repaired[offset..offset + 4], b"RSDS");
        assert!(repaired[offset + 24..].ends_with(b".pdb\0"));
        module.cv_record = actual.cv_record;
        bytes.pwrite_with(module, 48, LE).unwrap();
        assert_eq!(&repaired[..bytes.len()], bytes);
        assert!(!super::normalize(&mut dump).unwrap());
        assert_eq!(std::fs::read(&dump.path).unwrap(), repaired);
    }
}
