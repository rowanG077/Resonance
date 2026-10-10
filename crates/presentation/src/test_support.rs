//! Helpers shared by presentation tests.
use resonance_battle::{Actor, Side};

/// A save beside Iselia's save point, after the classroom sequence.
/// Host tests restore it directly; the game integration test owns story playback.
pub(crate) fn field_checkpoint(
    files: &resonance_content::prepared::Files,
) -> anyhow::Result<resonance_game::field::FieldCheckpoint> {
    use resonance_events::{
        SavedProgress,
        camera::{CameraRig, EntryCamera},
        party::Party,
    };
    let (data, _) = crate::new_game::admit_definitions(
        |path| Ok(files.read(path)?.to_vec()),
        files.diagnostics(),
    )?;
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![1, 2, 3];
    let leader = i32::from(party.field_leader);
    let mut progress = SavedProgress::new(party);
    // The field content stores its story counter in global word 16.
    progress.script_globals[16] = 2500;
    progress.event_flags.extend([31, 520]);
    let mut camera = CameraRig::default();
    *camera.current_mut() = EntryCamera::following(leader).camera;
    Ok(resonance_game::field::FieldCheckpoint {
        map_id: 332,
        position: [1968., 1005., 0.],
        heading: 276.,
        camera: Some(camera.settings(leader).map_err(anyhow::Error::msg)?),
        allow_incomplete_scripts: false,
        progress,
        played_ticks: 0,
    })
}

pub(crate) fn actor(side: Side, hp: i32, tp: u16) -> Actor {
    Actor {
        side,
        species: 0,
        equipment: resonance_battle::EquipmentAttributes {
            max_hp: hp,
            max_tp: tp,
            tp_cost_reduction: false,
            quick_escape: false,
            taunt_enabled: false,
            taunt_guard: false,
            taunt_cancel: false,
            control_ex: Default::default(),
            quick_turn: false,
            backstep_guard: false,
            casting: Default::default(),
            dagger_reach: false,
            contact: Default::default(),
            normal_combo_limit: 3,
            luck: 0,
            stats: Default::default(),
            affinities: [resonance_battle::Affinity::Normal; 9],
            damage: Default::default(),
            recovery: Default::default(),
            base_element: None,
            combo_traits: Default::default(),
            normal_guard: false,
            speed_multiplier: 1.,
            reaction_ex: Default::default(),
            stun_ex_bonus: false,
            spell_revenge: false,
        },
        control: Default::default(),
        availability: Default::default(),
        guard: Default::default(),
        hp,
        tp,
        control_ex_state: Default::default(),
        casting_state: Default::default(),
        stored_spell: None,
        overlimit: Default::default(),
        proficiency: 0,
        input: Default::default(),
        control_slot: 0,
        elements: Default::default(),
        attack_power: 100,
        conditions: Default::default(),
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        body: Default::default(),
        movement: Default::default(),
        reaction: Default::default(),
        hit_stop: 0,
        time_stop: 0,
    }
}
