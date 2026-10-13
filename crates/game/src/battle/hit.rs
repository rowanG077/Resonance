//! Bind authored contact rules to prepared battle resources.

pub(super) fn authored(
    source: &resonance_content::battle_action::ActionHit,
    recoil: &super::recoil::Parameters,
) -> anyhow::Result<resonance_battle::HitRule> {
    Ok(resonance_battle::HitRule {
        kind: source.kind,
        arte: source.arte,
        effect: None,
        effect_on_guard: source.effect_on_guard,
        sound: source.sound.map(resonance_battle::Sound::Cue),
        overlimit_pause: true,
        power: source.power,
        element: source.element,
        prevents_defeat: source.prevents_defeat,
        finisher_grade: source.finisher_grade,
        hp_drain: source.hp_drain,
        guard: resonance_battle::GuardRule {
            enabled: !source.bypass_guard,
            suppress_sound: source.suppress_guard_sound,
            pressure: source.guard_pressure,
            breaks: source.breaks_guard,
            unbreakable: source.unbreakable_guard,
        },
        reaction: resonance_battle::ReactionRule {
            capture: source.capture,
            suppress: source.suppress_reaction,
            force: source.force_reaction,
            recoil: recoil.hit(source)?,
            direction: Default::default(),
            hitstun: source.hitstun,
            alternate_motion: source.alternate_motion,
            armor_damage: source.armor_damage,
            stun_chance: source.stun_chance,
            stagger: source.stagger,
            hits_down: source.hits_down,
        },
        condition: source.condition,
    })
}
