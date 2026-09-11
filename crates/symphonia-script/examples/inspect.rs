//! Inspect a scenario or SKP wrapper without running native services.
use std::{collections::BTreeSet, fs};
use symphonia_script::{message, scenario, semantics::NativeRegistry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected scenario/SKP path")?;
    let bytes = fs::read(&path)?;
    let bytes = if path.ends_with(".skp") {
        let at = u32::from_be_bytes(bytes.get(4..8).ok_or("short SKP")?.try_into()?) as usize;
        bytes.get(at..).ok_or("invalid SKP offset")?
    } else {
        &bytes
    };
    let (listing, analysis) = scenario::disassemble(bytes)?;
    let registry = NativeRegistry::gqseaf();
    let calls: BTreeSet<_> = analysis
        .instructions
        .values()
        .filter(|i| i.mnemonic == "proc")
        .map(|i| i.operands[0] as u8)
        .collect();
    for id in calls {
        eprintln!("{id:#04x}: {:?}", registry.get(id));
    }
    eprintln!(
        "messages: {:#?}",
        message::parse(&bytes[analysis.header.auxiliary_offset()..])
    );
    print!("{listing}");
    Ok(())
}
