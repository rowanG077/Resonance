//! Enemy action choices and projectile geometry; the game owns hit policies.
use crate::read::Field;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_action::*;

/// Match enemy asset rows to the maintained native actions.
fn attacks(enemy: u8) -> &'static [EnemyAttack] {
    use EnemyAttack::*;
    match enemy {
        12 => &[Swing, Scatter, Lob],
        34 => &[Tail, Pounce],
        36 => &[Right, Push, Double, Triple, Counter],
        37 => &[Double, Cross, Triple],
        49 => &[Strike, Spit],
        50 => &[Strike, Spit, FireBall],
        _ => &[],
    }
}

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

const STAY_AFTER_ACTION: u32 = 0x20;

fn requirements(flags: u32) -> Result<EnemyRequirements> {
    const EASY_ONLY: u32 = 0x2000_0000;
    const NORMAL_OR_HARD: u32 = 4;
    const HARD_ONLY: u32 = 8;
    const HP_THREE_QUARTERS: u32 = 0x10_0000;
    const HP_HALF: u32 = 2;
    const HP_QUARTER: u32 = 1;
    const PRIORITY: u32 = 0x8_0000;
    const UNUSED: u32 = 0x40;
    ensure!(
        flags
            & !(EASY_ONLY
                | NORMAL_OR_HARD
                | HARD_ONLY
                | HP_THREE_QUARTERS
                | HP_HALF
                | HP_QUARTER
                | PRIORITY
                | STAY_AFTER_ACTION
                | UNUSED)
            == 0,
        "unsupported enemy eligibility flags {flags:#x}"
    );
    Ok(EnemyRequirements {
        difficulty: (if flags & HARD_ONLY != 0 {
            2
        } else {
            u8::from(flags & NORMAL_OR_HARD != 0)
        })..=(if flags & EASY_ONLY != 0 { 0 } else { 2 }),
        hp_percent: if flags & HP_QUARTER != 0 {
            Some(25)
        } else if flags & HP_HALF != 0 {
            Some(50)
        } else if flags & HP_THREE_QUARTERS != 0 {
            Some(75)
        } else {
            None
        },
        priority: flags & PRIORITY != 0,
    })
}

fn action(
    row: &[u8],
    attack: Option<EnemyAttack>,
    projectiles: &[resonance_content::battle_projectile::Projectile],
) -> Result<EnemyAction> {
    let mut unsupported_reason = if row[48] != 0
        || f32::from_bits(u32::read(row, 40)?) != 0.
        || f32::from_bits(u32::read(row, 44)?) != 0.
    {
        Some("enemy custom approach motion is not prepared".to_owned())
    } else if row[51] != 0 || u16::read(row, 58)? != 0 {
        Some("enemy special eligibility is not prepared".to_owned())
    } else if row[50] as i8 > 0 {
        Some("enemy follow-up selection is not prepared".to_owned())
    } else {
        None
    };
    let weight = u8::try_from(row[0] as i8).unwrap_or_else(|_| {
        unsupported_reason.get_or_insert_with(|| "negative enemy action weight".into());
        0
    });
    let flags = u32::read(row, 8)?;
    let requirements = requirements(flags).unwrap_or_else(|error| {
        unsupported_reason.get_or_insert_with(|| error.to_string());
        EnemyRequirements::default()
    });
    let target_policy = if row[1] == 0 {
        None
    } else {
        match TargetPolicy::try_from(row[1]) {
            Ok(policy) => Some(policy),
            Err(error) => {
                unsupported_reason.get_or_insert_with(|| error.to_string());
                None
            }
        }
    };
    let projectile = match attack {
        Some(EnemyAttack::Spit) => Some(0),
        Some(EnemyAttack::Scatter) => Some(0),
        Some(EnemyAttack::Lob) => Some(1),
        _ => None,
    }
    .map(|projectile| {
        projectiles
            .get(projectile)
            .cloned()
            .context("missing enemy projectile")
    })
    .transpose()?;
    if attack.is_none() {
        unsupported_reason.get_or_insert_with(|| "enemy action has no native definition".into());
    }
    Ok(EnemyAction {
        weight,
        target_policy,
        requirements,
        return_to_formation: flags & STAY_AFTER_ACTION == 0,
        range: <[i16; 2]>::read(row, 16)?,
        approach_range: i16::read(row, 20)?,
        approach_minimum: i16::read(row, 22)?,
        guard_chance: row[35] as i8,
        tp: row[52],
        attack,
        projectile,
        unsupported_reason,
    })
}

pub(crate) fn read(
    bytes: &[u8],
    enemy: u8,
    projectiles: &[resonance_content::battle_projectile::Projectile],
) -> Result<EnemyActions> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy action package");
    let (_, source) = section(bytes, 10)?;
    ensure!(source.len().is_multiple_of(68), "misaligned enemy actions");
    let rows = source
        .chunks_exact(68)
        .enumerate()
        .map(|(index, row)| action(row, attacks(enemy).get(index).copied(), projectiles))
        .collect::<Result<Vec<_>>>()?;
    let (_, source) = section(bytes, 12)?;
    ensure!(
        source.len() >= 26 && source[24] <= 4,
        "invalid enemy policy"
    );
    let policy = EnemyPolicy {
        back_row: (0..usize::from(source[24]))
            .map(|index| EnemyBackRow {
                action: source[14 + index],
                weight: source[18 + index],
            })
            .collect(),
        unsupported_reason: if source[8] != 0
            || source[11] != 0
            || source[22] != 0
            || source[25] != 0
        {
            Some("enemy native/counter/overlimit decisions are not prepared".into())
        } else if usize::from(source[23]) != rows.len() {
            Some("enemy action/decision row count differs".into())
        } else {
            None
        },
    };
    Ok(EnemyActions { rows, policy })
}

#[cfg(test)]
mod tests;
