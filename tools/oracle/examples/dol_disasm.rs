//! Read-only instruction inspection. Writes assembly text, never cooked assets.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CStr, c_char, c_void},
    fs,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 4, "DOL LIBLLVM ADDRESS SIZE");
    let dol = fs::read(&args[0])?;
    let address = u32::from_str_radix(args[2].trim_start_matches("0x"), 16)?;
    let size = usize::from_str_radix(args[3].trim_start_matches("0x"), 16)?;
    let word = |at| u32::from_be_bytes(dol[at..at + 4].try_into().unwrap());
    let offset = (0..18)
        .find_map(|i| {
            let start = word(0x48 + i * 4);
            let length = word(0x90 + i * 4);
            (address >= start && address.checked_add(size as u32)? <= start.checked_add(length)?)
                .then(|| (word(i * 4) + address - start) as usize)
        })
        .context("instruction range outside DOL sections")?;
    let bytes = dol
        .get(offset..offset + size)
        .context("truncated instruction range")?;
    unsafe {
        let llvm = libloading::Library::new(&args[1])?;
        for suffix in ["TargetInfo", "Target", "TargetMC", "Disassembler"] {
            let init: libloading::Symbol<unsafe extern "C" fn()> =
                llvm.get(format!("LLVMInitializePowerPC{suffix}").as_bytes())?;
            init();
        }
        type Create = unsafe extern "C" fn(
            *const c_char,
            *mut c_void,
            i32,
            *mut c_void,
            *mut c_void,
        ) -> *mut c_void;
        type Decode =
            unsafe extern "C" fn(*mut c_void, *const u8, u64, u64, *mut c_char, usize) -> usize;
        let create: libloading::Symbol<Create> = llvm.get(b"LLVMCreateDisasm")?;
        let decode: libloading::Symbol<Decode> = llvm.get(b"LLVMDisasmInstruction")?;
        let dispose: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            llvm.get(b"LLVMDisasmDispose")?;
        let context = create(
            c"powerpc-unknown-eabi".as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        ensure!(!context.is_null(), "LLVM PowerPC disassembler unavailable");
        for (i, instruction) in bytes.chunks_exact(4).enumerate() {
            let pc = address + i as u32 * 4;
            let mut output = [0 as c_char; 256];
            let decoded = decode(
                context,
                instruction.as_ptr(),
                4,
                pc.into(),
                output.as_mut_ptr(),
                output.len(),
            );
            let opcode = u32::from_be_bytes(instruction.try_into()?);
            if decoded == 0 {
                println!("{pc:08x}: .long 0x{opcode:08x}");
            } else {
                println!(
                    "{pc:08x}: {}",
                    CStr::from_ptr(output.as_ptr()).to_string_lossy()
                );
            }
        }
        dispose(context);
    }
    Ok(())
}
