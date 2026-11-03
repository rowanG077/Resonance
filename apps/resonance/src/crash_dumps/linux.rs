//! Compatibility with minidump-writer 0.13's Linux module reader.
//!
//! Nonadjacent PT_LOAD mappings (notably 64 KiB-aligned ARM64 executables
//! on 4 KiB kernels) become separate modules with incorrect image bases.
//! Its in-memory fallback ID also reads file offsets as virtual offsets.
//! Rebuild the module table from the saved maps and backing ELF files, in
//! the healthy monitor. Preserve every other stream and all saved CPU contexts.
//!
//! Upstream implementation: <https://docs.rs/crate/minidump-writer/0.13.0/source/src/linux/maps_reader.rs>
//! and its module_reader. Remove this workaround when the upstream writer passes
//! our split-PT_LOAD tests and stripped ARM64 source-resolution check unaided.
use anyhow::{Context, Result, ensure};
use minidump_common::format::{
    CvSignature, MINIDUMP_DIRECTORY, MINIDUMP_EXCEPTION_STREAM, MINIDUMP_HEADER,
    MINIDUMP_LOCATION_DESCRIPTOR, MINIDUMP_MODULE, MINIDUMP_STREAM_TYPE,
};
use object::{
    Endianness, elf,
    read::{
        ReadCache, ReadRef,
        elf::{FileHeader, ProgramHeader as _, SectionHeader as _},
    },
};
use procfs_core::{
    FromRead,
    process::{MMPermissions, MMapPath, MemoryMap, MemoryMaps},
};
use scroll::{LE, Pread, Pwrite, ctx::SizeWith};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Default)]
struct Load {
    offset: u64,
    address: u64,
    size: u64,
    executable: bool,
}

struct Image {
    loads: Vec<Load>,
    id: Vec<u8>,
}

/// Match the MODULE debug ID emitted by dump_syms, including ELF byte order.
pub(super) fn debug_id() -> Result<String> {
    let image = Image::read(Path::new("/proc/self/exe"))?;
    let mut guid = [0; 16];
    for (byte, value) in guid.iter_mut().zip(image.id) {
        *byte = value;
    }
    Ok(debugid::DebugId::from_guid_age(&guid, 0)?
        .breakpad()
        .to_string())
}

/// Save enough information to resolve the original instruction independently
/// of minidump/core success. This runs in the healthy monitor before either writer.
pub(super) fn write_fault_location(prefix: &Path, process: u32, pc: u64) -> Result<String> {
    let maps = fs::read_to_string(format!("/proc/{process}/maps"))?;
    let mut file = crate::logging::private_file(&prefix.with_file_name("memory-maps.txt"))?;
    file.write_all(maps.as_bytes())?;
    file.sync_all()?;
    let maps = MemoryMaps::from_read(maps.as_bytes())?;
    let Some(map) = maps
        .0
        .iter()
        .find(|map| map.address.0 <= pc && pc < map.address.1)
    else {
        return Ok("PC is outside mapped memory; see memory-maps.txt".into());
    };
    let MMapPath::Path(path) = &map.pathname else {
        return Ok(format!("PC is in {:?}; see memory-maps.txt", map.pathname));
    };
    // SAFETY: sysconf has no preconditions; this is the healthy monitor.
    let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })?;
    ensure!(page > 0, "invalid host page size");
    let image = Image::read(path)?;
    let (base, _) = image
        .range(map, page)
        .context("cannot resolve ELF load address")?;
    let relative = pc.checked_sub(base).context("PC precedes module")?;
    let address = relative
        + image
            .loads
            .iter()
            .map(|load| load.address)
            .min()
            .context("ELF has no load segments")?;
    let id: String = image.id.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "Instruction module: {}\nModule image base: {base:#x}\nModule-relative PC: {relative:#x}\nELF virtual address: {address:#x}\nModule build ID: {id}\nUse ELF virtual address with addr2line and the exact executable from this release.",
        path.display()
    ))
}

impl Image {
    fn read(path: &Path) -> Result<Self> {
        // The monitor is the same executable as the game. /proc/self/exe keeps
        // the loaded inode available even if the download was replaced later.
        let file = fs::File::open(if std::env::current_exe().ok().as_deref() == Some(path) {
            Path::new("/proc/self/exe")
        } else {
            path
        })?;
        Self::from_file(file)
    }

    fn from_file(file: impl object::read::ReadCacheOps) -> Result<Self> {
        // Sparse reads avoid copying the executable or its potentially huge
        // debug/symbol sections into the monitor after a low-memory crash.
        let cache = ReadCache::new(file);
        let header = elf::FileHeader64::<Endianness>::parse(&cache)?;
        let endian = header.endian()?;
        let programs = header.program_headers(endian, &cache)?;
        let loads = programs
            .iter()
            .filter(|p| p.p_type(endian) == elf::PT_LOAD)
            .map(|p| Load {
                offset: p.p_offset(endian),
                address: p.p_vaddr(endian),
                size: p.p_memsz(endian),
                executable: p.p_flags(endian).contains(elf::PF_X),
            })
            .collect();
        let mut id = None;
        for program in programs {
            if let Some(mut notes) = program.notes(endian, &cache)? {
                while let Some(note) = notes.next()? {
                    if note.name() == elf::ELF_NOTE_GNU
                        && note.n_type(endian) == elf::NT_GNU_BUILD_ID
                    {
                        id = Some(note.desc().to_vec());
                        break;
                    }
                }
            }
            if id.is_some() {
                break;
            }
        }
        let id = if let Some(id) = id {
            id
        } else {
            // Breakpad's fallback: XOR the first 4096 bytes of .text into
            // 16 bytes. Section offsets apply to the file, not virtual memory.
            let sections = header.sections(endian, &cache)?;
            let (_, text) = sections
                .section_by_name(endian, b".text")
                .context("ELF module has no build ID or .text section")?;
            let (offset, size) = text.file_range(endian).context(".text has no file data")?;
            let data = (&cache)
                .read_bytes_at(offset, size.min(4096))
                .ok()
                .context("cannot read ELF .text prefix")?;
            let mut id = vec![0; 16];
            for (index, byte) in data.iter().enumerate() {
                id[index % 16] ^= byte;
            }
            id
        };
        Ok(Self { loads, id })
    }

    fn range(&self, map: &MemoryMap, page: u64) -> Option<(u64, u32)> {
        let load = self.loads.iter().find(|load| {
            load.offset / page == map.offset / page
                && load.executable == map.perms.contains(MMPermissions::EXECUTE)
        })?;
        let bias = map
            .address
            .0
            .checked_add(load.offset)?
            .checked_sub(map.offset)?
            .checked_sub(load.address)?;
        let low = self.loads.iter().map(|load| load.address).min()?;
        let high = self.loads.iter().try_fold(0u64, |high, load| {
            Some(high.max(load.address.checked_add(load.size)?))
        })?;
        Some((
            bias.checked_add(low)?,
            high.checked_sub(low)?.try_into().ok()?,
        ))
    }
}

fn append(bytes: &mut Vec<u8>, data: &[u8]) -> Result<MINIDUMP_LOCATION_DESCRIPTOR> {
    // Minidump RVAs are 32-bit, even on 64-bit systems.
    let location = MINIDUMP_LOCATION_DESCRIPTOR {
        rva: bytes.len().try_into()?,
        data_size: data.len().try_into()?,
    };
    ensure!(
        bytes
            .len()
            .checked_add(data.len())
            .is_some_and(|end| end <= u32::MAX as usize),
        "minidump exceeds 4 GiB"
    );
    bytes.extend_from_slice(data);
    Ok(location)
}

fn normalize_bytes(bytes: &mut Vec<u8>, fault_address: u64) -> Result<()> {
    let header: MINIDUMP_HEADER = bytes.pread_with(0, LE)?;
    let mut streams = BTreeMap::new();
    for index in 0..header.stream_count as usize {
        let offset =
            header.stream_directory_rva as usize + index * MINIDUMP_DIRECTORY::size_with(&LE);
        let stream: MINIDUMP_DIRECTORY = bytes.pread_with(offset, LE)?;
        streams.insert(stream.stream_type, (offset, stream));
    }
    let (_, maps) = streams
        .get(&(MINIDUMP_STREAM_TYPE::LinuxMaps as u32))
        .context("dump has no Linux maps")?;
    let start = maps.location.rva as usize;
    let maps = MemoryMaps::from_read(
        bytes
            .get(start..start + maps.location.data_size as usize)
            .context("truncated maps")?,
    )?;
    let (directory, modules) = streams
        .get(&(MINIDUMP_STREAM_TYPE::ModuleListStream as u32))
        .context("dump has no modules")?;
    let mut offset = modules.location.rva as usize;
    let count: u32 = bytes.gread_with(&mut offset, LE)?;
    let mut images = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    // SAFETY: sysconf has no preconditions and is called in the monitor.
    let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })?;
    ensure!(page > 0, "invalid host page size");
    for _ in 0..count {
        let mut module: MINIDUMP_MODULE = bytes.gread_with(&mut offset, LE)?;
        if let Some(map) = maps
            .0
            .iter()
            .find(|map| map.address.0 == module.base_of_image)
            && let MMapPath::Path(path) = &map.pathname
            && let Some(image) = images
                .entry(path.clone())
                .or_insert_with(|| Image::read(path).ok())
            && let Some((base, size)) = image.range(map, page)
        {
            if !seen.insert((path.clone(), base)) {
                continue;
            }
            module.base_of_image = base;
            module.size_of_image = size;
            let mut cv = (CvSignature::Elf as u32).to_le_bytes().to_vec();
            cv.extend_from_slice(&image.id);
            module.cv_record = append(bytes, &cv)?;
        }
        records.push(module);
    }
    let mut table = vec![0; 4 + records.len() * MINIDUMP_MODULE::size_with(&LE)];
    let mut offset = 0;
    table.gwrite_with(u32::try_from(records.len())?, &mut offset, LE)?;
    for module in records {
        table.gwrite_with(module, &mut offset, LE)?;
    }
    let mut modules = modules.clone();
    modules.location = append(bytes, &table)?;
    bytes.pwrite_with(modules, *directory, LE)?;
    // crash-handler stores raw siginfo_t in signalfd_siginfo storage. Its
    // union offsets differ, so use the si_addr captured by our callback.
    if let Some((_, stream)) = streams.get(&(MINIDUMP_STREAM_TYPE::ExceptionStream as u32)) {
        let offset = stream.location.rva as usize;
        let mut exception: MINIDUMP_EXCEPTION_STREAM = bytes.pread_with(offset, LE)?;
        exception.exception_record.exception_address = fault_address;
        bytes.pwrite_with(exception, offset, LE)?;
    }
    Ok(())
}

pub(super) fn normalize(dump: &mut minidumper::MinidumpBinary, fault_address: u64) -> Result<()> {
    let bytes = dump
        .contents
        .as_mut()
        .context("Linux minidump has no contents")?;
    normalize_bytes(bytes, fault_address).context("normalizing Linux minidump modules")?;
    dump.file.seek(SeekFrom::Start(0))?;
    dump.file.write_all(bytes)?;
    dump.file.set_len(bytes.len() as u64)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_a_large_executable_without_reading_its_debug_sections() {
        use std::{
            cell::Cell,
            io::{Read, Seek, SeekFrom},
        };
        struct Counted<'a> {
            file: fs::File,
            bytes: &'a Cell<usize>,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let read = self.file.read(buffer)?;
                self.bytes.set(self.bytes.get() + read);
                Ok(read)
            }
        }
        impl Seek for Counted<'_> {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.file.seek(position)
            }
        }
        let file = fs::File::open(std::env::current_exe().unwrap()).unwrap();
        assert!(file.metadata().unwrap().len() > 1024 * 1024);
        let bytes = Cell::new(0);
        let image = Image::from_file(Counted {
            file,
            bytes: &bytes,
        })
        .unwrap();
        assert!(!image.loads.is_empty() && !image.id.is_empty());
        assert!(
            bytes.get() < 128 * 1024,
            "read {} bytes of the executable",
            bytes.get()
        );
    }

    #[test]
    fn separate_load_segments_share_the_original_image_base() {
        let image = Image {
            loads: vec![
                Load {
                    size: 0xc50958,
                    ..Default::default()
                },
                Load {
                    offset: 0xc50958,
                    address: 0xc60958,
                    size: 0x1033158,
                    executable: true,
                },
            ],
            id: vec![],
        };
        let maps = MemoryMaps::from_read(&b"555555550000-5555561a4000 r--p 00000000 00:00 1 /game\n5555561b0000-5555571e4000 r-xp 00c50000 00:00 1 /game\n"[..]).unwrap();
        let first = image.range(&maps.0[0], 4096).unwrap();
        let text = image.range(&maps.0[1], 4096).unwrap();
        assert_eq!(text, first);
        assert_eq!(text.0, 0x555555550000);
        assert!(u64::from(text.1) > 0x1b95ef4);
    }

    #[test]
    fn distinguish_segments_whose_file_offsets_share_a_page() {
        let image = Image {
            loads: vec![
                Load {
                    size: 0x958,
                    ..Default::default()
                },
                Load {
                    offset: 0x958,
                    address: 0x10958,
                    size: 0x400,
                    executable: true,
                },
            ],
            id: vec![],
        };
        let maps =
            MemoryMaps::from_read(&b"555555560000-555555561000 r-xp 00000000 00:00 1 /game\n"[..])
                .unwrap();
        assert_eq!(image.range(&maps.0[0], 4096).unwrap().0, 0x555555550000);
    }
}
