use super::*;

pub(super) fn resources(
    files: &Files,
    character: u8,
    model: &resonance_battle::ModelDefinition,
) -> Result<(Vec<ActionDefinition>, ActionResources)> {
    let mut resources = ActionResources::default();
    let common = EffectResource {
        bank: Some(Arc::new(
            files.json(resonance_content::battle_effect::COMMON_PATH)?,
        )),
        resource: 30,
        members: vec![],
        models: Default::default(),
    };
    let profiles = files.json(resonance_content::battle_profile::PARTY_PATH)?;
    let tints = files.json(resonance_content::battle_effect::TINTS_PATH)?;
    let resolver = battle::voice::Resolver::new(files, &profiles, []);
    let bindings = battle::normal::prepare_resources(
        &resolver,
        &battle::normal::Resources {
            character,
            model: Some(model),
            common: Some(&common),
            tints: Some(&tints),
        },
        &mut resources,
        &mut |request| Ok(Some(request)),
    )?;
    Ok((bindings, resources))
}

pub(super) fn control(
    files: &Files,
    character: u8,
    model: &resonance_battle::ModelDefinition,
) -> Result<Arc<resonance_battle::ControlDefinition>> {
    let profiles: resonance_content::battle_profile::Table =
        files.json(resonance_content::battle_profile::PARTY_PATH)?;
    Ok(Arc::new(battle::control::party(
        &profiles.records[usize::from(character - 1)],
        character,
        std::array::from_fn(resonance_battle::ActionKey),
        battle::control::motions(Some(model)),
    )?))
}
