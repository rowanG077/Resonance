//! Bind the original common contact programs and element tints before activation.
use anyhow::{Result, ensure};
use resonance_battle::{ContactElementFeedback, ContactFeedback, EffectAppearance, Element};
use resonance_content::battle_effect::Tints;
use std::collections::{BTreeMap, BTreeSet};

/// 1E12C tests independent action-family masks, with later matches replacing
/// earlier RGB. Callers pass the original admission enable mode explicitly.
pub fn admission(source: &Tints, flags: u32, enabled: bool) -> Option<[u8; 3]> {
    if !enabled {
        return None;
    }
    [0x02000004, 0x04000008, 0x08000010]
        .into_iter()
        .zip(source.admission_colors)
        .filter_map(|(mask, color)| (flags & mask != 0).then_some(color[..3].try_into().unwrap()))
        .next_back()
}

pub fn prepare(
    source: &Tints,
    common_resource: u32,
    elements: &BTreeSet<Option<Element>>,
) -> Result<(ContactFeedback, Vec<u16>)> {
    ensure!(
        source.contact_effects[0] == 0,
        "neutral contact has an element effect"
    );
    let appearance = |member| EffectAppearance {
        resource: common_resource,
        member,
    };
    let mut members = BTreeSet::from([0, 1, 2, 11, 12, 16, 47]);
    let mut prepared = BTreeMap::new();
    for element in elements.iter().copied().chain([None]) {
        let index = element.map_or(0, |element| element as usize + 1);
        let member = u16::from(source.contact_effects[index]);
        if member != 0 {
            members.insert(member);
        }
        prepared.insert(
            element,
            ContactElementFeedback {
                effect: (member != 0).then(|| appearance(member)),
                color: source.contact_colors[index][..3].try_into().unwrap(),
            },
        );
    }
    Ok((
        ContactFeedback {
            ordinary: [appearance(11), appearance(12)],
            guard: [appearance(1), appearance(2)],
            critical: appearance(16),
            guard_break: appearance(0),
            overlimit: appearance(47),
            elements: prepared,
        },
        members.into_iter().collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_admission_uses_original_family_masks_and_casting_mode() {
        let mut source: Tints =
            serde_json::from_str(include_str!("../../tests/fixtures/effect-tints.json")).unwrap();
        assert_eq!(admission(&source, 0x00041106, true), Some([40, 40, 112]));
        assert_eq!(admission(&source, 0x00444186, false), None);
        assert_eq!(admission(&source, 1, true), None);
        source.admission_colors = [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13; 4]];
        for (flags, color) in [
            (4, [1, 2, 3]),
            (0x04000000, [5, 6, 7]),
            (0x0800000c, [9, 10, 11]),
        ] {
            assert_eq!(admission(&source, flags, true), Some(color));
        }
    }
}
