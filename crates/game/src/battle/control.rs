//! Bind original normal selectors and player motion parameters before battle entry.
use super::profile;
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, ControlDefinition, ControlMotions, NormalControl};
use resonance_content::{
    battle_action::{NORMAL_PATH, NormalTable},
    prepared::Files,
};

pub fn party(
    files: &Files,
    character: u8,
    actor: ActorId,
    target: ActorId,
    actions: [u16; 7],
    motions: ControlMotions,
    combo_limit: u8,
) -> Result<ControlDefinition> {
    let profile = profile::party_template(files, character)?;
    Ok(ControlDefinition {
        actor,
        target,
        normals: normals(files, character, actions)?,
        shortcuts: [None; 4],
        combo_limit,
        walk_speed: profile.walk_speed.finite()?,
        run_speed: profile.run_speed.finite()?,
        turn_ticks: profile.turn_ticks,
        motions: Some(motions),
    })
}

pub fn normals(files: &Files, character: u8, actions: [u16; 7]) -> Result<[NormalControl; 7]> {
    let table: NormalTable = files.json(NORMAL_PATH)?;
    let group = character
        .checked_sub(1)
        .and_then(|index| table.groups.get(usize::from(index)))
        .context("missing normal-attack character")?;
    ensure!(group.selectors.len() == 7, "invalid normal selector count");
    let controls = group
        .selectors
        .iter()
        .zip(actions)
        .map(|(selector, action)| {
            let bundle = group
                .actions
                .get(usize::from(selector.action))
                .context("missing normal action")?;
            let descriptor = group
                .descriptors
                .get(bundle.descriptor as usize)
                .context("missing normal descriptor")?;
            Ok(NormalControl {
                action,
                allowed_directions: selector.allowed_directions,
                fallback: (selector.fallback != u8::MAX).then_some(selector.fallback),
                // 1F548 uses a signed-16 quantized load (GQR5).
                reach: f32::from(descriptor.reach[0] as i16),
                minimum_reach: f32::from(descriptor.reach[1] as i16),
                combo_at: descriptor.combo_at,
                buffer_until: descriptor.buffer_until,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(controls.try_into().unwrap())
}

/// 268C4/37824/1E12C: player shortcut slots use learned membership and the
/// descriptor's player range, independently of the automatic-policy disable list.
pub fn shortcuts(
    files: &Files,
    character: u8,
    member: &resonance_events::party::Member,
    actions: &std::collections::BTreeMap<u16, u16>,
) -> Result<[Option<resonance_battle::TechniqueControl>; 4]> {
    let catalogue: resonance_content::arte::Catalogue =
        files.json(resonance_content::arte::PATH)?;
    let learned_by = catalogue.learned_by(character)?;
    let mut shortcuts = [None; 4];
    for (slot, &id) in member.shortcuts.iter().enumerate() {
        if id == 0
            || !member.techniques.contains(&id)
            || !learned_by.iter().any(|&learned| u16::from(learned) == id)
        {
            continue;
        }
        let descriptor = catalogue.definition(usize::from(id))?;
        let raw = (i32::from(descriptor.cast_time_adjustment) << 16)
            | i32::from(descriptor.recovery_ticks as u16);
        let maximum = if descriptor.flags & 2 != 0 && raw >= 1000 {
            8000.
        } else {
            raw as f32
        };
        ensure!(maximum > 0., "invalid player technique range");
        shortcuts[slot] = Some(resonance_battle::TechniqueControl {
            action: *actions
                .get(&id)
                .context("missing learned player shortcut action")?,
            minimum: 0.,
            maximum,
        });
    }
    Ok(shortcuts)
}
