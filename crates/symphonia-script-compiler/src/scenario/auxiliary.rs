//! Readable auxiliary text and the byte-offset tables that reference it.
use super::{Context, Definitions, MessageIdentityUse};
use crate::Diagnostic;
use std::collections::{BTreeMap, BTreeSet};
use symphonia_script::{message, scenario::Instruction};

type References = BTreeMap<usize, (usize, String)>;

#[derive(Default)]
pub(super) struct Plan {
    pub data: References,
    pub imports: BTreeSet<String>,
    pub identities: Vec<MessageIdentityUse>,
    pub controls: usize,
    pub promoted_controls: usize,
}

pub(super) fn prepare(
    bytes: &[u8],
    auxiliary: usize,
    code: &BTreeMap<usize, &Instruction>,
    occupied: &BTreeSet<usize>,
    labels: &mut BTreeMap<usize, String>,
    references: &mut References,
    source: (&Definitions, Context),
) -> Result<Plan, Diagnostic> {
    let (definitions, context) = source;
    let mut plan = Plan::default();
    let Some(region) = bytes.get(auxiliary..) else {
        return Ok(plan);
    };
    let Ok(Some(table)) = message::offset_table(region) else {
        return Ok(plan);
    };
    let table_end = auxiliary + table.entry_bytes * table.offsets.len();
    // Don't reinterpret executable bytes or another declared structure as a table.
    if occupied.range(auxiliary..table_end).next().is_some()
        || labels.range(auxiliary + 1..table_end).next().is_some()
        || references
            .iter()
            .any(|(&start, (size, _))| start < table_end && start + size > auxiliary)
        || table
            .offsets
            .iter()
            .any(|&target| occupied.contains(&(auxiliary + target)))
    {
        return Ok(plan);
    }
    let Some(base) = labels.get(&auxiliary).cloned() else {
        return Ok(plan);
    };
    for (index, &target) in table.offsets.iter().enumerate() {
        let label = labels
            .entry(auxiliary + target)
            .or_insert_with(|| format!("aux_{index:04X}"));
        references.insert(
            auxiliary + index * table.entry_bytes,
            (
                table.entry_bytes,
                format!("relative{}({label}, {base});", table.entry_bytes * 8),
            ),
        );
    }
    let boundaries: BTreeSet<_> = labels
        .keys()
        .chain(references.keys())
        .chain(code.keys())
        .copied()
        .collect();
    let ranges = message::source_ranges(region, &table);
    for range in ranges.text {
        let mut start = auxiliary + range.start;
        let end = auxiliary + range.end;
        // Keep literal formatting controls visible as bytes, between text runs.
        let cuts: BTreeSet<_> = boundaries
            .range(start + 1..end)
            .copied()
            .chain((start..end).filter(|&at| {
                bytes[at].is_ascii_control() && !matches!(bytes[at], b'\n' | b'\r' | b'\t')
            }))
            .chain([end])
            .collect();
        for stop in cuts {
            if start < stop && occupied.range(start..stop).next().is_none() {
                let raw = &bytes[start..stop];
                let (decoded, _, errors) = encoding_rs::SHIFT_JIS.decode(raw);
                let (encoded, _, unrepresentable) = encoding_rs::SHIFT_JIS.encode(&decoded);
                if !errors && !unrepresentable && encoded.as_ref() == raw {
                    plan.data.insert(
                        start,
                        (
                            stop - start,
                            format!("sjis({});", crate::format::quote(&decoded)),
                        ),
                    );
                }
            }
            start = if stop < end
                && bytes[stop].is_ascii_control()
                && !matches!(bytes[stop], b'\n' | b'\r' | b'\t')
            {
                stop + 1
            } else {
                stop
            };
        }
    }
    let mut cache = BTreeMap::new();
    for range in ranges.controls {
        plan.controls += 1;
        let start = auxiliary + range.bytes.start;
        let end = auxiliary + range.bytes.end;
        if occupied.range(start..end).next().is_some()
            || boundaries.range(start + 1..end).next().is_some()
        {
            continue;
        }
        definitions.require("scenario::MessageControl")?;
        let key = (range.opcode, bytes[start + 1..end].to_vec());
        if !cache.contains_key(&key) {
            cache.insert(
                key.clone(),
                super::decode::message_expression(&key.1, range.opcode, definitions, context)?,
            );
        }
        let Some(expression) = &cache[&key] else {
            continue;
        };
        plan.promoted_controls += 1;
        plan.imports.extend(expression.imports.iter().cloned());
        let control = if let Some(name) =
            definitions.format("scenario::MessageControl", i32::from(range.opcode))
        {
            plan.imports.insert("scenario".into());
            name
        } else {
            range.opcode.to_string()
        };
        plan.data.insert(
            start,
            (
                end - start,
                format!("message_control({control}, {});", expression.text),
            ),
        );
        plan.identities
            .extend(expression.identities.iter().map(|id| MessageIdentityUse {
                byte_offset: start + 1 + id.pc as usize * 2,
                control: range.opcode,
                native: id.native.clone(),
                parameter: id.parameter.clone(),
                domain: id.domain.clone(),
                value: id.value,
                symbolic: id.symbolic,
                partial_mask: id.partial_mask,
            }));
    }
    Ok(plan)
}
