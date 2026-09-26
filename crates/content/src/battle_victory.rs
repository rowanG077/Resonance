//! Original victory motion packages and the regional group-voice descriptors.
//! Selection and playback control flow remain maintained battle scripts.
use crate::{battle_action::Animation, field_preload::File};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PATH: &str = "battle/victory.json";
pub const OPENING_GROUPS: [u8; 6] = [5, 15, 24, 25, 26, 27];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performances {
    pub module_sha256: String,
    pub archive_sha256: String,
    pub group_archive_sha256: String,
    pub camera: Camera,
    pub ordinary: Vec<Ordinary>,
    pub postures: Vec<Posture>,
    pub groups: Vec<Group>,
    pub files: BTreeMap<String, File>,
}

/// Ordinary result camera operands from 57718 / 564D4.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub initial_radius: f32,
    pub minimum_radius: f32,
    pub additional_radius: f32,
    pub pitch: f32,
    pub group_angle: f32,
    pub angular_step: f32,
    pub contraction: f32,
    pub degrees_to_radians: f32,
    pub focus_height: f32,
    /// Rows for two, three and four performers, including remaining party slots.
    pub placement: [[[f32; 3]; 4]; 3],
    pub circle_radius: f32,
    pub circle_extra_degrees: f32,
    pub circle_full_degrees: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Posture {
    pub character: u8,
    pub rate: f32,
    pub healthy_expression: [u8; 4],
    pub weak_expression: [u8; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ordinary {
    pub character: u8,
    pub selector: u8,
    pub source_sha256: String,
    pub body_sha256: String,
    /// Sparse clip replacing original motion slot 23 for this performance.
    pub motion: String,
    pub animations: Vec<Animation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: u8,
    pub character: u8,
    pub voice_command: u16,
    pub source_sha256: String,
    pub descriptor: [u8; 8],
}
