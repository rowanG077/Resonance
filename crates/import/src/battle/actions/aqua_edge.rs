//! Recover the rule and runner duration; verify the fixed three-blade controller.
use super::*;

#[cfg(test)]
fn validate_controller(rel: &Rel) -> Result<()> {
    let dispatch = rel.pointer(DATA, 0x1238)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x7587c)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48)
            && rel.local_targets().contains(&(1, 0x75608)),
        "unexpected Aqua Edge native controller"
    );
    // These constants implement the named controller, rather than a generic
    // timeline. Check its branch immediates and vector parameters together.
    for (offset, instruction) in [
        (0x75738, 0x2c030078),
        (0x75780, 0x2c03001e),
        (0x75720, 0x2c1b0003),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected Aqua Edge controller timing or blade count"
        );
    }
    validate_parameters(rel, 0x5200, 0x2800)
}

pub(super) fn validate_parameters(rel: &Rel, constants: usize, threshold: usize) -> Result<()> {
    let constants = rel.at((4, constants))?;
    for (i, expected) in [0_f32, 0., 1., std::f32::consts::PI / 180., 120., 0.75, 0.5]
        .into_iter()
        .enumerate()
    {
        ensure!(
            float(constants, i * 4)?.to_bits() == expected.to_bits(),
            "unexpected Aqua Edge spread or speed control"
        );
    }
    ensure!(
        float(rel.at((4, threshold))?, 0)? == 0.5,
        "unexpected horizontal aim threshold"
    );
    Ok(())
}

pub(super) fn recipe(source: &bundle::Bundle) -> Result<AquaEdgeRecipe> {
    ensure!(
        source
            .phases
            .iter()
            .skip(1)
            .all(|phase| phase.duration == 0),
        "unsupported Aqua Edge phase variants"
    );
    let recipe = AquaEdgeRecipe {
        lifetime: source.phases[0].duration,
        rule: source.rule(0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_water_rule_requires_the_exact_three_blade_controller() {
        let mut rel = Rel {
            bytes: vec![0; 0x75804],
            sections: vec![(4, 0x75800); 6],
            pointers: [
                ((5, 0x1238), (5, 0x5b58)),
                ((5, 0x5b58), (1, 0x7587c)),
                ((5, 0x5b5c), (1, 0x37e48)),
            ]
            .into(),
            local_targets: [(1, 0x75608)].into(),
        };
        for (offset, instruction) in [
            (0x75738, 0x2c030078_u32),
            (0x75780, 0x2c03001e),
            (0x75720, 0x2c1b0003),
        ] {
            rel.bytes[offset + 4..offset + 8].copy_from_slice(&instruction.to_be_bytes());
        }
        for (i, value) in [0_f32, 0., 1., std::f32::consts::PI / 180., 120., 0.75, 0.5]
            .into_iter()
            .enumerate()
        {
            rel.bytes[0x5204 + i * 4..0x5208 + i * 4].copy_from_slice(&value.to_be_bytes());
        }
        rel.bytes[0x2804..0x2808].copy_from_slice(&0.5_f32.to_be_bytes());
        let mut source = [0; 156];
        source[..16].copy_from_slice(&[0, 0, 0, 128, 0, 0, 0, 156, 0, 0, 0, 156, 0, 0, 0, 156]);
        source[16..18].copy_from_slice(&150_u16.to_be_bytes());
        source[128..].copy_from_slice(&[
            0, 32, 1, 20, 30, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        let source = bundle::Bundle::decode(&source).unwrap();
        validate_controller(&rel).unwrap();
        let recovered = recipe(&source).unwrap();
        assert_eq!(recovered.lifetime, 150);
        assert_eq!(
            (
                recovered.rule.power,
                recovered.rule.hitstun,
                recovered.rule.contact_cooldown
            ),
            (60, 20, 30)
        );
        assert!(matches!(
            recovered.rule.element,
            HitElement::Element(resonance_content::menu_data::Element::Water)
        ));
        rel.bytes[0x75787] = 31;
        assert!(
            validate_controller(&rel).is_err(),
            "a different turn tick must not use the fixed controller"
        );
    }
}
