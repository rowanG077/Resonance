//! The native update repeats each RGBA increment/decrement before advancing age.
//! Fold those two operations into the shared compiler's single per-tick rate.
use resonance_content::battle_effect::{
    EffectOperation, ValueRange, declaration::Declaration, document::Definition,
};

fn rate(value: u8) -> u8 {
    value.saturating_mul(2)
}

fn range(value: &mut ValueRange) {
    // Keep the original discrete choices and correlated random sample. The
    // shared birth evaluator clamps the sampled byte, rather than the range
    // endpoints, so choices above 127 retain their native saturation behavior.
    value.min *= 2.;
    value.max *= 2.;
    value.step *= 2.;
}

pub(super) fn normalize(definition: &mut Definition) {
    // Native 0x48bf03/0x48bf6b add to the same endpoint; 0x48bff5/0x48c055
    // likewise subtract twice. Neither pair advances the endpoint or age.
    for declaration in &mut definition.templates {
        if let Declaration::Particle { template, .. }
        | Declaration::ModelParticle { template, .. } = declaration
        {
            template.state.brighten = template.state.brighten.map(rate);
            template.fade = template.fade.map(rate);
        }
    }
    for event in definition.programs.iter_mut().flatten() {
        if let EffectOperation::Spawn { birth, .. } = &mut event.operation {
            for value in birth.brighten.iter_mut().chain(&mut birth.fade).flatten() {
                range(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_normalized_rate_matches_both_native_clamped_updates() {
        for color in 0u8..=255 {
            for source in 0u8..=255 {
                assert_eq!(
                    color.saturating_add(source).saturating_add(source),
                    color.saturating_add(rate(source)),
                );
                assert_eq!(
                    color.saturating_sub(source).saturating_sub(source),
                    color.saturating_sub(rate(source)),
                );
            }
        }
    }

    #[test]
    fn constructor_rates_keep_discrete_samples_and_correlation() {
        let source = ValueRange {
            min: 120.,
            max: 140.,
            step: 2.,
            draw: Some(3),
            reverse: true,
        };
        let mut normalized = source;
        range(&mut normalized);
        normalized.validate().unwrap();
        assert_eq!(normalized.draw, source.draw);
        assert_eq!(normalized.reverse, source.reverse);
        assert_eq!(normalized.choice_count(), source.choice_count());
        for sample in [0., 0.1, 0.5, 0.9, 0.999] {
            let native = (source.sample(sample) as u8).saturating_mul(2);
            assert_eq!(
                normalized.sample(sample).round().clamp(0., 255.) as u8,
                native
            );
        }
    }
}
