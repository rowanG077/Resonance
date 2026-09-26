//! Negative normal-hit emissions select equipped-weapon flight parameters.
use super::{WeaponFlightResource, action, recoil};
use anyhow::{Context, Result, ensure};
use resonance_battle::WeaponFlightDefinition;
use resonance_content::{battle_action::NormalTable, prepared::Files};

pub fn load(files: &Files, request: &WeaponFlightResource) -> Result<WeaponFlightDefinition> {
    let table: NormalTable = files.json(&request.source)?;
    let group = table
        .groups
        .get(usize::from(request.character))
        .context("missing weapon-flight character")?;
    let selector = group
        .selectors
        .get(usize::from(request.selection))
        .context("missing weapon-flight selection")?;
    let bundle = group
        .actions
        .get(usize::from(selector.action))
        .context("missing weapon-flight action")?;
    let start = usize::try_from(bundle.hit)?;
    let end = start
        .checked_add(usize::from(request.row))
        .context("weapon-flight row overflow")?;
    let rows = group
        .hits
        .get(start..=end)
        .context("missing weapon-flight hit")?;
    ensure!(
        rows.iter().all(|row| row.start >= 0),
        "weapon-flight hit follows stream end"
    );
    let row = &rows[rows.len() - 1];
    let profile =
        usize::try_from(-3_i16 - i16::from(row.emission)).context("hit is not a weapon flight")?;
    let flight = table
        .weapon_flights
        .get(profile)
        .context("unprepared weapon-flight profile")?;
    ensure!(row.hit_class <= 1, "weapon-flight response is not prepared");
    let rule = group
        .hit_rules
        .get(usize::from(row.rule))
        .context("missing weapon-flight hit rule")?;
    let mut hit = action::damage(
        rule,
        action::damage_kind(row.damage_kind)?,
        row.hit_class != 0,
        &request.impact,
    )?;
    hit.reaction = recoil::Parameters::load(files)?.reaction(rule, row.reaction, 1)?;
    Ok(WeaponFlightDefinition {
        slot: row.emission_operands[0],
        outbound_ticks: u16::try_from(flight.outbound_ticks)
            .context("invalid weapon outbound duration")?,
        speed: flight.speed.finite()?,
        return_speed: flight.return_speed.finite()?,
        direction_y: flight.direction_y.finite()?,
        hit,
        cooldown: rule.contact_cooldown,
        radius: row.radius.finite()?,
        height: row.height.finite()?,
        shape: action::shape(row.shape, row.inner_radius)?,
    })
}
