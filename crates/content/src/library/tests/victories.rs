use super::Fixture;
use crate::{
    battle_victory::{Event, document::*},
    diagnostics::Diagnostics,
};
use anyhow::Result;
use serde_json::json;

#[test]
fn shared_choreography_edits_keep_costume_selection_and_optional_failures_local() -> Result<()> {
    let fixture = Fixture::new();
    let write = |path: &str, value: &serde_json::Value| -> Result<()> {
        fixture.write(&format!("base/{path}"), &serde_json::to_vec(value)?);
        Ok(())
    };
    write(GROUPS, &json!([]))?;
    fixture.write("base/default.glb", b"default clip");
    fixture.write("base/formal.glb", b"formal clip");
    let path = "battle/party/hero/victory.json";
    let mut definition = json!({
        "costumes": {
            "default": {"healthy-party": {"motion": "default.glb", "timeline": "salute"},
                        "injured-leader": {"motion": "default.glb", "timeline": "cheer"}},
            "formal": {"healthy-party": {"motion": "formal.glb", "timeline": "salute"}},
            "unused": "a malformed unused costume"
        },
        "timelines": {
            "salute": {"events": [[0, {"pose": {"rate": 0}}], [20, {"idle": {}}]]},
            "cheer": {"events": [[0, {"expression": [1, 0, 0, 0]}]]},
            "unused": "a malformed unused timeline"
        }
    });
    write(path, &definition)?;
    let library = fixture.open(&[])?;
    let default = Selection {
        file: path.into(),
        costume: "default".into(),
    };
    let formal = Selection {
        file: path.into(),
        costume: "formal".into(),
    };
    let selections = [(1, 0, &default), (1, 3, &formal)];
    let strict = Diagnostics::new(true);
    let mut retained = library.snapshot();
    let before = capture(&mut retained, selections, &strict, &|| false)?;
    assert_eq!(before.ordinary.len(), 3);
    assert_eq!(before.ordinary[0].timeline, before.ordinary[2].timeline);
    assert_ne!(before.ordinary[0].motion, before.ordinary[2].motion);
    assert!(matches!(
        before.ordinary[0].timeline.as_ref().unwrap()[0].1,
        Event::Pose {
            rate: 0.,
            start: 0.,
            end: None,
            blend: 0,
            repeat: false
        }
    ));

    definition["timelines"]["salute"]["events"][1][0] = 45.into();
    write(path, &definition)?;
    let edited = capture(&mut library.snapshot(), selections, &strict, &|| false)?;
    for index in [0, 2] {
        assert_eq!(edited.ordinary[index].timeline.as_ref().unwrap()[1].0, 45);
    }
    assert_eq!(edited.ordinary[1].timeline, before.ordinary[1].timeline);
    let kept = capture(&mut retained, selections, &strict, &|| false)?;
    assert_eq!(kept.ordinary[0].timeline, before.ordinary[0].timeline);

    definition["timelines"]["salute"] = "broken shared timeline".into();
    write(path, &definition)?;
    let tolerant = Diagnostics::default();
    let partial = capture(&mut library.snapshot(), selections, &tolerant, &|| false)?;
    assert_eq!(partial.ordinary.len(), 1);
    assert_eq!(partial.ordinary[0].selector, 1);
    assert_eq!(partial.ordinary[0].costume, Some(0));
    assert_eq!(tolerant.entries().len(), 2);
    assert!(capture(&mut library.snapshot(), selections, &strict, &|| false).is_err());
    assert!(capture(&mut retained, selections, &strict, &|| true).is_err());
    Ok(())
}
