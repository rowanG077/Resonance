//! Replace instructions while retaining source branches, events and message data.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use symphonia_script::{
    Program,
    scenario::{Analysis, Control},
};

pub(crate) fn rewrite(
    bytes: &[u8],
    analysis: &Analysis,
    replacements: BTreeMap<u32, Vec<u16>>,
) -> Result<Vec<u8>> {
    if replacements.is_empty() {
        return Ok(bytes.to_vec());
    }
    let header = analysis.header;
    let end = if header.auxiliary_words == 0 {
        bytes.len()
    } else {
        header.auxiliary_offset()
    };
    let source = bytes[header.code_base()..end].as_chunks::<2>().0;
    let mut words = Vec::new();
    let mut addresses = BTreeMap::new();
    let mut pc = 0;
    while pc < source.len() {
        addresses.insert(
            pc as u32,
            u16::try_from(words.len()).context("normalized script exceeds PC range")?,
        );
        if let Some(replacement) = replacements.get(&(pc as u32)) {
            let instruction = &analysis.instructions[&(pc as u32)];
            ensure!(
                matches!(instruction.control, Control::Fallthrough),
                "cannot replace a branch instruction"
            );
            words.extend_from_slice(replacement);
            pc += instruction.size_words as usize;
        } else {
            words.push(u16::from_be_bytes(source[pc]));
            pc += 1;
        }
    }
    for instruction in analysis
        .instructions
        .values()
        .filter(|i| matches!(i.control, Control::Jump | Control::Call | Control::Branch))
    {
        words[usize::from(addresses[&instruction.pc]) + 1] = *addresses
            .get(&(instruction.operands[0] as u32))
            .context("missing relocated script target")?;
    }
    let mut result = bytes[..header.code_base()].to_vec();
    result[2..4].copy_from_slice(&addresses[&u32::from(header.default_pc)].to_be_bytes());
    if header.auxiliary_words != 0 {
        result[4..6].copy_from_slice(
            &u16::try_from(usize::from(header.code_base_words) + words.len())
                .context("normalized script exceeds auxiliary range")?
                .to_be_bytes(),
        );
    }
    for record in analysis.records.iter().filter(|record| record.active()) {
        let pc = u32::from(addresses[&record.pc]);
        result[record.offset + 8..record.offset + 12].copy_from_slice(&pc.to_be_bytes());
    }
    result.extend(words.into_iter().flat_map(u16::to_be_bytes));
    if header.auxiliary_words != 0 {
        result.extend_from_slice(&bytes[end..]);
    }
    Program::decode(&result).context("normalized script")?;
    Ok(result)
}
