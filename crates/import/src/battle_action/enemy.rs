//! Enemy package tables consumed by 298C0/3DA00. These are source
//! declarations; the cooker never turns their control flow into authored code.
use super::*;

fn section(bytes: &[u8], field: usize) -> Result<(usize, &[u8])> {
    let start = usize::from(u16::read(bytes, field)?);
    ensure!(start >= 488, "invalid enemy action section");
    let mut end = bytes.len();
    for offset in (4..20)
        .step_by(2)
        .map(|at| u16::read(bytes, at).map(usize::from))
        .chain(
            (24..488)
                .step_by(4)
                .map(|at| u32::read(bytes, at).map(|v| v as usize)),
        )
    {
        let offset = offset?;
        if offset > start {
            end = end.min(offset);
        }
    }
    Ok((
        start,
        bytes
            .get(start..end)
            .context("enemy action section outside package")?,
    ))
}

fn action(row: &[u8]) -> Result<EnemyAction> {
    Ok(EnemyAction {
        weight: row[0] as i8,
        target_policy: row[1],
        recovery_ticks: row[2],
        recovery_clip: row[3],
        recovery_rate: FloatOperand::read(row, 4)?,
        requirements: u32::read(row, 8)?,
        target_state: u16::read(row, 12)?,
        duration: u16::read(row, 14)?,
        range: <[i16; 2]>::read(row, 16)?,
        approach_range: i16::read(row, 20)?,
        approach_minimum: i16::read(row, 22)?,
        animation: u16::read(row, 24)?,
        command: u16::read(row, 26)?,
        hit: u16::read(row, 28)?,
        combo_at: u16::read(row, 30)?,
        followup_group: row[32],
        effect: row[34],
        guard_chance: row[35],
        vulnerable: <[u16; 2]>::read(row, 36)?,
        movement_speed: FloatOperand::read(row, 40)?,
        movement_rate: FloatOperand::read(row, 44)?,
        movement_clip: row[48],
        stagger_threshold: row[49],
        followup_chance: row[50],
        required_monster: row[51],
        tp: row[52],
        hit_recovery_clip: row[53],
        recovery_command: u16::read(row, 54)?,
        resource_decrement: row[56],
        required_story_flag: u16::read(row, 58)?,
        cast_voices: <[u16; 2]>::read(row, 60)?,
        technique: u16::read(row, 64)?,
        storage: unreferenced_storage(row, vec![0..33, 34..57, 58..66]),
    })
}

/// Terminal records consume their first halfword only. Preserve the physical
/// trailing operands when present, and retain padding outside the source table.
fn fixed<T>(
    bytes: &[u8],
    stride: usize,
    sentinel: i16,
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<(Vec<T>, usize)> {
    if bytes.is_empty() {
        return Ok((vec![], 0));
    }
    let last = bytes
        .chunks(stride)
        .rposition(|row| i16::read(row, 0).ok() == Some(sentinel))
        .context("unterminated enemy action table")?;
    let end = ((last + 1) * stride).min(bytes.len());
    let rows = bytes[..end]
        .chunks(stride)
        .map(|row| {
            if row.len() == stride {
                decode(row)
            } else {
                ensure!(i16::read(row, 0)? == sentinel, "truncated enemy action row");
                let mut padded = vec![0; stride];
                padded[..row.len()].copy_from_slice(row);
                decode(&padded)
            }
        })
        .collect::<Result<_>>()?;
    Ok((rows, end))
}

pub(crate) fn read(bytes: &[u8]) -> Result<EnemyActions> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy action package");
    let (_, source) = section(bytes, 10)?;
    ensure!(source.len().is_multiple_of(68), "misaligned enemy actions");
    let rows = source
        .chunks_exact(68)
        .map(action)
        .collect::<Result<Vec<_>>>()?;
    let (_, source) = section(bytes, 12)?;
    ensure!(
        source.len() >= 26 && source[24] <= 4,
        "invalid enemy policy"
    );
    let policy = EnemyPolicy {
        native: source[8],
        counter_chance: source[11],
        counter_count: source[22],
        ordinary_count: source[23],
        back_row_count: source[24],
        back_row_actions: <[u8; 4]>::read(source, 14)?,
        back_row_weights: <[u8; 4]>::read(source, 18)?,
        overlimit_first_action: source[25],
        storage: unreferenced_storage(source, vec![8..9, 11..12, 14..26]),
    };
    let (_, source) = section(bytes, 8)?;
    ensure!(
        source.len().is_multiple_of(28),
        "misaligned enemy hit rules"
    );
    let hit_rules = source
        .chunks_exact(28)
        .map(hit_rule)
        .collect::<Result<_>>()?;
    let mut storage = Vec::new();
    let mut retain = |start, source: &[u8], covered| {
        storage.extend(
            unreferenced_storage(source, covered)
                .into_iter()
                .map(|mut row| {
                    row.offset += start;
                    row
                }),
        );
    };
    let (start, source) = section(bytes, 16)?;
    let (hits, end) = fixed(source, 32, -1, hit)?;
    retain(start, source, vec![0..end]);
    let (start, source) = section(bytes, 18)?;
    let (animations, end) = fixed(source, 12, -2, animation)?;
    retain(start, source, vec![0..end]);
    let (start, source) = section(bytes, 14)?;
    // Declarations may contain signed -1 or dormant roots outside this pool.
    // Retain those rows; preparation checks the selected executable route.
    let roots = rows
        .iter()
        .flat_map(|row| [row.command, row.recovery_command])
        .filter(|&root| root != u16::MAX && usize::from(root) * 2 < source.len())
        .map(u32::from);
    let commands = commands(source, roots)?;
    retain(start, source, command_ranges(&commands).collect());
    Ok(EnemyActions {
        rows,
        policy,
        hit_rules,
        hits,
        animations,
        commands,
        storage,
    })
}

#[cfg(test)]
mod tests;
