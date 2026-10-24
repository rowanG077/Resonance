use super::*;

pub(super) fn resources(
    files: &Files,
    character: u8,
    model: &resonance_battle::ModelDefinition,
) -> Result<(Vec<ActionDefinition>, ActionResources)> {
    let mut resources = ActionResources::default();
    let common = EffectResource {
        bank: Some(Arc::new(resonance_content::battle_effect::document::read(
            resonance_content::battle_effect::document::COMMON,
            files.diagnostics(),
            |path| files.read(path),
        )?)),
        resource: resonance_battle::ResourceId(30),
        members: vec![],
        models: Default::default(),
    };
    let profiles = resonance_content::battle_rules::party(&files)?;
    let tints = files.json(resonance_content::battle_effect::TINTS_PATH)?;
    let resolver = battle::voice::Resolver::new(files, &profiles, []);
    let recoil = battle::recoil::Parameters::load(files)?;
    let bindings = battle::normal::prepare_resources(
        &resolver,
        &battle::normal::Resources {
            character: character.try_into()?,
            recoil: &recoil,
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
        resonance_content::battle_rules::party(&files)?;
    Ok(Arc::new(battle::control::party(
        &profiles.records[usize::from(character - 1)],
        &profiles.normal_attacks[usize::from(character - 1)],
        std::array::from_fn(resonance_battle::ActionKey),
        battle::control::motions(Some(model)),
    )?))
}
