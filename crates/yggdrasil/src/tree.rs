use crate::drawing::Image;
use crate::{
    data::{Assets, LeafArt, clamp, round, smooth},
    drawing::{Drawing, image_rgba},
};
pub(crate) mod growth;

const W: usize = crate::ENVIRONMENT_WIDTH as usize;
const H: usize = crate::ENVIRONMENT_HEIGHT as usize;
pub(crate) const GROWTH_STEPS: usize = 600;
const GROUND_Y: usize = 253;

// Girth develops after the growing tip reaches each point on a branch.
const THICKENING_SPAN: f64 = 0.46;
const GIRTH_POWER: f64 = 1.48;
const TAPER_POWER: f64 = 0.83;
const TIP_FRACTION: f64 = 0.08;
const MIN_BRANCH_RADIUS: f64 = 0.75;
const BARK: [[u8; 3]; 8] = [
    [60, 48, 61],
    [89, 64, 74],
    [120, 80, 82],
    [151, 100, 82],
    [178, 129, 88],
    [204, 158, 105],
    [229, 191, 129],
    [245, 215, 156],
];

pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
    pub s: f64,
}
pub(crate) struct Branch {
    pub id: String,
    pub parent: Option<String>,
    pub attach: f64,
    pub birth: f64,
    pub end: f64,
    pub initial: f64,
    pub width: f64,
    pub path: Vec<Point>,
    pub mature_length: f64,
    pub twig: bool,
    pub root: bool,
}

pub(crate) struct Leaf {
    pub parent: String,
    pub attach: f64,
    pub asset: usize,
    pub birth: f64,
    pub end: f64,
    pub front: bool,
    pub kind: LeafKind,
    pub direction: [f64; 2],
    pub span: f64,
}
#[derive(PartialEq, Eq)]
pub(crate) enum LeafKind {
    Shoot,
    Canopy,
}

impl Branch {
    pub fn length(&self, p: f64) -> f64 {
        self.initial
            + (self.mature_length - self.initial)
                * clamp((p - self.birth) / (self.end - self.birth))
    }
    pub fn arrival(&self, s: f64) -> f64 {
        if s <= self.initial {
            -0.13
        } else {
            self.birth
                + (self.end - self.birth) * (s - self.initial) / (self.mature_length - self.initial)
        }
    }
    pub fn radius(&self, s: f64, p: f64) -> f64 {
        let arrival = self.arrival(s);
        if p < arrival {
            return 0.;
        }
        let age = clamp((p - arrival) / THICKENING_SPAN);
        let girth = p.powf(GIRTH_POWER);
        let taper = TIP_FRACTION
            + (1. - TIP_FRACTION) * (1. - clamp(s / self.mature_length)).powf(TAPER_POWER);
        MIN_BRANCH_RADIUS
            + ((self.width * taper).max(MIN_BRANCH_RADIUS) - MIN_BRANCH_RADIUS) * age * girth
    }
    pub fn point(&self, s: f64) -> [f64; 2] {
        let distance = s.clamp(0., self.mature_length);
        let i = self
            .path
            .partition_point(|p| p.s <= distance)
            .saturating_sub(1)
            .min(self.path.len() - 2);
        let a = &self.path[i];
        let b = &self.path[i + 1];
        let d = b.s - a.s;
        let t = (distance - a.s) / if d == 0. { 1. } else { d };
        [a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t]
    }
}
struct Material {
    i: usize,
    distance: f64,
    s: f64,
    u: f64,
    light: f64,
}
pub(crate) struct Tree {
    pub branches: Vec<Branch>,
    foliage: Vec<Leaf>,
    canopies: Vec<growth::Canopy>,
    materials: Vec<Vec<Material>>,
    pub frame: Option<usize>,
    pub image: Image,
}
impl Tree {
    pub fn new(assets: &Assets) -> Self {
        let (branches, foliage) = growth::skeleton(&assets.tree);
        let canopies = growth::canopies(assets);
        let materials = branches.iter().map(Self::material).collect();
        let image = Drawing::new(W as i32, H as i32).snapshot();
        Self {
            branches,
            foliage,
            canopies,
            materials,
            frame: None,
            image,
        }
    }
    pub fn branch(&self, id: &str) -> &Branch {
        self.branches
            .iter()
            .find(|b| b.id == id)
            .expect("authored branch")
    }
    fn material(b: &Branch) -> Vec<Material> {
        let mut samples: Vec<Option<Material>> = (0..W * H).map(|_| None).collect();
        for pair in b.path.windows(2) {
            let a = &pair[0];
            let z = &pair[1];
            let vx = z.x - a.x;
            let vy = z.y - a.y;
            let l2 = vx * vx + vy * vy;
            if l2 == 0. {
                continue;
            }
            let len = l2.sqrt();
            let nx = -vy / len;
            let ny = vx / len;
            let r = b.radius(a.s, 1.).max(b.radius(z.s, 1.)) + 1.;
            let left = (a.x.min(z.x) - r).floor().max(0.) as usize;
            let right = (a.x.max(z.x) + r).ceil().min((W - 1) as f64) as usize;
            let top = (a.y.min(z.y) - r).floor().max(0.) as usize;
            let bottom = (a.y.max(z.y) + r).ceil().min((H - 1) as f64) as usize;
            for y in top..=bottom {
                for x in left..=right {
                    if b.id == "trunk" && y > GROUND_Y {
                        continue;
                    }
                    let t = clamp(((x as f64 + 0.5 - a.x) * vx + (y as f64 + 0.5 - a.y) * vy) / l2);
                    let dx = x as f64 + 0.5 - (a.x + vx * t);
                    let dy = y as f64 + 0.5 - (a.y + vy * t);
                    let distance = dx.hypot(dy);
                    let i = y * W + x;
                    if distance > r || samples[i].as_ref().is_some_and(|m| m.distance <= distance) {
                        continue;
                    }
                    samples[i] = Some(Material {
                        i,
                        distance,
                        s: a.s + (z.s - a.s) * t,
                        u: dx * nx + dy * ny,
                        light: nx * (-0.65) + ny * (-0.76),
                    });
                }
            }
        }
        samples.into_iter().flatten().collect()
    }
    pub fn render(&mut self, assets: &Assets, progress: f64) {
        let frame = round(clamp(progress) * GROWTH_STEPS as f64) as usize;
        if self.frame == Some(frame) {
            return;
        }
        self.frame = Some(frame);
        let p = frame as f64 / GROWTH_STEPS as f64;
        let mut score = vec![f32::NEG_INFINITY; W * H];
        let mut owners = vec![-1i16; W * H];
        let mut us = vec![0f32; W * H];
        let mut ss = us.clone();
        let mut rs = us.clone();
        let mut lights = us.clone();
        for (index, b) in self.branches.iter().enumerate() {
            let length = b.length(p);
            if length <= 0. {
                continue;
            }
            for m in &self.materials[index] {
                if m.s > length + 1e-8 {
                    continue;
                }
                let radius = b.radius(m.s, p);
                let s = radius - m.distance;
                let i = m.i;
                if s < 0. || s <= score[i] as f64 {
                    continue;
                }
                score[i] = s as f32;
                owners[i] = index as i16;
                us[i] = m.u as f32;
                ss[i] = m.s as f32;
                rs[i] = radius as f32;
                lights[i] = m.light as f32;
            }
        }
        let mut wood = vec![0u8; W * H * 4];
        let mut hero = wood.clone();
        let bark = assets.image(&assets.tree.bark_texture);
        let (bark_width, bark_height) = (bark.width() as usize, bark.height() as usize);
        let bark_pixels = bark.source_pixels();
        let texel = |i: usize| f64::from(bark_pixels[i]);
        for i in 0..W * H {
            let owner = owners[i];
            if owner < 0 {
                continue;
            }
            let (x, y) = (i % W, i / W);
            let def = &self.branches[owner as usize];
            let edge = x == 0
                || y == 0
                || x == W - 1
                || y == H - 1
                || owners[i - 1] < 0
                || owners[i + 1] < 0
                || owners[i - W] < 0
                || owners[i + W] < 0;
            let tx =
                (us[i].floor() as i32 + owner as i32 * 7).rem_euclid(bark_width as i32) as usize;
            let ty =
                (ss[i].floor() as i32 + owner as i32 * 19).rem_euclid(bark_height as i32) as usize;
            let ti = (ty * bark_width + tx) * 4;
            let luminance = (texel(ti) + texel(ti + 1) + texel(ti + 2)) / 3.;
            let above = (((ty + bark_height - 4) % bark_height) * bark_width + tx) * 4;
            let below = (((ty + 4) % bark_height) * bark_width + tx) * 4;
            let nearby = (texel(above)
                + texel(above + 1)
                + texel(above + 2)
                + texel(below)
                + texel(below + 1)
                + texel(below + 2))
                / 6.;
            let relief = (luminance - 113.) / 100. + (luminance - nearby) / 24.;
            let across = (us[i] as f64 / rs[i] as f64).clamp(-1., 1.);
            let shade = 3.25
                + 2.2 * lights[i] as f64 * across
                + 0.7 * (1. - across * across).sqrt()
                + relief * 0.95;
            let mut ci = if edge {
                (shade.floor() - 2.).clamp(0., 7.)
            } else {
                round(shade).clamp(1., 7.)
            } as usize;
            if def.id == "trunk" && rs[i] < 4. {
                ci = ci.min(6);
            }
            wood[i * 4..i * 4 + 3].copy_from_slice(&BARK[ci]);
            wood[i * 4 + 3] = 255;
            if !def.twig {
                hero[i * 4..i * 4 + 4].copy_from_slice(&wood[i * 4..i * 4 + 4]);
            }
        }
        let mut c = Drawing::new(W as i32, H as i32);
        c.image(&image_rgba(W as i32, H as i32, &wood), 0., 0.);
        self.foliage(&mut c, assets, p, false);
        c.image(&image_rgba(W as i32, H as i32, &hero), 0., 0.);
        self.foliage(&mut c, assets, p, true);
        for b in &self.branches {
            if b.root || b.twig {
                continue;
            }
            let length = b.length(p);
            if length == 0.
                && b.parent
                    .as_ref()
                    .is_none_or(|id| self.branch(id).length(p) < b.attach)
            {
                continue;
            }
            if p > b.end + 0.035 {
                continue;
            }
            let [x, y] = b.point(length).map(round);
            c.pixel_rect([x - 1., y - 2., 3., 3.], "#395841");
            c.pixel_rect([x - 1., y - 3., 2., 2.], "#a5b85b");
            c.pixel_rect([x - 1., y - 3., 1., 1.], "#e4d78b");
        }
        self.image = c.snapshot();
    }
    fn blade(c: &mut Drawing, assets: &Assets, asset: &LeafArt, x: f64, y: f64, growth: f64) {
        if growth <= 0. {
            return;
        }
        if growth < 0.12 {
            c.pixel_rect([round(x), round(y) - 1., 1., 2.], "#497646");
            c.pixel_rect([round(x), round(y) - 1., 1., 1.], "#becd79");
            return;
        }
        let image = assets.image(&asset.image);
        let [px, py] = asset.pivot;
        let (w, h) = (image.width() as f64, image.height() as f64);
        let g = clamp((growth - 0.12) / 0.88);
        let length = 0.25 + 0.75 * g.powf(0.63);
        let opening = 0.16 + 0.84 * smooth((g - 0.08) / 0.82);
        let wide = w > h;
        let width = round(w * if wide { length } else { opening }).max(1.);
        let height = round(h * if wide { opening } else { length }).max(2.);
        c.image_scaled(
            &image,
            round(x - px * width / w),
            round(y - py * height / h),
            width,
            height,
        );
    }
    fn foliage(&self, c: &mut Drawing, assets: &Assets, p: f64, front: bool) {
        let leaves = &assets.tree.shoots;
        for leaf in &self.foliage {
            let g = clamp((p - leaf.birth) / (leaf.end - leaf.birth));
            if leaf.front != front || g <= 0. {
                continue;
            }
            let [x, y] = self.branch(leaf.parent.as_str()).point(leaf.attach);
            let asset = leaf.asset;
            if leaf.kind == LeafKind::Shoot {
                let [dx, dy] = leaf.direction;
                let span = leaf.span;
                let reach = clamp(g / 0.46);
                c.pixel_line(
                    [x, y],
                    [x + dx * span * reach, y + dy * span * reach],
                    "#305a39",
                );
                for n in 0..3 {
                    let at = 0.28 + n as f64 * 0.36;
                    let opening = clamp((g - at * 0.46) / 0.54);
                    if opening <= 0. {
                        continue;
                    }
                    let blade = if n == 2 {
                        &leaves[asset]
                    } else {
                        &assets.tree.shoot_sides[n]
                    };
                    Self::blade(
                        c,
                        assets,
                        blade,
                        x + dx * span * at,
                        y + dy * span * at,
                        opening,
                    );
                }
                continue;
            }
            let patch = &self.canopies[asset];
            let [px, py] = patch.pivot;
            let (ox, oy) = (x - px, y - py);
            let rib_growth = clamp(g / growth::RIB_GROWTH_DURATION);
            for rib in &patch.ribs {
                let a = rib.a;
                let b = rib.b;
                c.pixel_line(
                    [ox + a[0], oy + a[1]],
                    [
                        ox + a[0] + (b[0] - a[0]) * rib_growth,
                        oy + a[1] + (b[1] - a[1]) * rib_growth,
                    ],
                    "#305a39",
                );
            }
            for blade in &patch.blades {
                let g = clamp((g - blade.delay) / blade.opening);
                if g <= 0. {
                    continue;
                }
                let reach = clamp(g * 4.);
                let [sx, sy] = blade.stem;
                let x = sx + (blade.x - sx) * reach;
                let y = sy + (blade.y - sy) * reach;
                c.pixel_line([ox + sx, oy + sy], [ox + x, oy + y], "#305a39");
                Self::blade(
                    c,
                    assets,
                    &blade.asset,
                    ox + x,
                    oy + y,
                    clamp((g - 0.12) / 0.88),
                );
            }
        }
    }
}
