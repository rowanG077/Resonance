//! Bind common contact programs and element tints before activation.
use resonance_battle::EffectAppearance;
use resonance_content::battle_effect::Tints;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy)]
pub struct ContactElementFeedback {
    /// None means an intentionally empty element entry.
    pub effect: Option<EffectAppearance>,
    pub color: [u8; 3],
}

#[derive(Debug, Clone)]
pub struct ContactArt {
    pub ordinary: EffectAppearance,
    pub guard: EffectAppearance,
    pub critical: EffectAppearance,
    pub guard_break: EffectAppearance,
    pub overlimit: EffectAppearance,
    /// Neutral first, followed by Element::ALL.
    pub elements: [ContactElementFeedback; 9],
}

pub fn admission(
    source: Option<&Tints>,
    flash: Option<resonance_content::arte::AdmissionFlash>,
) -> Option<[u8; 3]> {
    use resonance_content::arte::AdmissionFlash;
    let colors = &source?.admission_colors;
    let [r, g, b, _] = match flash? {
        AdmissionFlash::Basic => colors.basic,
        AdmissionFlash::Advanced => colors.advanced,
        AdmissionFlash::Arcane => colors.arcane,
    };
    Some([r, g, b])
}

pub fn prepare(source: &Tints, common_resource: u32) -> (ContactArt, Vec<u16>) {
    let appearance = |member| EffectAppearance {
        resource: common_resource,
        member,
    };
    let mut members = BTreeSet::from([0, 1, 11, 16, 47]);
    // Prepare all elements because equipment can change them during battle.
    let elements = std::array::from_fn(|index| {
        let member = u16::from(source.contact_effects[index]);
        if member != 0 {
            members.insert(member);
        }
        ContactElementFeedback {
            effect: (member != 0).then(|| appearance(member)),
            color: source.contact_colors[index][..3].try_into().unwrap(),
        }
    });
    (
        ContactArt {
            ordinary: appearance(11),
            guard: appearance(1),
            critical: appearance(16),
            guard_break: appearance(0),
            overlimit: appearance(47),
            elements,
        },
        members.into_iter().collect(),
    )
}
