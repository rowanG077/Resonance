use super::*;
use crate::battle_effect::document::Definition;
use serde_json::{Value, json};

fn read(program: Value) -> Result<Vec<ScheduledEvent>> {
    let definition: Definition = serde_json::from_value(
        json!({"programs":[program],"templates":{"first":{"kind":"unsupported","reason":"test"},"second":{"kind":"unsupported","reason":"test"}}}),
    )?;
    Ok(definition.programs.into_iter().next().unwrap())
}

fn write(program: Vec<ScheduledEvent>) -> Result<Value> {
    let definition = Definition {
        programs: vec![program],
        templates: vec![
            crate::battle_effect::declaration::Declaration::Unsupported {
                reason: "test".into()
            };
            2
        ],
        template_names: vec!["first".into(), "second".into()],
        artwork: None,
    };
    let value: Value = serde_json::from_slice(&serde_json::to_vec(&definition)?)?;
    Ok(value["programs"][0].clone())
}

#[test]
fn repetitions_keep_fresh_and_shared_randomness_and_previous_particle_targets() -> Result<()> {
    let mut authored = json!([{"at":2,"repeat":3,"every":5,"events":[
        {"kind":"spawn","particle":"first","birth":{"random_draws":2,
            "offset":[{"min":0,"max":1,"draw":1},null,null],
            "size":[{"min":10,"max":20,"draw":1},null,null]}},
        {"at":2,"kind":"spawn","particle":"second","birth":{
            "velocity":[{"min":0,"max":1,"draw":-2},null,null],
            "model_parent":{"previous_spawn":1,"joint":3}}},
        {"at":3,"kind":"set_motion","previous_spawn":2,"velocity":[null,2,null]}
    ]}]);
    for repetitions in [3, 4] {
        authored[0]["repeat"] = repetitions.into();
        let events = read(authored.clone())?;
        assert_eq!(events.len(), repetitions as usize * 3);
        for (iteration, group) in events.chunks_exact(3).enumerate() {
            assert_eq!(
                group.iter().map(|e| e.at).collect::<Vec<_>>(),
                [2, 4, 5].map(|t| t + iteration as u32 * 5)
            );
            let EffectOperation::Spawn { birth: first, .. } = &group[0].operation else {
                panic!()
            };
            let EffectOperation::Spawn { birth: next, .. } = &group[1].operation else {
                panic!()
            };
            assert_eq!(
                first.offset[0].unwrap().draw,
                Some(iteration as i64 * 2 + 1)
            );
            assert_eq!(first.offset[0].unwrap().draw, first.size[0].unwrap().draw);
            assert_eq!(next.velocity[0].unwrap().draw, Some(iteration as i64 * 2));
            assert_eq!(next.model_parent.unwrap().spawn, iteration * 2);
            let EffectOperation::SetMotion { spawn, .. } = group[2].operation else {
                panic!()
            };
            assert_eq!(spawn, iteration * 2);
        }
        let compact = write(events.clone())?;
        assert_eq!(compact.as_array().unwrap().len(), 1);
        assert_eq!(compact[0]["repeat"], repetitions);
        assert_eq!(read(compact)?, events);
    }
    Ok(())
}

#[test]
fn simultaneous_bursts_and_interleaved_groups_keep_authored_order() -> Result<()> {
    let authored = json!([
        {"repeat":3,"every":2,"events":[
            {"repeat":3,"kind":"notice","text":"A"},
            {"at":1,"kind":"notice","text":"B"}]},
        {"at":2,"kind":"notice","text":"C"},
        {"at":2,"repeat":2,"kind":"notice","text":"D"}
    ]);
    let events = read(authored)?;
    let texts: Vec<_> = events
        .iter()
        .map(|e| match &e.operation {
            EffectOperation::Notice { text } => (e.at, text.as_str()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        texts,
        [
            (0, "A"),
            (0, "A"),
            (0, "A"),
            (1, "B"),
            (2, "A"),
            (2, "A"),
            (2, "A"),
            (2, "C"),
            (2, "D"),
            (2, "D"),
            (3, "B"),
            (4, "A"),
            (4, "A"),
            (4, "A"),
            (5, "B")
        ]
    );
    assert_eq!(read(write(events.clone())?)?, events);
    Ok(())
}

#[test]
fn invalid_repetition_and_references_fail_without_unbounded_expansion() {
    for program in [
        json!([{"kind":"notice","text":"x","repeat":0}]),
        json!([{"kind":"notice","text":"x","repeat":u32::MAX}]),
        json!([{"kind":"notice","text":"x","repeat":3,"every":u32::MAX}]),
        json!([{"repeat":u32::MAX,"events":[{"kind":"notice","text":"x"}]}]),
        json!([{"repeat":1,"events":[]}]),
        json!([{"kind":"notice","text":"x","typo":1}]),
        json!([{"at":0,"operation":{"kind":"notice","text":"old wrapper"}}]),
        json!([{"kind":"set_motion","previous_spawn":0}]),
        json!([{"kind":"set_motion","previous_spawn":1}]),
        json!([{"kind":"spawn","particle":"first","birth":{"offset":[{"min":0,"max":1,"draw":-1},null,null]}}]),
        json!([{"kind":"spawn","particle":"first","birth":{"random_draws":1,"offset":[{"min":0,"max":1,"draw":1},null,null]}}]),
    ] {
        assert!(read(program.clone()).is_err(), "accepted {program}");
    }
}
