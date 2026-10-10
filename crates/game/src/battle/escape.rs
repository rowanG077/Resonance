//! Ordinary Escape operands, resolved against the same roster before activation.
use super::{model::ModelSource, voice};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, EscapeActorDefinition, EscapeDefinition, Sound};
use resonance_content::menu_data::MenuData;
use resonance_events::party::Party;

/// Admitted equipment grants the escape bonus regardless of owner availability.
pub(super) fn magic_mist(
    party: &Party,
    menus: &MenuData,
    characters: impl IntoIterator<Item = u8>,
) -> Result<bool> {
    let mut active = false;
    for character in characters {
        ensure!((1..=9).contains(&character), "unprepared Escape character");
        let index = usize::from(character.checked_sub(1).context("zero Escape character")?);
        let member = party
            .members
            .get(index)
            .context("missing Escape party member")?;
        for &item in &member.equipment {
            let item = menus
                .items
                .get(usize::from(item))
                .context("missing Escape equipment")?;
            active |= item.properties.quick_escape;
        }
    }
    Ok(active)
}

pub fn prepare(
    voices: &voice::Resolver<'_>,
    roster: impl Iterator<Item = (ActorId, ModelSource)> + Clone,
    party: &Party,
    menus: &MenuData,
    allowed: bool,
    level_difference: i8,
    mut sound: impl FnMut(voice::Sound) -> Result<Option<Sound>>,
) -> Result<EscapeDefinition> {
    ensure!(
        (-8..=8).contains(&level_difference),
        "invalid Escape level difference"
    );
    let magic_mist = magic_mist(
        party,
        menus,
        roster.clone().filter_map(|(_, source)| match source {
            ModelSource::Party(character) => Some(character),
            _ => None,
        }),
    )?;
    let actors = roster
        .filter(|(_, source)| matches!(source, ModelSource::Party(_)))
        .map(|(actor, source)| {
            Ok(EscapeActorDefinition {
                actor,
                request: voices.select(source, |v| v.escape_request, &mut sound)?,
                success: voices.select(source, |v| v.escape_success, &mut sound)?,
                cancel: voices.select(source, |v| v.escape_cancel, &mut sound)?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(EscapeDefinition {
        allowed,
        level_difference,
        magic_mist,
        actors,
    })
}
