//! Resolve original action records from the same verified encounter snapshot.
use super::{HitResource, HitSelection};
use anyhow::{Context, Result, bail, ensure};
use resonance_battle::{DamageKind, Element, GuardRule, HitElement, HitRule, HitShape, Power};
use resonance_content::{battle_action, prepared::Files, source::FloatOperand};

pub fn hit(files: &Files, request: &HitResource) -> Result<battle_action::HitRule> {
    let HitSelection::Technique { member, phase } = request.selection else {
        let source: resonance_content::battle_model::Enemy = files.json(&request.source)?;
        return source
            .actions
            .hit_rules
            .get(usize::from(request.rule))
            .cloned()
            .context("missing enemy hit rule");
    };
    let table: battle_action::Table = files.json(&request.source)?;
    let bundle = table
        .records
        .get(usize::from(member))
        .and_then(Option::as_ref)
        .with_context(|| format!("missing action {member} in {}", request.source))?;
    // The common loader copies four phases. Retained compact slots need their
    // own source-backed selection route before they can be activated.
    ensure!(
        bundle.phases.len() == 4,
        "compact action selection is not prepared"
    );
    let phase = bundle
        .phases
        .get(usize::from(phase))
        .context("missing action phase")?;
    let index = phase.indices[0]
        .checked_add(u32::from(request.rule))
        .context("action hit index overflow")?;
    bundle
        .hit_rules
        .get(index as usize)
        .cloned()
        .context("missing action hit rule")
}

pub(super) fn damage_kind(kind: u8) -> Result<DamageKind> {
    match kind {
        0 => Ok(DamageKind::Slash),
        1 => Ok(DamageKind::Thrust),
        2 => Ok(DamageKind::Magic),
        other => bail!("contact damage kind {other} is not prepared"),
    }
}

pub(super) fn shape(kind: u8, width: FloatOperand) -> Result<HitShape> {
    match kind {
        0 => Ok(HitShape::Box),
        1 => Ok(HitShape::Cylinder),
        2 => Ok(HitShape::GroundCircle),
        3 => Ok(HitShape::Ring {
            width: width.finite()?,
        }),
        4 => Ok(HitShape::Sphere),
        other => bail!("contact shape {other} is not prepared"),
    }
}

pub(super) fn damage(
    row: &battle_action::HitRule,
    kind: DamageKind,
    guarded: bool,
    impact: &Option<super::EffectResource>,
) -> Result<HitRule> {
    // Only admit the completed damage/guard operations. Cooking retains the full
    // record; preparation must not silently discard conditions or reactions.
    ensure!(
        row.flags & !0x19e3 == 0,
        "action hit flags {:#x} are not prepared",
        row.flags
    );
    ensure!(
        row.conditions == 0,
        "action hit conditions are not prepared"
    );
    ensure!(row.sound == 0, "action hit sound is not prepared");
    let element = match row.element {
        0 => HitElement::Inherited,
        10 => HitElement::Neutral,
        element @ 1..=8 => HitElement::Element(Element::ALL[usize::from(element - 1)]),
        other => bail!("invalid action hit element {other}"),
    };
    let power = match row.power_mode {
        0 | 2 => Power::Normal,
        1 => Power::Percent(row.power),
        3 => Power::Fixed(row.power),
        mode => bail!("action hit power mode {mode} is not prepared"),
    };
    Ok(HitRule {
        impact: if row.impact_effect == 0 {
            ensure!(impact.is_none(), "unexpected contact impact binding");
            None
        } else {
            let binding = impact.as_ref().context("missing contact impact binding")?;
            let member = u16::from(row.impact_effect);
            ensure!(
                binding.members.contains(&member),
                "unbound contact impact member {member}"
            );
            Some(resonance_battle::ImpactEffect {
                appearance: resonance_battle::EffectAppearance {
                    resource: binding.resource,
                    member,
                },
                on_guard: row.flags & 0x800 != 0,
            })
        },
        reaction: resonance_battle::ReactionRule {
            stun_chance: row.stun_chance,
            stagger: row.stagger,
            hits_down: row.flags & 0x40 != 0,
            ..Default::default()
        },
        kind,
        arte: row.flags & 0x20 != 0,
        power,
        element,
        prevents_defeat: row.flags & 1 != 0,
        guard: GuardRule {
            enabled: guarded,
            pressure: row.guard_pressure,
            breaks: row.flags & 0x80 != 0,
            unbreakable: row.flags & 0x1000 != 0,
        },
    })
}
