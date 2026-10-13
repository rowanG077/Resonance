//! Sword Dancer's authored variants select appearance and available actions once.
use anyhow::{Context, Result, ensure};
use resonance_battle::{EnemyBehavior, EnemyDecisionDefinition, ModelDefinition};

const LAST_STAGE: usize = 2;
const WINGS: [&str; 2] = ["Bone_hane01_L", "Bone_hane01_R"];
const TAIL: &str = "Bone_sippo01";

pub fn sword_scale(variant: usize, authored_scale: f32) -> Result<f32> {
    ensure!(
        variant <= LAST_STAGE,
        "unsupported Sword Dancer stage {variant}"
    );
    let scale = authored_scale - (LAST_STAGE - variant) as f32 * 0.1;
    ensure!(
        scale.is_finite() && scale > 0.,
        "invalid Sword Dancer scale"
    );
    Ok(scale)
}

pub fn apply_sword(
    variant: usize,
    model: Option<&ModelDefinition>,
    decision: &mut EnemyDecisionDefinition,
) -> Result<()> {
    ensure!(
        variant <= LAST_STAGE,
        "unsupported Sword Dancer stage {variant}"
    );
    ensure!(
        decision.choices.len() == 9,
        "Sword Dancer requires nine prepared choices"
    );
    ensure!(
        decision.behavior.is_none(),
        "Sword Dancer already has a prepared behavior"
    );
    let model = model.context("Sword Dancer requires its prepared body")?;
    let roots: Vec<&str> = match variant {
        0 => WINGS.into_iter().chain([TAIL]).collect(),
        1 => WINGS.into_iter().collect(),
        _ => Vec::new(),
    };
    let bone_scales = roots
        .into_iter()
        .map(|name| {
            let index = model
                .skeleton
                .bones
                .iter()
                .position(|bone| bone.name == name)
                .with_context(|| format!("Sword Dancer is missing part {name}"))?;
            Ok((u16::try_from(index)?, [0.; 3]))
        })
        .collect::<Result<Vec<_>>>()?;
    decision.disabled_choices = match variant {
        0 => (2..9).collect(),
        1 => (5..8).collect(),
        _ => Vec::new(),
    };
    if variant == 0 {
        decision.choices[8].chain_group = 0;
    }
    if !bone_scales.is_empty() {
        decision.behavior = Some(EnemyBehavior::ModelParts {
            bone_scales,
            hidden_weapons: Vec::new(),
        });
    }
    Ok(())
}
