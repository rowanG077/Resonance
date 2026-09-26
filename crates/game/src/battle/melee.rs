//! Load attached party/enemy contacts from the encounter's verified snapshot.
use super::{MeleeResource, MeleeSelection, action, recoil};
use anyhow::{Context, Result, ensure};
use resonance_battle::MeleeDefinition;
use resonance_content::{
    battle_action::{Hit, HitRule, NormalTable},
    battle_model::Enemy,
    prepared::Files,
};

pub fn load(files: &Files, request: &MeleeResource) -> Result<MeleeDefinition> {
    match request.selection {
        MeleeSelection::Normal {
            character,
            selection,
        } => {
            let table: NormalTable = files.json(&request.source)?;
            let group = table
                .groups
                .get(usize::from(character))
                .context("missing normal-attack character")?;
            let selector = group
                .selectors
                .get(usize::from(selection))
                .context("missing normal-attack selection")?;
            let bundle = group
                .actions
                .get(usize::from(selector.action))
                .context("missing normal-attack action")?;
            contact(files, request, bundle.hit, &group.hits, &group.hit_rules)
        }
        MeleeSelection::Enemy { action } => {
            let source: Enemy = files.json(&request.source)?;
            let bundle = source
                .actions
                .rows
                .get(usize::from(action))
                .context("missing enemy action")?;
            contact(
                files,
                request,
                u32::from(bundle.hit),
                &source.actions.hits,
                &source.actions.hit_rules,
            )
        }
    }
}

fn contact(
    files: &Files,
    request: &MeleeResource,
    start: u32,
    hits: &[Hit],
    rules: &[HitRule],
) -> Result<MeleeDefinition> {
    let index = start
        .checked_add(u32::from(request.row))
        .context("contact index overflow")?;
    let rows = hits
        .get(start as usize..=index as usize)
        .context("missing attached contact")?;
    ensure!(
        rows.iter().all(|row| row.start >= 0),
        "contact lies past stream end"
    );
    let row = &rows[rows.len() - 1];
    // 2D564: only ordinary attached contacts. Negative emissions allocate
    // projectiles; zero and negative attachment counts use distinct consumers.
    ensure!(
        row.emission >= 0 && (1..=4).contains(&row.attachment_count),
        "attached contact emission is not prepared"
    );
    ensure!(
        row.hit_class <= 1,
        "attached contact response is not prepared"
    );
    let rule = rules
        .get(usize::from(row.rule))
        .context("missing attached hit rule")?;
    let mut hit = action::damage(
        rule,
        action::damage_kind(row.damage_kind)?,
        row.hit_class != 0,
        &request.impact,
    )?;
    // Ordinary bone-group submissions pass direction 1 to 3D920.
    hit.reaction = recoil::Parameters::load(files)?.reaction(rule, row.reaction, 1)?;
    let mut anchors = Vec::new();
    for &id in &row.emission_operands[..row.attachment_count as usize] {
        let group = request
            .anchor_groups
            .get(usize::from(id))
            .context("missing contact attachment group")?;
        anchors.extend_from_slice(group);
    }
    ensure!(
        !anchors.is_empty() && anchors.len() <= 40,
        "invalid contact anchor count"
    );
    Ok(MeleeDefinition {
        hit,
        cooldown: rule.contact_cooldown,
        radius: row.radius.finite()?,
        height: row.height.finite()?,
        shape: action::shape(row.shape, row.inner_radius)?,
        anchors,
        trail: (row.emission_operands[0] <= 3).then_some(row.emission_operands[0]),
    })
}
