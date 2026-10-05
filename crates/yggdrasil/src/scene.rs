//! Authored keyframes. Continuous quantities interpolate; drawings and actions
//! change on their keyframe. Missing optional attachments have no effect.
use crate::data::{clamp, smooth};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Pose {
    #[default]
    Idle,
    Walk,
    ColetteTrip,
    Settle,
    Sit,
    Sleep,
}
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Stance {
    #[default]
    Relaxed,
    Holding,
    Pouring,
}
impl Stance {
    pub fn cel(self) -> usize {
        match self {
            Self::Relaxed => 1,
            Self::Holding => 2,
            Self::Pouring => 3,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClipFrame {
    pub name: String,
    pub index: usize,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Actor {
    pub id: String,
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub pose: Pose,
    pub face: f64,
    pub side: bool,
    pub held: bool,
    pub stance: Stance,
    pub clip: Option<ClipFrame>,
    pub frame: usize,
    pub crouch: f64,
    pub lean: f64,
    pub heading: Option<f64>,
    pub rest_lean: f64,
    pub tail_wag: f64,
    pub eyes_closed: bool,
    pub seated: bool,
    pub seat_at: Option<f64>,
    #[serde(skip)]
    pub travel: f64,
    #[serde(skip)]
    pub pose_since: f64,
    #[serde(skip)]
    pub pose_age: f64,
    #[serde(skip)]
    pub rest_age: f64,
    #[serde(skip)]
    pub moving: bool,
    #[serde(skip)]
    pub turn_front: bool,
}
impl Actor {
    pub fn face(&self) -> f64 {
        if self.face == 0. { 1. } else { self.face }
    }
    pub fn walking(&self) -> bool {
        self.pose == Pose::Walk || self.moving
    }
    pub fn resting(&self) -> bool {
        (self.seated || matches!(self.pose, Pose::Sit | Pose::Sleep | Pose::Settle)) && !self.moving
    }
    pub fn clip(&self) -> Option<&ClipFrame> {
        self.clip.as_ref().filter(|_| !self.walking())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PropKind {
    #[default]
    Can,
    Support,
    Tie,
    Flower,
    Book,
    Pencil,
    Specimen,
    Bread,
    PaperCharm,
    CharmCord,
    ManaSpark,
    Log,
    HearthStone,
    Campfire,
    Blanket,
    Basket,
    Bowl,
}
impl PropKind {
    pub fn asset(self) -> &'static str {
        match self {
            Self::Can => "can",
            Self::Support => "support",
            Self::Tie => "tie",
            Self::Flower => "flower",
            Self::Book => "book",
            Self::Pencil => "pencil",
            Self::Specimen => "specimen",
            Self::Bread => "bread",
            Self::PaperCharm => "paper-charm",
            Self::CharmCord => "charm-cord",
            Self::ManaSpark => "mana-spark",
            Self::Log => "log",
            Self::HearthStone => "hearth-stones",
            Self::Campfire => "campfire",
            Self::Blanket => "blanket",
            Self::Basket => "basket",
            Self::Bowl => "bowl",
        }
    }
}

fn unit_scale() -> f64 {
    1.
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Layer {
    Back,
    Front,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Grip {
    Right,
    Left,
    Both,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Placement {
    #[default]
    Held,
    Drop,
    Ground,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WaterFlow {
    pub start: f64,
    pub end: f64,
    pub target: [f64; 2],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Prop {
    pub id: String,
    pub t: f64,
    #[serde(rename = "type")]
    pub kind: PropKind,
    pub attach: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    #[serde(default)]
    pub angle: f64,
    pub face: Option<f64>,
    #[serde(default = "unit_scale")]
    pub scale: f64,
    pub layer: Option<Layer>,
    pub grip: Option<Grip>,
    pub grip_offset: Option<[f64; 2]>,
    pub anchor: Option<[f64; 2]>,
    #[serde(default)]
    pub pin_drop: f64,
    #[serde(default)]
    pub open: f64,
    #[serde(default)]
    pub heat: f64,
    #[serde(default)]
    pub wrapped: f64,
    #[serde(default)]
    pub strength: f64,
    #[serde(default)]
    pub charcoal: f64,
    #[serde(default)]
    pub bread_half: usize,
    #[serde(default)]
    pub cel: usize,
    pub finger_width: Option<f64>,
    pub finger_height: Option<f64>,
    pub visible: bool,
    #[serde(default)]
    pub grounded: bool,
    #[serde(default)]
    pub support_wrap: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub bitten: bool,
    #[serde(default)]
    pub emissive: bool,
    #[serde(default)]
    pub empty: bool,
    pub flow: Option<WaterFlow>,
    pub rest_anchor: Option<RestAnchor>,
    pub tree_attach: Option<TreeAnchor>,
    #[serde(default)]
    pub placement: Placement,
    #[serde(skip)]
    pub actor: Option<Actor>,
    #[serde(skip)]
    pub event_id: String,
    #[serde(skip)]
    pub wrap_centre: f64,
    #[serde(skip)]
    pub wrap_radius: f64,
    #[serde(skip)]
    pub tree_radius: f64,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestAnchor {
    pub actor: String,
    pub time: f64,
    pub drop_at: f64,
    pub rest_at: f64,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TreeAnchor {
    pub branch: String,
    pub distance: f64,
    pub offset: [f64; 2],
}
impl Prop {
    pub fn face(&self) -> f64 {
        self.face.filter(|v| *v != 0.).unwrap_or(1.)
    }
}

pub(crate) trait Keyframe: Clone {
    fn time(&self) -> f64;
    fn interpolate(&self, next: &Self, time: f64, amount: f64) -> Self;
}
pub(crate) fn sample<T: Keyframe>(keys: &[T], time: f64) -> T {
    let i = keys.partition_point(|k| k.time() <= time).saturating_sub(1);
    let a = &keys[i];
    let b = &keys[(i + 1).min(keys.len() - 1)];
    let duration = b.time() - a.time();
    let amount = if duration > 0. {
        clamp((time - a.time()) / duration)
    } else {
        0.
    };
    a.interpolate(b, time, amount)
}
fn blend(a: &mut f64, b: f64, u: f64) {
    *a += (b - *a) * u;
}
impl Keyframe for Actor {
    fn time(&self) -> f64 {
        self.t
    }
    fn interpolate(&self, b: &Self, time: f64, u: f64) -> Self {
        let mut out = self.clone();
        out.x += (b.x - self.x) * u;
        out.y += (b.y - self.y) * u;
        blend(&mut out.crouch, b.crouch, smooth(u));
        blend(&mut out.lean, b.lean, smooth(u));
        blend(&mut out.rest_lean, b.rest_lean, smooth(u));
        blend(&mut out.tail_wag, b.tail_wag, smooth(u));
        let distance = (b.x - self.x).hypot(b.y - self.y);
        out.travel += distance * u;
        const WALK_SPEED_THRESHOLD: f64 = 8.; // scene pixels / second
        out.moving = b.t > self.t && distance / (b.t - self.t) > WALK_SPEED_THRESHOLD;
        // Facing is authored independently of the path: a short backstep must
        // not reverse the actor. Finish travelling before starting a turn.
        // The trip is drawn facing its travel direction; do not turn into it.
        if !out.moving
            && (b.pose != Pose::ColetteTrip || self.pose == b.pose)
            && let (Some(a), Some(b)) = (&mut out.heading, b.heading)
        {
            blend(a, b, smooth(u));
        }
        out.pose_age = (time - self.pose_since).max(0.);
        out.rest_age = self.seat_at.map_or(0., |t| (time - t).max(0.));
        if let Some(heading) = out.heading {
            out.face = if heading < 0. { -1. } else { 1. };
            out.turn_front = !out.moving && heading.abs() < 0.48;
        }
        out
    }
}

impl Keyframe for Prop {
    fn time(&self) -> f64 {
        self.t
    }
    fn interpolate(&self, b: &Self, _: f64, u: f64) -> Self {
        let mut out = self.clone();
        if self.attach == b.attach {
            blend(&mut out.x, b.x, u);
            blend(&mut out.y, b.y, u);
        }
        blend(&mut out.angle, b.angle, smooth(u));
        if let (Some(a), Some(b)) = (&mut out.z, b.z) {
            blend(a, b, u);
        }
        for (a, b) in [
            (&mut out.scale, b.scale),
            (&mut out.pin_drop, b.pin_drop),
            (&mut out.open, b.open),
            (&mut out.heat, b.heat),
            (&mut out.wrapped, b.wrapped),
            (&mut out.strength, b.strength),
            (&mut out.charcoal, b.charcoal),
        ] {
            blend(a, b, u);
        }
        out
    }
}

#[cfg(test)]
#[test]
fn short_steps_preserve_facing_and_stationary_turns_still_work() {
    let mut keys = [
        Actor {
            x: 20.,
            face: 1.,
            ..Default::default()
        },
        Actor {
            t: 0.5,
            x: 12.,
            y: 8.,
            face: 1.,
            ..Default::default()
        },
        Actor {
            t: 1.,
            x: 12.,
            y: 8.,
            face: -1.,
            heading: Some(-1.),
            ..Default::default()
        },
    ];
    // Both discrete facing and a smooth heading must survive a backstep.
    for heading in [None, Some(1.)] {
        keys[0].heading = heading;
        keys[1].heading = heading;
        for tick in (0..=crate::FPS / 2).rev() {
            let actor = sample(&keys, tick as f64 / crate::FPS as f64);
            assert_eq!(actor.face(), 1.);
            assert!(!actor.turn_front);
        }
    }
    assert!(sample(&keys, 0.75).turn_front);
    assert_eq!(sample(&keys, 1.).face(), -1.);
    // A destination's turn must not begin during the approach.
    keys[1].heading = Some(-1.);
    assert_eq!(sample(&keys, 0.4).face(), 1.);
    assert!(!sample(&keys, 0.4).turn_front);
}
