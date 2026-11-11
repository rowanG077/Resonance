use super::*;
use resonance_content::session::SessionData;
use serde_json::json;

fn party() -> Result<(Party, SessionData)> {
    let items = [None, Some(0), Some(3), Some(1), Some(0), Some(0)].map(|kind| {
        json!({
            "equipment_kind":kind,"allowed_characters":511,"stack_limit":20
        })
    });
    let data: SessionData = serde_json::from_value(json!({
        "version":1,"experience":[0,0],
        "items": items,
        "characters": vec![json!({
            "affinity":0,"level":1,"experience":0,"base_stats":[1,0,0,0,0,0,0],
            "luck":0,"overlimit":0,"equipment":vec![0;6],"shortcuts":vec![0;4],
            "techniques":[],
            "growth":vec![json!({"base":0,"random":0,"title_bonus":0});7]
        });9]
    }))?;
    Ok((Party::new(&data, Default::default())?, data))
}

#[test]
fn only_equipped_visual_items_and_selected_costumes_are_retained() -> Result<()> {
    let (mut party, data) = party()?;
    party.formation = vec![1, 6];
    party.members[0].costume = 1;
    party.members[0].equipment[0] = 1;
    party.members[0].equipment[5] = 3; // Body armour is not a carried shield.
    party.members[1].costume = 2;
    party.members[1].equipment[0] = 4;
    party.members[5].equipment[5] = 2;
    party.items.insert(5, 1); // Owning an item does not warm it.
    assert_eq!(
        selected(&party, &data, 0..party.members.len()),
        [
            Key::Costume(1, 1),
            Key::Costume(2, 2),
            Key::Equipment(1),
            Key::Equipment(2),
            Key::Equipment(4),
        ]
        .into()
    );
    assert_eq!(
        required(&party, &data),
        [Key::Costume(1, 1), Key::Equipment(1), Key::Equipment(2),].into()
    );
    party.members[0].costume = 2;
    party.members[0].equipment[0] = 5;
    let next = selected(&party, &data, 0..party.members.len());
    assert!(!next.contains(&Key::Costume(1, 1)));
    assert!(!next.contains(&Key::Equipment(1)));
    assert!(next.contains(&Key::Costume(1, 2)));
    assert!(next.contains(&Key::Equipment(5)));
    Ok(())
}

#[test]
fn admission_preserves_the_in_flight_job_and_reports_only_required_failures() -> Result<()> {
    let key = Key::Costume(1, 1);
    let (release, held) = std::sync::mpsc::channel();
    let task = Task::spawn("held selected costume", move |_| -> Result<Prepared> {
        held.recv()?;
        anyhow::bail!("injected costume failure")
    })?;
    let mut state = Selected {
        pending: Some(Pending::Cpu { key, task }),
        ..Default::default()
    };
    state
        .failed
        .insert(Key::Equipment(99), "unused broken equipment".into());
    let required = BTreeSet::from([key]);
    for _ in 0..3 {
        assert!(!state.ready(&required)?);
        assert!(
            matches!(&state.pending, Some(Pending::Cpu { key: pending, task }) if *pending == key && !task.cancelled.load(Ordering::Relaxed))
        );
    }
    assert!(state.ready(&BTreeSet::new())?);
    // A required failure cannot hide behind an earlier still-pending costume.
    state
        .failed
        .insert(Key::Equipment(1), "selected shield failed to upload".into());
    let error = state.ready(&[key, Key::Equipment(1)].into()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("selected shield failed to upload")
    );
    release.send(())?;
    Ok(())
}
