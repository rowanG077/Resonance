//! Version-specific table roots; table decoding is shared by every module.
use super::{
    all::SourceLayout,
    archive_directories::ArchiveLayout,
    entrance::EntranceLayout,
    message_tables::MessageLayout,
    motion::MotionLayout,
    placement::PlacementLayout,
    ui_tables::UiLayout,
    unison_tables::{UnisonLayout, UnisonPresentationLayout},
};
pub(super) use crate::embedded::write;
use std::path::Path;

pub(super) const PARTY_COUNT: u8 = 9;
pub(super) const SETTINGS_BYTES: usize = 496;

pub(super) struct Layout {
    pub normal_actions: usize,
    pub normal_groups: u8,
    pub party_settings: usize,
    pub party_settings_end: usize,
    pub casting_voices: usize,
    pub casting_voices_end: usize,
    pub casting_programs: super::casting_programs::Layout,
    pub victory_groups: usize,
    pub entrance: EntranceLayout,
    pub contact_colors: usize,
    pub contact_effects: usize,
    pub contact_sounds: usize,
    pub party_contact_sounds: usize,
    pub item_throw_origins: [usize; 2],
    pub tint_palette: usize,
    pub archives: ArchiveLayout,
    pub sources: SourceLayout,
    pub placement: PlacementLayout,
    pub motion: MotionLayout,
    pub ui: UiLayout,
    pub messages: MessageLayout,
    pub unison: UnisonLayout,
    pub unison_opener: usize,
}

impl Layout {
    pub const RETAIL: Self = Self {
        unison_opener: 0x960,
        normal_actions: 0x3a48,
        normal_groups: 9,
        party_settings: 0x3d30,
        party_settings_end: 0x5280,
        casting_voices: 0x59d8,
        casting_voices_end: 0x5a00,
        casting_programs: super::casting_programs::Layout {
            animation: 0x11e0,
            commands: 0x1210,
            end: 0x1238,
        },
        victory_groups: 0x5ea8,
        entrance: EntranceLayout {
            geometry: 0xc0,
            native_size: 0x4f0,
            rotation: 0x53c,
            center: 0x548,
            origin: [0x4d8, 0x550],
            angular: 0x554,
            zero: 0x4e4,
        },
        contact_colors: 0x13d0,
        contact_effects: 0x13f8,
        contact_sounds: 0x1404,
        party_contact_sounds: 0x1d30,
        item_throw_origins: [0xc3c, 0xca8],
        tint_palette: 0x1564,
        archives: ArchiveLayout {
            magic: 0xfe0,
            skill: 0x11c4,
            arena: 0x3b90,
            weapon: 0x6e8,
            weapon_slots: 158,
        },
        sources: SourceLayout {
            enemy: (4, 0x2404),
            archives: [(4, 0x1f34), (4, 0x1f4c), (4, 0x24b4), (4, 0xdfc)],
        },
        placement: PlacementLayout {
            party_points: 0x1084,
            enemy_points: 0x2350,
            strategies: 0x10dc,
            strategy_columns: 10,
            party_scalars: [
                0x1130, 0x1104, 0x1134, 0x1138, 0x113c, 0x1140, 0x1144, 0x10fc,
            ],
            enemy_scalars: [
                0x23c8, 0x23cc, 0x23d0, 0x23d8, 0x23dc, 0x23e0, 0x23e4, 0x23d4,
            ],
        },
        motion: MotionLayout {
            reactions: 0x5600,
            projectile_motions: [0x1cbc, 0x1cec],
            acceleration: 0x15ac,
            action_drag: 0x1e84,
            drag_thresholds: [0x1654, 0x1658],
            stop_epsilon: 0x1660,
            projectile_velocity_reset_scale: 0xce8,
        },
        ui: UiLayout {
            punctuation: [0x26e0, 0x271c],
            party_icons: 0x4450,
            party_colors: 0x4180,
            gauge_bonus: 0x4290,
            combo: 0x4308,
            damage: 0x4350,
            notices: 0x4374,
            result_positions: 0x2d94,
            result_colors: 0x2db0,
            texture_bindings: 0xd8,
            scan: 0x4c0,
            unison_palettes: 0xe74,
            strategy: 0x4214,
            roster: 0x4240,
            leader: 0x42c4,
            marker: 0x29c8,
            marker_scale: [0x2a28, 0x2a2c, 0x2a30, 0x2a34],
            marker_follow_speed: 0x4558,
        },
        messages: MessageLayout {
            notices: 0x14f4,
            hud: [0x220, 0x2c3c, 0x4584, 0x43b4, 0x4500],
            extra_notices: [
                [0x1690, 0x1698],
                [0x6598, 0x65a0],
                [0x6600, 0x6604],
                [0x6f60, 0x6f68],
                [0x1494, 0x15c4],
            ],
            result_section: 4,
            steal: [0x508, 0x50c, 0x514, 0x520, 0x528, 0x530],
            steal_prefix: true,
            result_messages: [
                0x2f44, 0x2f30, 0x2f70, 0x2f5c, 0x2f88, 0x2f9c, 0x2fb8, 0x2fcc, 0x2e0c, 0x2e28,
                0x3018,
            ],
            results: [
                0x2e60, 0x2e6c, 0x2e78, 0x2e88, 0x2e94, 0x2eac, 0x2ec0, 0x2ed4, 0x2ee4, 0x2eec,
                0x2efc, 0x2f04,
            ],
        },
        unison: UnisonLayout {
            opener_contact_delays: [0xe9c, 0xea8],
            placement: 0xec8,
            combined_voices: [0xefc, 0xf10],
            overlimit_voices: [0x1264, 0x1278],
            presentation: UnisonPresentationLayout {
                windup_rate: 0xf44,
                windup_color: 0xe60,
                combined_color: 0xef8,
                hidden_position: [0xf28, 0xf2c],
                camera_eye: [0xf30, 0xf34],
                camera_target_y: 0xf38,
                first_x: 0xf3c,
                spacing: 0xf40,
                short_weapon_penalty: 0xf70,
                minimum_distance: 0xf74,
                zero: 0xf14,
                title: (4, 0xf48),
            },
        },
    };

    pub fn identify(file: &Path) -> Option<(&str, Self)> {
        let name = file.file_name()?.to_str()?;
        let layout = match name {
            "US_r_Top2Btl.rel" => Self::RETAIL,
            "r_Top2Btl.rel" => Self {
                party_contact_sounds: 0x1d08,
                tint_palette: 0x154c,
                placement: PlacementLayout {
                    party_points: 0x107c,
                    strategies: 0x10d4,
                    party_scalars: [
                        0x1128, 0x10fc, 0x112c, 0x1130, 0x1134, 0x1138, 0x113c, 0x10f4,
                    ],
                    ..Self::RETAIL.placement
                },
                motion: MotionLayout {
                    acceleration: 0x1594,
                    drag_thresholds: [0x163c, 0x1640],
                    stop_epsilon: 0x1648,
                    ..Self::RETAIL.motion
                },
                ui: UiLayout {
                    party_icons: 0x43f4,
                    party_colors: 0x4120,
                    gauge_bonus: 0x4230,
                    combo: 0x42a8,
                    damage: 0x42f0,
                    notices: 0x4314,
                    result_positions: 0x2d8c,
                    result_colors: 0x2da8,
                    texture_bindings: 0xd8,
                    scan: 0x4c0,
                    unison_palettes: 0xe74,
                    strategy: 0x41b4,
                    roster: 0x41e0,
                    leader: 0x4264,
                    marker: 0x29c0,
                    marker_scale: [0x2a20, 0x2a24, 0x2a28, 0x2a2c],
                    marker_follow_speed: 0x4508,
                    ..Self::RETAIL.ui
                },
                messages: MessageLayout {
                    notices: 0x14dc,
                    hud: [0x220, 0x2c34, 0x4534, 0x44b4, 0x44a8],
                    extra_notices: [
                        [0x1678, 0x1680],
                        [0x6550, 0x655c],
                        [0x65c0, 0x65c4],
                        [0x6f20, 0x6f28],
                        [0x147c, 0x15ac],
                    ],
                    result_section: 4,
                    steal: [0x508, 0x520, 0x50c, 0x518, 0x52c, 0x534],
                    steal_prefix: false,
                    result_messages: [
                        0x2f18, 0x2f18, 0x2f30, 0x2f30, 0x2f48, 0x2f60, 0x2f74, 0x2f88, 0x2e04,
                        0x2e1c, 0x2fd0,
                    ],
                    results: [
                        0x2e58, 0x2e64, 0x2e70, 0x2e80, 0x2e8c, 0x2ea4, 0x2eb8, 0x2ecc, 0x2edc,
                        0x2ee4, 0x2eec, 0x2ef4,
                    ],
                },
                unison: UnisonLayout {
                    overlimit_voices: [0x124c, 0x1260],
                    presentation: UnisonPresentationLayout {
                        short_weapon_penalty: 0xf60,
                        minimum_distance: 0xf64,
                        ..Self::RETAIL.unison.presentation
                    },
                    ..Self::RETAIL.unison
                },
                ..Self::RETAIL
            },
            "US_Top2Btl.rel" | "US_m_Top2Btl.rel" | "Top2Btl.rel" | "m_Top2Btl.rel" => Self {
                unison_opener: 0xdd8,
                normal_actions: 0x46f8,
                normal_groups: 11,
                party_settings: 0x4b80,
                party_settings_end: 0x60d0,
                casting_voices: 0x6838,
                casting_voices_end: 0x6860,
                casting_programs: super::casting_programs::Layout {
                    animation: 0x1678,
                    commands: 0x16a8,
                    end: 0x16d0,
                },
                victory_groups: 0x6d08,
                entrance: if matches!(name, "US_Top2Btl.rel" | "US_m_Top2Btl.rel") {
                    EntranceLayout {
                        geometry: 0x538,
                        native_size: 0x2050,
                        rotation: 0x2100,
                        center: 0x210c,
                        origin: [0x1fac, 0x2114],
                        angular: 0x2118,
                        zero: 0x1fb8,
                    }
                } else {
                    EntranceLayout {
                        geometry: 0x538,
                        native_size: 0x2060,
                        rotation: 0x2110,
                        center: 0x211c,
                        origin: [0x1fbc, 0x2124],
                        angular: 0x2128,
                        zero: 0x1fc8,
                    }
                },
                contact_colors: 0x1868,
                contact_effects: 0x1890,
                contact_sounds: 0x189c,
                party_contact_sounds: if name.starts_with("US_") {
                    0x4268
                } else {
                    0x4250
                },
                item_throw_origins: [0x10b4, 0x1138],
                tint_palette: if name.starts_with("US_") {
                    0x3814
                } else {
                    0x380c
                },
                archives: ArchiveLayout {
                    magic: 0x1478,
                    skill: 0x165c,
                    arena: 0x4858,
                    weapon: 0xb60,
                    weapon_slots: 158,
                },
                sources: if name.starts_with("US_") {
                    SourceLayout {
                        enemy: (4, 0x5370),
                        archives: [(4, 0x463c), (4, 0x4688), (4, 0x5cd0), (4, 0x2da0)],
                    }
                } else {
                    SourceLayout {
                        enemy: (4, 0x5380),
                        archives: [(4, 0x464c), (4, 0x4698), (4, 0x5ce0), (4, 0x2db0)],
                    }
                },
                placement: if name.starts_with("US_") {
                    PlacementLayout {
                        party_points: 0x3224,
                        enemy_points: 0x4fe0,
                        strategies: 0x327c,
                        strategy_columns: 12,
                        party_scalars: [
                            0x3320, 0x32dc, 0x3324, 0x3328, 0x332c, 0x3330, 0x3334, 0x32d4,
                        ],
                        enemy_scalars: [
                            0x512c, 0x5130, 0x5134, 0x513c, 0x5140, 0x5144, 0x5148, 0x5138,
                        ],
                    }
                } else {
                    PlacementLayout {
                        party_points: 0x322c,
                        enemy_points: 0x4ff0,
                        strategies: 0x3284,
                        strategy_columns: 12,
                        party_scalars: [
                            0x3328, 0x32e4, 0x332c, 0x3330, 0x3334, 0x3338, 0x333c, 0x32dc,
                        ],
                        enemy_scalars: [
                            0x513c, 0x5140, 0x5144, 0x514c, 0x5150, 0x5154, 0x5158, 0x5148,
                        ],
                    }
                },
                motion: if name.starts_with("US_") {
                    MotionLayout {
                        reactions: 0x6460,
                        projectile_motions: [0x2154, 0x2184],
                        acceleration: 0x385c,
                        action_drag: 0x442c,
                        drag_thresholds: [0x3964, 0x3968],
                        stop_epsilon: 0x3970,
                        projectile_velocity_reset_scale: 0x2c78,
                    }
                } else {
                    MotionLayout {
                        reactions: 0x6460,
                        projectile_motions: [0x2154, 0x2184],
                        acceleration: 0x3854,
                        action_drag: 0x443c,
                        drag_thresholds: [0x395c, 0x3960],
                        stop_epsilon: 0x3968,
                        projectile_velocity_reset_scale: 0x2c88,
                    }
                },
                ui: if name.starts_with("US_") {
                    UiLayout {
                        punctuation: [0x6250, 0x628c],
                        party_icons: 0x8cf0,
                        party_colors: 0x8a20,
                        gauge_bonus: 0x8b30,
                        combo: 0x8ba8,
                        damage: 0x8bf0,
                        notices: 0x8c14,
                        result_positions: 0x6a88,
                        result_colors: 0x6aa4,
                        texture_bindings: 0x868,
                        scan: 0x1f70,
                        unison_palettes: 0x2ea4,
                        strategy: 0x8ab4,
                        roster: 0x8ae0,
                        leader: 0x8b64,
                        marker: 0x65b8,
                        marker_scale: [0x664c, 0x6650, 0x6654, 0x6658],
                        marker_follow_speed: 0x8e40,
                    }
                } else {
                    UiLayout {
                        punctuation: [0x6260, 0x629c],
                        party_icons: 0x8ca4,
                        party_colors: 0x89d0,
                        gauge_bonus: 0x8ae0,
                        combo: 0x8b58,
                        damage: 0x8ba0,
                        notices: 0x8bc4,
                        result_positions: 0x6a90,
                        result_colors: 0x6aac,
                        texture_bindings: 0x868,
                        scan: 0x1f80,
                        unison_palettes: 0x2eb4,
                        strategy: 0x8a64,
                        roster: 0x8a90,
                        leader: 0x8b14,
                        marker: 0x65c0,
                        marker_scale: [0x6654, 0x6658, 0x665c, 0x6660],
                        marker_follow_speed: 0x8e00,
                    }
                },
                messages: if name.starts_with("US_") {
                    MessageLayout {
                        notices: 0x37a4,
                        hud: [0x15c0, 0x692c, 0x8e6c, 0x8c54, 0x8da0],
                        extra_notices: [
                            [0x39a0, 0x39a8],
                            [0xafc8, 0xafd0],
                            [0xb030, 0xb034],
                            [0xb9a0, 0xb9a8],
                            [0x3744, 0x388c],
                        ],
                        result_section: 4,
                        steal: [0x2068, 0x206c, 0x2074, 0x2080, 0x2088, 0x2090],
                        steal_prefix: true,
                        result_messages: [
                            0x7190, 0x717c, 0x71e4, 0x71d0, 0x7268, 0x727c, 0x7328, 0x7448, 0x6b20,
                            0x6b3c, 0x74cc,
                        ],
                        results: [
                            0x6bb4, 0x6bc0, 0x6bcc, 0x6bdc, 0x6be8, 0x6c00, 0x6c14, 0x6c28, 0x6c38,
                            0x6c40, 0x6c50, 0x6c58,
                        ],
                    }
                } else {
                    MessageLayout {
                        notices: 0x379c,
                        hud: [0x15c0, 0x6934, 0x8e2c, 0x8d64, 0x8d58],
                        extra_notices: [
                            [0x3998, 0x39a0],
                            [0xaf90, 0xaf9c],
                            [0xb000, 0xb004],
                            [0xb970, 0xb978],
                            [0x373c, 0x3884],
                        ],
                        result_section: 4,
                        steal: [0x2078, 0x2090, 0x207c, 0x2088, 0x209c, 0x20a4],
                        steal_prefix: false,
                        result_messages: [
                            0x7174, 0x7174, 0x71b4, 0x71b4, 0x7238, 0x7250, 0x72f4, 0x7414, 0x6b28,
                            0x6b40, 0x7494,
                        ],
                        results: [
                            0x6bbc, 0x6bc8, 0x6bd4, 0x6be4, 0x6bf0, 0x6c08, 0x6c1c, 0x6c30, 0x6c40,
                            0x6c48, 0x6c50, 0x6c58,
                        ],
                    }
                },
                unison: if name.starts_with("US_") {
                    UnisonLayout {
                        opener_contact_delays: [0x2ecc, 0x2ed8],
                        placement: 0x2ef8,
                        combined_voices: [0x2f2c, 0x2f40],
                        overlimit_voices: [0x3514, 0x3528],
                        presentation: UnisonPresentationLayout {
                            windup_rate: 0x2f74,
                            windup_color: 0x2e90,
                            combined_color: 0x2f28,
                            hidden_position: [0x2f58, 0x2f5c],
                            camera_eye: [0x2f60, 0x2f64],
                            camera_target_y: 0x2f68,
                            first_x: 0x2f6c,
                            spacing: 0x2f70,
                            short_weapon_penalty: 0x2fcc,
                            minimum_distance: 0x2fd0,
                            zero: 0x2f44,
                            title: (4, 0x2f78),
                        },
                    }
                } else {
                    UnisonLayout {
                        opener_contact_delays: [0x2edc, 0x2ee8],
                        placement: 0x2f08,
                        combined_voices: [0x2f3c, 0x2f50],
                        overlimit_voices: [0x350c, 0x3520],
                        presentation: UnisonPresentationLayout {
                            windup_rate: 0x2f84,
                            windup_color: 0x2ea0,
                            combined_color: 0x2f38,
                            hidden_position: [0x2f68, 0x2f6c],
                            camera_eye: [0x2f70, 0x2f74],
                            camera_target_y: 0x2f78,
                            first_x: 0x2f7c,
                            spacing: 0x2f80,
                            short_weapon_penalty: 0x2fcc,
                            minimum_distance: 0x2fd0,
                            zero: 0x2f54,
                            title: (4, 0x2f88),
                        },
                    }
                },
            },
            "Top2BtlD.rel" => Self {
                unison_opener: 0x41b8,
                normal_actions: 0x30d8,
                normal_groups: 9,
                party_settings: 0x42f0,
                party_settings_end: 0x5840,
                casting_voices: 0x6350,
                casting_voices_end: 0x6378,
                casting_programs: super::casting_programs::Layout {
                    animation: 0x40e8,
                    commands: 0x4118,
                    end: 0x4140,
                },
                victory_groups: 0x6a60,
                entrance: EntranceLayout {
                    geometry: 0x3238,
                    native_size: 0x1cbc,
                    rotation: 0x1ce8,
                    center: 0x1cc4,
                    origin: [0x1ccc, 0x1cd0],
                    angular: 0x1cd4,
                    zero: 0x1cb0,
                },
                contact_colors: 0x908,
                contact_effects: 0x930,
                contact_sounds: 0x93a,
                party_contact_sounds: 0x13fc,
                item_throw_origins: [0x510, 0x580],
                tint_palette: 0x6c0,
                archives: ArchiveLayout {
                    magic: 0x3ee8,
                    skill: 0x40cc,
                    arena: 0x658,
                    weapon: 0x3788,
                    weapon_slots: 157,
                },
                sources: SourceLayout {
                    enemy: (5, 0x228),
                    archives: [(5, 0x3a5b), (5, 0x3a48), (5, 0x7e0), (5, 0x39fc)],
                },
                placement: PlacementLayout {
                    party_points: 0x15c8,
                    enemy_points: 0x90c,
                    strategies: 0x164c,
                    strategy_columns: 10,
                    party_scalars: [
                        0x15fc, 0x1600, 0x1604, 0x1608, 0x15b4, 0x160c, 0x1610, 0x1554,
                    ],
                    enemy_scalars: [0x970, 0x974, 0x978, 0x97c, 0x980, 0x984, 0x988, 0x8f8],
                },
                motion: MotionLayout {
                    reactions: 0x5da0,
                    projectile_motions: [0x134c, 0x137c],
                    acceleration: 0x3ec,
                    action_drag: 0x124c,
                    drag_thresholds: [0x3f0, 0x3f4],
                    stop_epsilon: 0x3f8,
                    projectile_velocity_reset_scale: 0x1978,
                },
                ui: UiLayout {
                    punctuation: [0xaf8, 0xb34],
                    party_icons: 0x3c70,
                    party_colors: 0x39a0,
                    gauge_bonus: 0x3ae8,
                    combo: 0x3b6c,
                    damage: 0x3bc0,
                    notices: 0x3be4,
                    result_positions: 0x289c,
                    result_colors: 0x2918,
                    texture_bindings: 0x1fd0,
                    scan: 0x1d1c,
                    unison_palettes: 0x2248,
                    strategy: 0x3a58,
                    roster: 0x3a84,
                    leader: 0x3b10,
                    marker: 0x2558,
                    marker_scale: [0x2534, 0x2574, 0x2578, 0x257c],
                    marker_follow_speed: 0x3b0c,
                },
                messages: MessageLayout {
                    notices: 0x64c,
                    hud: [0x32d, 0x5874, 0x5fbd, 0x6003, 0x5ffc],
                    extra_notices: [
                        [0x10, 0x15],
                        [0x6958, 0x6961],
                        [0x6970, 0x6974],
                        [0x6c48, 0x6c4e],
                        [0x160, 0x1ab],
                    ],
                    result_section: 5,
                    steal: [0x3751, 0x3764, 0x3754, 0x375d, 0x376d, 0x3775],
                    steal_prefix: false,
                    result_messages: [
                        0x5888, 0x5888, 0x589d, 0x589d, 0x58b2, 0x58c7, 0x58d8, 0x58e9, 0x59a5,
                        0x59bc, 0x58fa,
                    ],
                    results: [
                        0x5918, 0x5922, 0x592d, 0x593a, 0x5944, 0x5959, 0x596b, 0x597d, 0x598b,
                        0x5990, 0x5998, 0x5913,
                    ],
                },
                unison: UnisonLayout {
                    opener_contact_delays: [0x2274, 0x227d],
                    placement: 0x22dc,
                    combined_voices: [0x2318, 0x232c],
                    overlimit_voices: [0x37c, 0x390],
                    presentation: UnisonPresentationLayout {
                        windup_rate: 0x2234,
                        windup_color: 0x2230,
                        combined_color: 0x2314,
                        hidden_position: [0x2288, 0x228c],
                        camera_eye: [0x232c, 0x2330],
                        camera_target_y: 0x2334,
                        first_x: 0x2338,
                        spacing: 0x233c,
                        short_weapon_penalty: 0x230c,
                        minimum_distance: 0x2310,
                        zero: 0x2290,
                        title: (5, 0x42d0),
                    },
                },
            },
            _ => return None,
        };
        Some((name, layout))
    }
}
