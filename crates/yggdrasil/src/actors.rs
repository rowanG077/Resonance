use crate::{
    data::{Assets, Cel, Clip, WalkParts, round},
    drawing::{Drawing, Image, image_rgba},
    scene::{Actor, Grip, Pose},
    timeline::Timeline,
    tree::Tree,
};

// Sprite registration: 128px source cels in a padded 160px articulation layer.
const CEL: f64 = 128.;
const PADDING: f64 = 16.;
const CENTER: f64 = CEL / 2.;
const FOOT: f64 = 116.;
const LAYER: f64 = CEL + 2. * PADDING;
const CROUCH_DROP: f64 = 8.;
const MAX_HAND_MASKS: usize = 512;
const STRIDE: f64 = 48.;
const NOISHE_STRIDE: f64 = 54.;

pub(crate) struct Profile {
    pub cel: usize,
    pub phase: f64,
    pub bob: f64,
    pub crouch: f64,
    pub angle: f64,
}
/// One registration shared by a held object and its foreground hand/cuff mask.
pub(crate) struct HeldCel<'a> {
    pub cel: &'a Cel,
    pub pivot: f64,
    pub angle: f64,
    pub drop: f64,
}
impl HeldCel<'_> {
    pub fn hand(&self, slot: Grip) -> [f64; 2] {
        self.cel.hands[usize::from(slot == Grip::Left)]
    }
}
pub(crate) struct Performers<'a> {
    pub assets: &'a Assets,
    pub timeline: &'a Timeline,
    pub tree: &'a Tree,
    pub progress: f64,
}
impl Performers<'_> {
    fn rig(&self, a: &Actor) -> &WalkParts {
        &self.assets.data.rigs[&a.id]
    }
    fn clip(&self, a: &Actor, name: &str) -> &Clip {
        &self.assets.data.clips[&a.id][name]
    }
    fn uses_profile(&self, a: &Actor) -> bool {
        a.id != "noishe" && (a.walking() || a.side)
    }
    fn profile(&self, a: &Actor) -> Profile {
        let phase = a.travel * std::f64::consts::TAU / STRIDE;
        let crouch = a.crouch;
        Profile {
            cel: a.stance.cel(),
            phase,
            bob: if a.walking() {
                round((phase * 2.).cos() * 0.7)
            } else {
                0.
            },
            crouch,
            angle: (crouch * 22. + a.lean).to_radians(),
        }
    }
    fn rest_frame(&self, a: &Actor) -> usize {
        if a.pose == Pose::Sleep || a.eyes_closed {
            3
        } else if a.pose == Pose::Settle || a.seated && a.seat_at.is_some() && a.rest_age < 0.25 {
            1
        } else {
            2
        }
    }
    pub fn held_cel(&self, a: &Actor) -> HeldCel<'_> {
        if a.resting() || a.clip().is_some() {
            let (name, index) = a.clip().map_or_else(
                || ("rest", self.rest_frame(a)),
                |cel| (cel.name.as_str(), cel.index),
            );
            let clip = self.clip(a, name);
            return HeldCel {
                cel: &clip.frames[index],
                pivot: clip.pivot,
                angle: if a.resting() {
                    a.rest_lean
                } else if name == "gather" {
                    0.
                } else {
                    a.lean
                }
                .to_radians(),
                drop: 0.,
            };
        }
        let p = self.profile(a);
        let clip = self.clip(a, "profile");
        HeldCel {
            cel: &clip.frames[p.cel],
            pivot: clip.pivot,
            angle: if a.walking() { 0. } else { p.angle },
            drop: if a.walking() {
                p.bob
            } else {
                p.crouch * CROUCH_DROP
            },
        }
    }
    fn part(c: &mut Drawing, image: &Image, pivot: [f64; 2], angle: f64, dx: f64, dy: f64) {
        c.save();
        c.translate(PADDING + pivot[0] + dx, PADDING + pivot[1] + dy);
        c.rotate(angle);
        c.image(image, -pivot[0], -pivot[1]);
        c.restore();
    }
    fn profile_bend(
        &self,
        c: &mut Drawing,
        rig: &WalkParts,
        cel: usize,
        hip: f64,
        angle: f64,
        drop: f64,
    ) {
        let (cos, sin) = (angle.cos(), angle.sin());
        for side in [1, 0] {
            let [kx, ky] = rig.pivots[side];
            let [ax, ay] = rig.ankles[side];
            let target = [
                CENTER + (kx - CENTER) * cos - (ky - hip) * sin,
                hip + drop + (kx - CENTER) * sin + (ky - hip) * cos,
            ];
            let rest = (ky - ay).atan2(kx - ax);
            let next = (target[1] - ay).atan2(target[0] - ax);
            Self::part(
                c,
                &self.assets.image(&rig.legs[side]),
                [ax, ay],
                (next - rest).clamp(-0.28, 0.28),
                0.,
                0.,
            );
        }
        Self::part(
            c,
            &self.assets.image(&rig.body[cel]),
            [CENTER, hip],
            angle,
            0.,
            drop,
        );
    }
    fn noishe(&self, c: &mut Drawing, a: &Actor, time: f64) {
        let walk = a.walking();
        let rig = &self.assets.data.noishe;
        let phase = a.travel * std::f64::consts::TAU / NOISHE_STRIDE;
        let amount = a.tail_wag;
        let wag = if walk {
            0.16 * phase.sin()
        } else {
            amount * (-0.65 - 0.24 * (time * 6.).sin())
        };
        let bob = if walk { round((time * 14.).sin()) } else { 0. };
        let p = &rig.tail;
        Self::part(c, &self.assets.image(&p.image), p.pivot, wag, 0., bob);
        for leg in &rig.legs {
            let swing = if walk {
                (phase + leg.phase * std::f64::consts::TAU).sin()
            } else {
                0.
            };
            Self::part(
                c,
                &self.assets.image(&leg.image),
                leg.pivot,
                swing * 0.12,
                0.,
                bob - swing.max(0.),
            );
        }
        c.image(&self.assets.image(&rig.body), PADDING, PADDING + bob);
    }
    fn actor_image(&self, a: &Actor, time: f64) -> Image {
        let mut c = Drawing::new(LAYER as i32, LAYER as i32);
        let id = a.id.as_str();
        let source = self.assets.image(&self.assets.data.cast[id].image);
        let frame = a.frame as f64;
        if a.resting() || a.clip().is_some() {
            let cel = self.held_cel(a);
            Self::part(
                &mut c,
                &self.assets.image(&cel.cel.image),
                [CENTER, cel.pivot],
                cel.angle,
                0.,
                cel.drop,
            );
        } else if a.pose == Pose::ColetteTrip {
            let frame = trip_frame(a.pose_age, &self.assets.data.trip) as f64;
            c.image_rect(
                &source,
                Some([frame * CEL, 0., CEL, CEL]),
                [PADDING, PADDING, CEL, CEL],
                1.,
                crate::drawing::BlendMode::SrcOver,
            );
        } else if self.uses_profile(a) {
            let p = self.profile(a);
            let m = self.rig(a);
            let clip = self.clip(a, "profile");
            let frames = &clip.frames;
            if a.turn_front {
                c.image(&self.assets.image(&frames[0].image), PADDING, PADDING);
            } else if a.walking() {
                let parts = m;
                for side in [1, 0] {
                    let swing = (p.phase + side as f64 * std::f64::consts::PI).sin();
                    Self::part(
                        &mut c,
                        &self.assets.image(&parts.legs[side]),
                        parts.pivots[side],
                        swing * 0.075,
                        swing * 2.5,
                        -swing.max(0.) * 1.5 + p.bob,
                    );
                }
                c.image(
                    &self.assets.image(&parts.body[p.cel]),
                    PADDING,
                    PADDING + p.bob,
                );
            } else if p.crouch > 0. || p.angle != 0. {
                self.profile_bend(
                    &mut c,
                    m,
                    p.cel,
                    clip.pivot,
                    p.angle,
                    p.crouch * CROUCH_DROP,
                );
            } else {
                c.image(&self.assets.image(&frames[p.cel].image), PADDING, PADDING);
            }
        } else if id == "noishe" {
            self.noishe(&mut c, a, time);
        } else if id == "regal" {
            c.image_rect(
                &source,
                Some([frame * CEL, 0., CEL, CEL]),
                [PADDING, PADDING, CEL, CEL],
                1.,
                crate::drawing::BlendMode::SrcOver,
            );
        } else {
            let mut cel = Drawing::new(CEL as i32, CEL as i32);
            cel.image_rect(
                &source,
                Some([frame * CEL, 0., CEL, CEL]),
                [0., 0., CEL, CEL],
                1.,
                crate::drawing::BlendMode::SrcOver,
            );
            Self::part(
                &mut c,
                &cel.snapshot(),
                [CENTER, 89.],
                a.lean.to_radians(),
                0.,
                0.,
            );
        }
        if id != "noishe" {
            c.clean_specks();
        }
        c.snapshot()
    }
    pub fn actor(&self, c: &mut Drawing, a: &Actor, time: f64) {
        let id = a.id.as_str();
        let size = self.assets.data.cast[id].size;
        let image = self.actor_image(a, time);
        for yy in -2..=2 {
            let r = round(size * 0.17 * (1. - (yy * yy) as f64 / 9.).sqrt());
            c.fill(
                [round(a.x) - r, round(a.y) + yy as f64, 2. * r, 1.],
                "#23392b",
                0.16,
                crate::drawing::BlendMode::SrcOver,
            );
        }
        c.save();
        c.translate(round(a.x), round(a.y));
        c.scale(
            if id == "noishe" || a.pose == Pose::ColetteTrip {
                -a.face()
            } else {
                a.face()
            },
            1.,
        );
        if a.pose == Pose::Sleep {
            let phase = self.assets.data.cast[id].breathing_phase;
            c.translate(0., round((time * 1.25 + phase).sin() * 0.6));
        }
        c.image_scaled(
            &image,
            round(-(CENTER + PADDING) * size / CEL),
            round(-(FOOT + PADDING) * size / CEL),
            round(LAYER * size / CEL),
            round(LAYER * size / CEL),
        );
        c.restore();
    }
    pub fn hand(&self, a: &Actor, anchor: Option<[f64; 2]>, slot: Grip) -> [f64; 2] {
        if slot == Grip::Both && anchor.is_none() {
            let r = self.hand(a, None, Grip::Right);
            let l = self.hand(a, None, Grip::Left);
            return [(r[0] + l[0]) / 2., (r[1] + l[1]) / 2.];
        }
        let s = self.assets.data.cast[&a.id].size / CEL;
        if a.resting() || a.clip().is_some() || self.uses_profile(a) {
            let cel = self.held_cel(a);
            let [x, y] = anchor.unwrap_or_else(|| cel.hand(slot));
            let (dx, dy) = (x - CENTER, y - cel.pivot);
            let heading = if a.resting() || a.clip().is_some_and(|c| c.name == "gather") {
                a.face()
            } else {
                a.heading.unwrap_or(a.face())
            };
            let turn = 1. - heading.abs().min(1.);
            return [
                a.x + heading * (dx * cel.angle.cos() - dy * cel.angle.sin()) * s,
                a.y + (cel.pivot + dx * cel.angle.sin() + dy * cel.angle.cos() - FOOT + cel.drop)
                    * s
                    + turn * if a.id == "colette" { 12. } else { 6. },
            ];
        }
        if a.id == "regal" {
            return [a.x + a.face() * 4. * s, a.y - 33. * s];
        }
        let lean = a.lean.to_radians();
        [
            a.x + a.face() * (21. * lean.cos() + lean.sin()) * s,
            a.y + (-27. + 21. * lean.sin() - lean.cos()) * s,
        ]
    }
    pub fn mask(&self, image: &Image, points: &[[f64; 2]]) -> Image {
        let key = (
            image.0.id,
            points.iter().map(|p| p.map(f64::to_bits)).collect(),
        );
        let mut masks = self.assets.masks.lock().unwrap();
        if let Some(mask) = masks.get(&key) {
            return mask.clone();
        }
        let w = image.width();
        let h = image.height();
        let mut pixels = image.source_pixels().into_owned();
        for y in 0..h {
            for x in 0..w {
                if !inside(x as f64 + 0.5, y as f64 + 0.5, points) {
                    let i = ((y * w + x) * 4) as usize;
                    pixels[i..i + 4].fill(0);
                }
            }
        }
        let mask = image_rgba(w, h, &pixels);
        // Bound retained hand masks across changes of pose and grip.
        if masks.len() >= MAX_HAND_MASKS {
            masks.clear();
        }
        masks.insert(key, mask.clone());
        mask
    }
    pub fn overlay(
        &self,
        c: &mut Drawing,
        a: &Actor,
        image: &Image,
        pivot_y: f64,
        angle: f64,
        drop: f64,
    ) {
        let mut local = Drawing::new(LAYER as i32, LAYER as i32);
        Self::part(&mut local, image, [CENTER, pivot_y], angle, 0., drop);
        let s = self.assets.data.cast[&a.id].size / CEL;
        c.save();
        c.translate(round(a.x), round(a.y));
        c.scale(a.face(), 1.);
        c.image_scaled(
            &local.snapshot(),
            round(-(CENTER + PADDING) * s),
            round(-(FOOT + PADDING) * s),
            round(LAYER * s),
            round(LAYER * s),
        );
        c.restore();
    }
}
fn trip_frame(age: f64, sequence: &[crate::data::Exposure]) -> usize {
    let mut remaining = (crate::exposure(age) * crate::FPS as f64).round() as usize;
    for &crate::data::Exposure { cel, frames: hold } in sequence {
        if remaining < hold {
            return cel;
        }
        remaining -= hold;
    }
    sequence.last().unwrap().cel
}
fn inside(x: f64, y: f64, points: &[[f64; 2]]) -> bool {
    let mut hit = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[j];
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            hit = !hit;
        }
        j = i;
    }
    hit
}
