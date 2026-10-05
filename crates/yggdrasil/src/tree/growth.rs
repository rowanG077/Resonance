//! Build one persistent tree from editable curves and painted crown patterns.
use super::{Branch, Leaf, LeafKind, Point};
use crate::data::{Assets, ImageRef, LeafArt, TreeArt, clamp, round};
use serde::Deserialize;

const MIN_CURVE_SAMPLES: usize = 8;
const CHILD_GROWTH_DELAY: f64 = 0.008;
const MIN_BRANCH_GROWTH: f64 = 0.04;
const MIN_LEAF_OPENING: f64 = 0.018;
const ATTACHMENT_SEARCH_STEP: f64 = 3.;
const SHOOT_NODE_SPACING: f64 = 4.5;
const BLADE_GRID_SPACING: f64 = 3.65;
const BLADE_POSITION_JITTER: f64 = 1.7;
const TWIG_BIRTH_DELAY: f64 = 0.01;
const TWIG_BIRTH_STAGGER: f64 = 0.003;
const TWIG_GROWTH_PER_PIXEL: f64 = 0.001;
const SHOOT_BIRTH_DELAY: f64 = 0.001;
const SHOOT_OPENING_DISTANCE: f64 = 7.5;
pub(super) const RIB_GROWTH_DURATION: f64 = 0.52;
const BLADE_BIRTH_DELAY: f64 = 0.005;
const BLADE_BIRTH_STAGGER: f64 = 0.025;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GrowthTiming {
    twig_duration: f64,
    twig_finish_by: f64,
    crown_delay: f64,
    crown_delay_variation: f64,
    crown_opening: f64,
    full_crown_base: f64,
    full_crown_tip_weight: f64,
    shoot_opening: [f64; 2],
    blade_opening: f64,
    blade_opening_variation: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BranchCurve {
    id: String,
    parent: Option<String>,
    #[serde(default)]
    attach: f64,
    #[serde(default)]
    at_tip: bool,
    birth: f64,
    end: f64,
    width: f64,
    points: Vec<[f64; 2]>,
    #[serde(default)]
    initial: f64,
    #[serde(default)]
    kind: BranchKind,
}
#[derive(Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum BranchKind {
    #[default]
    Bough,
    Root,
    Ridge,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CrownTier {
    y: f64,
    x: Vec<f64>,
}

pub(super) struct Canopy {
    pub pivot: [f64; 2],
    pub ribs: Vec<Rib>,
    pub blades: Vec<Blade>,
}
pub(super) struct Rib {
    pub a: [f64; 2],
    pub b: [f64; 2],
}
pub(super) struct Blade {
    pub x: f64,
    pub y: f64,
    pub stem: [f64; 2],
    pub delay: f64,
    pub opening: f64,
    pub asset: LeafArt,
}

/// Catmull–Rom curves interpolate the authored points. Arc lengths keep bark,
/// leaves and child branches attached to fixed positions along the wood.
fn sample_curve(points: &[[f64; 2]]) -> Vec<Point> {
    let mut path: Vec<Point> = Vec::new();
    let mut distance = 0.;
    let mut push = |[x, y]: [f64; 2]| {
        if let Some(last) = path.last() {
            distance += (x - last.x).hypot(y - last.y);
        }
        path.push(Point { x, y, s: distance });
    };
    for i in 0..points.len() - 1 {
        let [a, b, c, d] = [
            points[i.saturating_sub(1)],
            points[i],
            points[i + 1],
            points[(i + 2).min(points.len() - 1)],
        ];
        let steps = ((c[0] - b[0]).hypot(c[1] - b[1]).ceil() as usize).max(MIN_CURVE_SAMPLES);
        for step in 0..steps {
            let t = step as f64 / steps as f64;
            push([0, 1].map(|axis| {
                0.5 * (2. * b[axis]
                    + (-a[axis] + c[axis]) * t
                    + (2. * a[axis] - 5. * b[axis] + 4. * c[axis] - d[axis]) * t * t
                    + (-a[axis] + 3. * b[axis] - 3. * c[axis] + d[axis]) * t * t * t)
            }));
        }
    }
    push(*points.last().expect("branch control points"));
    path
}
impl BranchCurve {
    fn build(&self, origin: [f64; 2], branches: &[Branch], twig: bool) -> Branch {
        let parent = self.parent.as_ref().map(|id| {
            branches
                .iter()
                .find(|b| &b.id == id)
                .expect("parent precedes child")
        });
        let attach = if self.at_tip {
            parent
                .expect("tip attachment requires parent")
                .mature_length
        } else {
            self.attach
        };
        let anchor = parent.map_or(origin, |b| b.point(attach));
        let birth = parent.map_or(self.birth, |b| {
            self.birth.max(b.arrival(attach) + CHILD_GROWTH_DELAY)
        });
        let points: Vec<_> = self
            .points
            .iter()
            .map(|p| [anchor[0] + p[0], anchor[1] + p[1]])
            .collect();
        let path = sample_curve(&points);
        Branch {
            id: self.id.clone(),
            parent: self.parent.clone(),
            attach,
            birth,
            end: self.end.max(birth + MIN_BRANCH_GROWTH),
            initial: self.initial,
            width: self.width,
            mature_length: path.last().unwrap().s,
            path,
            root: self.kind == BranchKind::Root,
            twig,
        }
    }
}
fn leaf(branch: &Branch, attach: f64, asset: usize, birth: f64, end: f64, kind: LeafKind) -> Leaf {
    let birth = birth.max(branch.arrival(attach));
    Leaf {
        parent: branch.id.clone(),
        attach,
        asset,
        birth,
        end: end.max(birth + MIN_LEAF_OPENING),
        front: kind == LeafKind::Canopy,
        kind,
        direction: [0.; 2],
        span: 0.,
    }
}

pub(crate) fn skeleton(art: &TreeArt) -> (Vec<Branch>, Vec<Leaf>) {
    let timing = &art.timing;
    let mut branches: Vec<Branch> = Vec::new();
    let mut depths = Vec::new();
    let mut primary = Vec::new();
    for curve in &art.branches {
        let depth = curve.parent.as_ref().map_or(0, |id| {
            depths[branches
                .iter()
                .position(|b| &b.id == id)
                .expect("branch parent")]
                + 1
        });
        if curve.kind == BranchKind::Bough {
            primary.push(branches.len());
        }
        depths.push(depth);
        branches.push(curve.build(art.origin, &branches, false));
    }
    let mut foliage = Vec::new();
    let mut shoot_branches = primary.clone();
    let mut patch = 0;
    for (tier_index, tier) in art.crown.iter().enumerate() {
        for (column, x) in tier.x.iter().enumerate() {
            // Fixed offsets soften the tiers without movement or random popping.
            let cx = x + (patch as f64 * 2.4).sin() * 6. + (tier.y * 0.037).sin() * 3.;
            let cy = tier.y
                + (patch as f64 * 1.8).sin() * 7.
                + (cx * 0.051).sin() * 3.
                + if cx < 175. {
                    5.
                } else if cx > 350. {
                    -4.
                } else {
                    0.
                };
            let target = [cx, cy + 15.];
            // Connect every crown patch to the nearest supporting bough.
            // Penalize the trunk and branch bases to favor smaller branches.
            let mut nearest = (usize::MAX, 0., f64::INFINITY);
            for &index in &primary {
                let b = &branches[index];
                let mut s = 6.;
                while s <= b.mature_length {
                    let p = b.point(s);
                    let cost = (target[0] - p[0]).hypot(target[1] - p[1])
                        + if b.parent.is_none() { 14. } else { 0. }
                        + if s < 12. { 8. } else { 0. };
                    if cost < nearest.2 {
                        nearest = (index, s, cost);
                    }
                    s += ATTACHMENT_SEARCH_STEP;
                }
            }
            let (parent, attach, _) = nearest;
            let b = &branches[parent];
            let anchor = b.point(attach);
            let (dx, dy) = (target[0] - anchor[0], target[1] - anchor[1]);
            let birth =
                b.arrival(attach) + TWIG_BIRTH_DELAY + (patch % 5) as f64 * TWIG_BIRTH_STAGGER;
            let curve = BranchCurve {
                id: format!("crown-twig-{patch:02}"),
                parent: Some(b.id.clone()),
                attach,
                at_tip: false,
                birth,
                end: (birth + timing.twig_duration + dx.hypot(dy) * TWIG_GROWTH_PER_PIXEL)
                    .min(timing.twig_finish_by),
                width: 2.1 + (patch % 3) as f64 * 0.35,
                points: vec![
                    [0., 0.],
                    [dx * 0.35, dy * 0.23 - 3.],
                    [dx * 0.76, dy * 0.7 - 2.],
                    [dx, dy],
                ],
                initial: 0.,
                kind: BranchKind::Bough,
            };
            let twig = curve.build(art.origin, &branches, true);
            let mut asset = (patch * 5 + tier_index) % art.canopies.len();
            if tier.y >= 171. {
                asset = if column == 0 && cx < 150. {
                    2
                } else if column == tier.x.len() - 1 && cx > 380. {
                    3
                } else {
                    [2, 7, 4, 3, 6][patch % 5]
                };
            }
            let ready = twig.arrival(twig.mature_length);
            let delay = timing.crown_delay
                + timing.crown_delay_variation * (0.5 + 0.5 * (patch as f64 * 2.17).sin());
            let bloom = ready + (1. - ready) * delay;
            let full = (bloom + timing.crown_opening)
                .max(timing.full_crown_base + ready * timing.full_crown_tip_weight)
                .min(1.);
            foliage.push(leaf(
                &twig,
                twig.mature_length,
                asset,
                bloom,
                full,
                LeafKind::Canopy,
            ));
            shoot_branches.push(branches.len());
            depths.push(depths[parent] + 1);
            branches.push(twig);
            patch += 1;
        }
    }
    // Juvenile leaves follow each extending tip, rooted at fixed material nodes.
    for index in shoot_branches {
        let b = &branches[index];
        let mut nodes = Vec::new();
        let mut s = if b.parent.is_none() { 20. } else { 2. };
        while s < b.mature_length - 1. {
            nodes.push(s);
            s += SHOOT_NODE_SPACING;
        }
        nodes.push(b.mature_length - 0.25);
        for (k, s) in nodes.into_iter().enumerate() {
            let start = b.arrival(s) + SHOOT_BIRTH_DELAY;
            let interval = (b.end - b.birth) / (b.mature_length - b.initial);
            let duration = (interval * SHOOT_OPENING_DISTANCE)
                .clamp(timing.shoot_opening[0], timing.shoot_opening[1]);
            let a = b.point((s - 1.).max(0.));
            let z = b.point((s + 1.).min(b.mature_length));
            let distance = (z[0] - a[0]).hypot(z[1] - a[1]);
            let distance = if distance == 0. { 1. } else { distance };
            let (tx, ty) = ((z[0] - a[0]) / distance, (z[1] - a[1]) / distance);
            let side = if k % 2 == 0 { -1. } else { 1. };
            let (dx, dy) = (
                tx * 0.45 - ty * side * 0.9,
                ty * 0.45 + tx * side * 0.9 - 0.28,
            );
            let norm = dx.hypot(dy);
            let mut node = leaf(
                b,
                s,
                k % art.shoots.len(),
                start,
                (start + duration).min(1.),
                LeafKind::Shoot,
            );
            node.direction = [dx / norm, dy / norm];
            node.span = (7 + (k * 7 + depths[index] * 3) % 5) as f64;
            foliage.push(node);
        }
    }
    (branches, foliage)
}

// A stable spatial variation: only the painted cell and canopy number matter.
fn variation(x: f64, y: f64, seed: usize) -> f64 {
    let n = (x * 127.1 + y * 311.7 + seed as f64 * 74.7).sin() * 43758.5453;
    n - n.floor()
}

pub(super) fn canopies(assets: &Assets) -> Vec<Canopy> {
    let art = &assets.tree;
    let timing = &art.timing;
    let shoots: Vec<_> = art.shoots.iter().map(|a| assets.image(&a.image)).collect();
    art.canopies
        .iter()
        .enumerate()
        .map(|(seed, design)| {
            let image = assets.image(&design.image);
            let pixels = image.source_pixels();
            let (w, h) = (image.width() as f64, image.height() as f64);
            let [px, py] = design.pivot;
            let ribs: Vec<_> = [
                [w * 0.5, 3.],
                [w * 0.23, h * 0.27],
                [w * 0.8, h * 0.26],
                [3., h * 0.67],
                [w - 3., h * 0.69],
            ]
            .into_iter()
            .map(|b| Rib { a: design.pivot, b })
            .collect();
            let mut blades = Vec::new();
            let mut gy = 4.;
            while gy < h - 2. {
                let mut gx = 2.;
                while gx < w - 2. {
                    let x = gx + (variation(gx, gy, seed) - 0.5) * BLADE_POSITION_JITTER;
                    let y = gy + (variation(gy, gx, seed + 5) - 0.5) * BLADE_POSITION_JITTER;
                    let i = (round(y) as usize * image.width() as usize + round(x) as usize) * 4;
                    if pixels[i + 3] != 0 {
                        let shape =
                            (variation(gx, gy, seed + 11) * shoots.len() as f64).floor() as usize;
                        let source = &shoots[shape];
                        let pivot = art.shoots[shape].pivot;
                        let bx = x - source.width() as f64 * 0.5 + pivot[0];
                        let by = y - source.height() as f64 * 0.5 + pivot[1];
                        let mut closest = ([0.; 2], 0., f64::INFINITY);
                        for rib in &ribs {
                            let (dx, dy) = (rib.b[0] - px, rib.b[1] - py);
                            let t = clamp(((bx - px) * dx + (by - py) * dy) / (dx * dx + dy * dy));
                            let at = [px + dx * t, py + dy * t];
                            let distance = (bx - at[0]).hypot(by - at[1]);
                            if distance < closest.2 {
                                closest = (at, t, distance);
                            }
                        }
                        let image = ImageRef(format!(
                            "{}/{}/{:02x}{:02x}{:02x}",
                            art.palette,
                            shape + 1,
                            pixels[i],
                            pixels[i + 1],
                            pixels[i + 2]
                        ));
                        blades.push(Blade {
                            x: bx,
                            y: by,
                            stem: closest.0,
                            delay: closest.1 * RIB_GROWTH_DURATION
                                + BLADE_BIRTH_DELAY
                                + variation(gx, gy, seed + 29) * BLADE_BIRTH_STAGGER,
                            opening: timing.blade_opening
                                + variation(gx, gy, seed + 31) * timing.blade_opening_variation,
                            asset: LeafArt { image, pivot },
                        });
                    }
                    gx += BLADE_GRID_SPACING;
                }
                gy += BLADE_GRID_SPACING;
            }
            blades.sort_by(|a, b| a.y.total_cmp(&b.y));
            Canopy {
                pivot: design.pivot,
                ribs,
                blades,
            }
        })
        .collect()
}

#[cfg(test)]
#[test]
fn branches_and_leaves_remain_attached() {
    let assets = Assets::load().unwrap();
    let (branches, foliage) = skeleton(&assets.tree);
    for branch in &branches {
        assert!(branch.path.windows(2).all(|p| p[1].s > p[0].s));
        if let Some(parent) = &branch.parent {
            let parent = branches.iter().find(|b| &b.id == parent).unwrap();
            let anchor = parent.point(branch.attach);
            let start = branch.point(0.);
            assert!((anchor[0] - start[0]).hypot(anchor[1] - start[1]) < 1e-9);
            assert!(branch.birth >= parent.arrival(branch.attach));
        }
    }
    for leaf in foliage {
        let parent = branches.iter().find(|b| b.id == leaf.parent).unwrap();
        assert!(leaf.attach <= parent.mature_length);
        assert!(leaf.birth >= parent.arrival(leaf.attach));
        assert!(leaf.end > leaf.birth);
    }
}
