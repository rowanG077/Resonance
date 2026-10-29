use super::*;
use crate::library::{Library, Mod};
use serde_json::json;
use std::fs;

#[test]
fn menu_text_preserves_literal_prose_colors_buttons_and_empty_lines() -> Result<()> {
    let literal = "あ {plain text} \\ \"quoted\"\n\nlast line\n";
    let text: MenuText = serde_json::from_value(json!(literal))?;
    assert_eq!(text.lines.len(), 4);
    assert_eq!(serde_json::to_value(&text)?, json!(literal));
    let value = json!(["Press ", {"button":"a_pressed"}, "\n", {"color":"gold"},
        "Remaining:", {"color":"white"}, "%d"]);
    let text: MenuText = serde_json::from_value(value.clone())?;
    assert_eq!(
        text.lines,
        vec![
            vec![
                MenuSpan::Text {
                    text: "Press ".into(),
                    color: 9
                },
                MenuSpan::Button { sprite: 6 }
            ],
            vec![
                MenuSpan::Text {
                    text: "Remaining:".into(),
                    color: 8
                },
                MenuSpan::Text {
                    text: "%d".into(),
                    color: 9
                }
            ],
        ]
    );
    assert_eq!(serde_json::to_value(text)?, value);
    for bad in [
        json!({"lines":[]}),
        json!([{"button":6}]),
        json!([{"button":"missing"}]),
        json!([{"color":"missing"}]),
        json!([{"color":"red", "button":"a"}]),
        json!([{"kind":"text", "text":"old", "color":9}]),
        json!("tab\tcontrol"),
    ] {
        assert!(
            serde_json::from_value::<MenuText>(bad.clone()).is_err(),
            "accepted {bad}"
        );
    }
    assert!(serde_json::from_value::<SynopsisLine>(json!([{"button":"a"}])).is_err());
    assert!(serde_json::from_value::<SynopsisLine>(json!("two\nlines")).is_err());
    Ok(())
}

#[test]
fn menu_prose_mods_and_synopsis_updates_preserve_snapshot_and_fallback_behavior() -> Result<()> {
    let base = tempfile::tempdir()?;
    let overlay = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(&json!({
            "version": crate::library::VERSION, "game":"tales-of-symphonia"
        }))?,
    )?;
    let name = "synopsis.json";
    fs::write(
        base.path().join(name),
        serde_json::to_vec(&json!({
            "heading":"Journey", "title":"Departure", "text":["At home.",""],
            "updates":{"3":["Back again."]}
        }))?,
    )?;
    let library = Library::open(
        base.path(),
        &[Mod {
            name: "prose".into(),
            root: overlay.path().into(),
        }],
    )?;
    let mut retained = library.snapshot();
    let original: SynopsisEntry = retained.json(name, crate::preparation::JSON_LIMIT)?;
    assert_eq!(original.lines(0), original.lines(2));
    assert_eq!(original.lines(1), original.lines(255));
    assert_ne!(original.lines(1), original.lines(3));
    fs::write(
        overlay.path().join(name),
        serde_json::to_vec(&json!({
            "heading":"Journey", "title":"Departure", "text":["A new beginning."],
            "updates":{"2":[[{"color":"green"},"On the road."]],"3":[]}
        }))?,
    )?;
    let changed: SynopsisEntry = library
        .snapshot()
        .json(name, crate::preparation::JSON_LIMIT)?;
    assert_eq!(changed.lines(1)[0].0[0].text, "A new beginning.");
    assert_eq!(changed.lines(2)[0].0[0].color, 4);
    assert!(changed.lines(3).is_empty());
    let old: SynopsisEntry = retained.json(name, crate::preparation::JSON_LIMIT)?;
    assert_eq!(old.lines(1), original.lines(1));
    let mut data = SynopsisData {
        entries: vec![changed; SYNOPSIS_COUNT],
        months: std::array::from_fn(|_| "month".into()),
    };
    data.validate()?;
    data.entries[0].updates.insert(4, Vec::new());
    assert!(data.validate().is_err());
    Ok(())
}
