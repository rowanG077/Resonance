use super::*;
use crate::{
    SceneClip, ScenePart,
    battle::{
        effects::EffectBank,
        pose::{Bone, Motion, Skeleton, Transform},
        visual::Rig,
    },
    model_preview::{ModelPreview, PreviewPart},
};

fn content(binding: ModelRef, external_animation: bool) -> BattleEffectPrograms {
    let id = EffectId {
        bank: EffectBank::Magic(37),
        id: 0,
    };
    BattleEffectPrograms {
        actors: vec![EffectActor {
            id,
            geometry: Geometry::Model {
                model: binding,
                animation: (!external_animation).then_some(0),
                loop_animation: true,
                presentation: ModelPresentation {
                    external_animation,
                    ..Default::default()
                },
            },
            material: None,
            palette: None,
            screen_texture: None,
            element_variants: vec![],
            use_element_variant: false,
            blend: Blend::Alpha,
            blend_variants: Default::default(),
            uv: [0; 4],
            uv_animation: None,
            depth_test: true,
            depth_write: true,
            cull_back: false,
            owner_layer: None,
            during_pause: false,
            retained: false,
            lifetime: Some(30),
            orientation: Orientation::World,
            space: EffectSpace::World,
            follow_emitter: false,
            bottom_anchored: false,
            ground_relative: false,
            ground: None,
            periodic: None,
            position: [0.; 3],
            velocity: [0.; 3],
            acceleration: [0.; 3],
            acceleration_change: [0.; 3],
            angles: [0.; 3],
            angular_velocity: [0.; 3],
            local_offset: [0.; 3],
            local_velocity: [0.; 3],
            dimensions: [1.; 3],
            dimension_velocity: [0.; 3],
            dimension_acceleration: [0.; 3],
            dimension_acceleration_until: None,
            colors: [[255; 4]; 2],
            color_gradient: false,
            brighten: [0; 4],
            darken: [0; 4],
            brighten_until: 0,
            darken_from: 0,
            copies: 1,
            copy_rotation: 0,
        }],
        programs: vec![EffectProgram {
            id,
            end_tick: 1,
            emissions: vec![EffectEmission {
                tick: 0,
                repeat: None,
                command: EffectCommand::Particle {
                    actor: id,
                    attachment: Attachment::Emitter,
                    modifiers: vec![],
                },
            }],
        }],
        materials: vec![],
    }
}

fn model(binding: ModelRef, slots: &[u16]) -> EffectModel {
    let part = PreviewPart {
        animation: None,
        scene: ScenePart {
            resource: 0,
            mesh: "effect.glb".into(),
            textures: vec![],
            materials: vec![],
            appearance: None,
            translation: [0.; 3],
            clips: slots
                .iter()
                .map(|&resource_slot| SceneClip {
                    resource_slot,
                    duration_seconds: 1.,
                    animation_resource: None,
                    secondary_pose_nodes: vec![],
                })
                .collect(),
            autoplay: false,
            texture_animations: vec![],
            bone_names: vec!["root".into()],
            material_nodes: vec![],
            outline_color: None,
            secondary_motion: Default::default(),
        },
        attached_to: None,
        additive: false,
        uv_offsets: vec![],
    };
    let mut outline = part.clone();
    outline.scene.outline_color = Some([0, 0, 0, 255]);
    EffectModel {
        binding,
        pose_joints: vec![vec![0], vec![0]],
        model: ModelPreview {
            scale: 1.,
            elevation: 0.,
            parts: vec![part, outline],
            hidden_geometry: vec![],
            node_scales: vec![],
        },
        rig: Some(Rig {
            skeleton: Skeleton {
                bones: vec![Bone {
                    bind_channels: Default::default(),
                    name: "root".into(),
                    parent: None,
                    bind: Transform::default(),
                }],
            },
            motions: slots
                .iter()
                .map(|&slot| {
                    (
                        slot,
                        Motion {
                            duration_frames: 30.,
                            tracks: vec![],
                        },
                    )
                })
                .collect(),
            attack_groups: Default::default(),
            effect_groups: Default::default(),
            weapon_bones: Default::default(),
        }),
    }
}

fn birth(content: &mut BattleEffectPrograms) -> &mut Vec<Modifier> {
    let EffectCommand::Particle { modifiers, .. } = &mut content.programs[0].emissions[0].command
    else {
        unreachable!()
    };
    modifiers
}

fn select(index: u8) -> Modifier {
    Modifier::Byte {
        field: ByteField::ModelIndex,
        operation: Arithmetic::Set,
        value: IntegerValue::Constant(i16::from(index)),
    }
}

fn play(model: u8, clip: u8) -> Modifier {
    Modifier::PlayModelAnimation {
        animation: ModelAnimation {
            model,
            clip,
            blend_ticks: 0,
            rate: 0.5,
            hold: false,
        },
    }
}

#[test]
fn replacement_models_require_playable_clips_on_every_layer_before_startup() {
    let binding = ModelRef::EnemyAnimated {
        monster: 49,
        index: 0,
        animation_model: 0,
    };
    let mut content = content(binding, false);
    birth(&mut content).push(select(1));
    content.validate().unwrap();
    let mut prepared = vec![model(binding, &[0]), model(binding.with_index(1), &[0])];
    content.validate_models(&prepared).unwrap();
    assert!(
        content
            .validate_models(&prepared[..1])
            .unwrap_err()
            .to_string()
            .contains("uncooked battle effect model")
    );
    prepared[1].model.parts[1].scene.clips.clear();
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("layer 1 lacks playable clip 0")
    );
    prepared[1] = model(binding.with_index(1), &[0]);
    prepared[1].model.parts[0].scene.clips[0].duration_seconds = 0.;
    assert!(content.validate_models(&prepared).is_err());
}

#[test]
fn external_replacements_and_retained_clips_require_their_cooked_rigs() {
    let binding = ModelRef::Magic {
        package: 37,
        index: 0,
    };
    let mut content = content(binding, true);
    birth(&mut content).extend([select(1), play(1, 2)]);
    content.validate().unwrap();
    let mut prepared = vec![model(binding, &[]), model(binding.with_index(1), &[2])];
    content.validate_models(&prepared).unwrap();
    prepared[1].rig = None;
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("required external rig")
    );
    prepared[1] = model(binding.with_index(1), &[2]);
    prepared[1].rig.as_mut().unwrap().motions.clear();
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("external rig lacks clip 2")
    );
    prepared[1] = model(binding.with_index(1), &[2]);
    prepared[1].model.parts[1].scene.clips.clear();
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("layer 1 lacks playable clip 2")
    );

    content.actors[0].retained = true;
    *birth(&mut content) = vec![play(0, 0)];
    content.programs[0].emissions.push(EffectEmission {
        tick: 1,
        repeat: None,
        command: EffectCommand::ModifyRetained {
            slot: 0,
            modifiers: vec![play(0, 2)],
        },
    });
    content.validate().unwrap();
    let mut prepared = vec![model(binding, &[0, 2])];
    content.validate_models(&prepared).unwrap();
    prepared[0].rig.as_mut().unwrap().motions.remove(&2);
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("external rig lacks clip 2")
    );
}

#[test]
fn colette_weapon_defers_equipment_but_not_archive_animation_requirements() {
    let mut content = content(ModelRef::ColetteWeapon, false);
    assert!(content.validate_models(&[]).is_err());
    let Geometry::Model { animation, .. } = &mut content.actors[0].geometry else {
        unreachable!()
    };
    *animation = None;
    content.validate_models(&[]).unwrap();
    let Geometry::Model { presentation, .. } = &mut content.actors[0].geometry else {
        unreachable!()
    };
    presentation.external_animation = true;
    assert!(content.validate_models(&[]).is_err());
}

#[test]
fn pose_joint_mapping_preserves_outline_names_and_requires_complete_ordered_indices() {
    let binding = ModelRef::Magic {
        package: 92,
        index: 0,
    };
    let mut content = content(binding, true);
    birth(&mut content).push(play(0, 0));
    let mut prepared = vec![model(binding, &[0])];
    prepared[0].model.parts[1].scene.bone_names[0] = "ROOT".into();
    content.validate_models(&prepared).unwrap();
    prepared[0].pose_joints[1].clear();
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("pose joint mapping")
    );
    prepared[0].pose_joints[1] = vec![1];
    assert!(content.validate_models(&prepared).is_err());
    prepared[0].pose_joints[1] = vec![0];
    prepared[0].rig.as_mut().unwrap().skeleton.bones.push(Bone {
        bind_channels: Default::default(),
        name: "child".into(),
        parent: Some(0),
        bind: Transform::default(),
    });
    for part in &mut prepared[0].model.parts {
        part.scene.bone_names.push("child".into());
    }
    prepared[0].pose_joints = vec![vec![0, 1], vec![0, 1]];
    content.validate_models(&prepared).unwrap();
    prepared[0].pose_joints[0].swap(0, 1);
    assert!(
        content
            .validate_models(&prepared)
            .unwrap_err()
            .to_string()
            .contains("unordered primary joints")
    );
    prepared[0].pose_joints.clear();
    assert!(content.validate_models(&prepared).is_err());
}
