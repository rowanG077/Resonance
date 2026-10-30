use super::*;

#[test]
fn named_initial_ex_bindings_and_defaults_are_explicit() -> Result<()> {
    let document: Document = serde_json::from_value(serde_json::json!({
        "mappings":{"1":"strong","54":"ex-attack"},
        "skills":{"strong":{"tendency":"strike","text":{"name":"Strong","description":"Power"}},"ex-attack":{"text":{"name":"EX Attack","description":"Guard"}}},
        "characters":{"lloyd":{"levels":{},"compounds":{"0":"ex-attack"}}},
        "gem_items":{"1":"gem1","2":"gem2","3":"gem3","4":"gem4","max":"max"},
        "labels":{},"activation_labels":{}
    }))?;
    assert_eq!(document.skill_id("strong")?, 1);
    assert_eq!(document.compound_id(PartyCharacter::Lloyd, "ex-attack")?, 0);
    assert!(document.skill_id("missing").is_err());
    assert!(
        document
            .compound_id(PartyCharacter::Colette, "ex-attack")
            .is_err()
    );
    let strong = &document.skills["strong"];
    assert!(strong.stat_bonuses.is_empty());
    assert_eq!(strong.activation, runtime::ExActivation::Constant);
    let json = serde_json::to_value(strong)?;
    assert!(
        json.get("activation").is_none()
            && json.get("stat_bonuses").is_none()
            && json.get("combinations").is_none()
    );
    Ok(())
}
