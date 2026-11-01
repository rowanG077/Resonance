//! Replay controller changes recorded by the single-action capture fixture.
use super::{Trace, field, random_inputs::Pin};
use anyhow::{Context as ContextExt, Result, ensure};
use serde_json::Value;

pub(super) fn neutral_after_admission(source: &Trace) -> Result<bool> {
    let Some(capture) = source.setup.get("capture") else {
        return Ok(false);
    };
    let receipt: Value =
        serde_json::from_slice(&serde_json::from_value::<Pin>(capture.clone())?.read_bytes()?)?;
    let Some(boundary) = receipt.get("single_action_control") else {
        return Ok(false);
    };
    validate_boundary(source, boundary)?;
    Ok(true)
}

fn validate_boundary(source: &Trace, boundary: &Value) -> Result<()> {
    let owner: usize = field(&source.setup, "owner")?;
    let slot: u64 = field(&source.setup["source_actor_map"][owner], "source_slot")?;
    let actor = source.setup["initial_battle"]["actors"]
        .as_array()
        .context("missing initial actors")?
        .iter()
        .find(|actor| actor["slot"] == slot)
        .context("missing source owner")?;
    let address: u64 = field(actor, "address")?;
    let first = source.frames.first().context("missing admitted frame")?;
    ensure!(
        source.setup["actors"][owner]["side"] == 1
            && boundary["source_owner"] == slot
            && boundary["presentation_tick"] == first["presentation_tick"]
            && boundary["combat_tick"] == first["combat_tick"],
        "neutral controller change is not at the enemy admission boundary"
    );
    let changes = boundary["changes"]
        .as_array()
        .context("missing control changes")?;
    ensure!(
        changes.len() == 2,
        "unexpected single-action fixture changes"
    );
    for (change, (offset, clear, set)) in changes
        .iter()
        .zip([(0x107c, 0x38_u8, 0_u8), (0x107d, 0xc0_u8, 0xc0_u8)])
    {
        let before: u8 = serde_json::from_value(change[2].clone())?;
        ensure!(
            change.as_array().is_some_and(|values| values.len() == 4)
                && change[0] == format!("{:#x}", address + offset)
                && change[1] == "B"
                && change[3] == ((before & !clear) | set),
            "single-action fixture changed more than neutral controller input"
        );
    }
    Ok(())
}

#[test]
fn controller_boundary_rejects_wrong_actor_time_and_unrelated_changes() {
    use serde_json::json;
    let source = Trace {
        case: "enemy:46:2".into(),
        setup: json!({"owner":0,"actors":[{"side":1}],
            "source_actor_map":[{"source_slot":6}],
            "initial_battle":{"actors":[{"slot":6,"address":0x812e8480_u64}]}}),
        frames: vec![json!({"presentation_tick":89,"combat_tick":30})],
        events: vec![],
        assessment: Value::Null,
        incomplete_observations: vec![],
    };
    let boundary = json!({"source_owner":6,"presentation_tick":89,"combat_tick":30,
        "changes":[["0x812e94fc","B",218,194],["0x812e94fd","B",155,219]]});
    validate_boundary(&source, &boundary).unwrap();
    for (key, value) in [
        ("source_owner", 5),
        ("presentation_tick", 90),
        ("combat_tick", 31),
    ] {
        let mut changed = boundary.clone();
        changed[key] = json!(value);
        assert!(validate_boundary(&source, &changed).is_err());
    }
    let mut changed = boundary;
    changed["changes"][0][3] = json!(195);
    assert!(validate_boundary(&source, &changed).is_err());
}
