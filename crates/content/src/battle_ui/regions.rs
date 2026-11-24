//! Source rectangles sampled by battle HUD roles, in [x, y, width, height] order.

type Rect = [u16; 4];

pub struct HudRegions {
    pub targets: [Rect; 5],
    pub stun: Rect,
    pub notice_frame: Rect,
    pub notice_fill: Rect,
    pub notice_icons: [Rect; 4],
    pub notice_faces: [Rect; 9],
    pub stat_icons: [Rect; 10],
    pub scan_icons: [Rect; 10],
    pub unison_frame: Rect,
    pub escape: [Rect; 3],
}

impl HudRegions {
    pub fn new(notice_icon_origins: [[u16; 2]; 4]) -> Self {
        let mut targets = [[48, 256, 144, 64]; 5];
        targets[0] = [0, 256, 48, 64];
        Self {
            targets,
            stun: [256, 25, 48, 30],
            notice_frame: [384, 376, 128, 80],
            notice_fill: [408, 456, 104, 56],
            notice_icons: notice_icon_origins.map(|[u, v]| [u, v, 24, 24]),
            // Notices and Unison use slightly different cuts of the last three faces.
            notice_faces: std::array::from_fn(|i| {
                [64 + i as u16 * 32, 104, if i >= 6 { 40 } else { 32 }, 32]
            }),
            stat_icons: [
                [288, 136, 24, 24],
                [352, 56, 24, 24],
                [304, 56, 24, 24],
                [328, 80, 24, 24],
                [256, 56, 24, 24],
                [280, 56, 24, 24],
                // Accuracy and the blinking curse overlay share this role.
                [0, 72, 280, 32],
                [32, 72, 24, 24],
                [352, 80, 24, 24],
                [328, 56, 24, 24],
            ],
            scan_icons: std::array::from_fn(|i| {
                if i < 9 {
                    [i as u16 * 24, 136, 24, 24]
                } else {
                    [214, 136, 48, 24]
                }
            }),
            unison_frame: [0, 368, 352, 144],
            escape: [[304, 208, 144, 80], [448, 208, 16, 32], [312, 208, 128, 80]],
        }
    }
}
