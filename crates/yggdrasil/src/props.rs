use crate::{
    actors::Performers,
    data::{clamp, round, smooth},
    drawing::{BlendMode, Drawing},
    exposure,
    scene::{Actor, Grip, Layer, Placement, Pose, Prop, PropKind},
    timeline::SceneState,
};
const PROP_LAYER: i32 = 256;
const PROP_ORIGIN: f64 = PROP_LAYER as f64 / 2.;

impl Performers<'_> {
    pub fn prop_position(&self, p: &Prop, time: f64) -> Prop {
        let mut r = p.clone();
        if p.support_wrap && p.actor.is_none() {
            let b = self.tree.branch("trunk");
            r.wrap_centre = crate::ENVIRONMENT_SCALE * b.point(5.)[0];
            r.wrap_radius = crate::ENVIRONMENT_SCALE * b.radius(5., self.progress);
        } else if let Some(a) = &p.tree_attach {
            let b = self.tree.branch(&a.branch);
            let at = b.point(a.distance);
            r.x = crate::ENVIRONMENT_SCALE * (at[0] + a.offset[0]);
            r.y = crate::ENVIRONMENT_SCALE * (at[1] + a.offset[1]);
            r.tree_radius = crate::ENVIRONMENT_SCALE * b.radius(a.distance, self.progress);
        } else if let Some(a) = &p.rest_anchor
            && matches!(p.placement, Placement::Drop | Placement::Ground)
        {
            let rest = self.hand(
                &self.timeline.actor_at(&p.event_id, &a.actor, a.time),
                None,
                Grip::Right,
            );
            let [mut x, mut y] = rest;
            if p.placement == Placement::Drop {
                let at = a.drop_at - 1e-6;
                let release = self.hand(
                    &self.timeline.actor_at(&p.event_id, &a.actor, at),
                    None,
                    Grip::Right,
                );
                let u = clamp((time - a.drop_at) / (a.rest_at - a.drop_at));
                x = release[0] + (rest[0] - release[0]) * u;
                y = release[1] + (rest[1] - release[1]) * u * u;
            }
            r.x = x;
            r.y = y;
            r.grounded = p.placement == Placement::Ground;
        } else if let Some(a) = &p.actor
            && (p.anchor.is_some() || p.grip.is_some() || p.kind == PropKind::Can && a.held)
        {
            let anchor = p.anchor.map(|[x, y]| [x, y + p.pin_drop]);
            let [mut x, mut y] = self.hand(a, anchor, p.grip.unwrap_or(Grip::Right));
            if let Some([dx, dy]) = p.grip_offset {
                let angle = p.angle.to_radians();
                x += r.face() * (dx * angle.cos() - dy * angle.sin());
                y += dx * angle.sin() + dy * angle.cos();
            }
            r.x = x;
            r.y = y;
        }
        r
    }
    fn grip(&self, c: &mut Drawing, a: &Actor, slot: Grip, width: Option<f64>, height: f64) {
        if !self.assets.data.rigs.contains_key(&a.id) || a.turn_front || !a.held {
            return;
        }
        if slot == Grip::Both {
            self.grip(c, a, Grip::Right, width, height);
            self.grip(c, a, Grip::Left, width, height);
            return;
        }
        let cel = self.held_cel(a);
        let [hx, hy] = cel.hand(slot);
        let w = width.unwrap_or(if a.resting() { 12. } else { 4. });
        let l = round(hx - w / 2.);
        let t = hy - (height / 2.).floor();
        let r = l + w;
        let b = t + height;
        let mask = self.mask(
            &self.assets.image(&cel.cel.image),
            &[
                [l, t + 1.],
                [l + 1., t],
                [r - 1., t],
                [r, t + 1.],
                [r, b],
                [l, b],
            ],
        );
        self.overlay(c, a, &mask, cel.pivot, cel.angle, cel.drop);
    }
    fn forearm(&self, c: &mut Drawing, a: &Actor, points: &[[f64; 2]]) {
        let cel = self.held_cel(a);
        let mask = self.mask(&self.assets.image(&cel.cel.image), points);
        self.overlay(c, a, &mask, cel.pivot, cel.angle, cel.drop);
    }
    fn spout(p: &Prop) -> [f64; 2] {
        let a = p.angle * std::f64::consts::PI / 180.;
        [
            p.x + p.face() * (22. * a.cos() - 10. * a.sin()),
            p.y + 22. * a.sin() + 10. * a.cos(),
        ]
    }
    fn water(&self, c: &mut Drawing, p: &Prop, time: f64) {
        let Some(flow) = &p.flow else {
            return;
        };
        let event = self.timeline.event(&p.event_id).unwrap();
        const DROP_INTERVAL: f64 = 0.045;
        const MAX_DROP_AGE: f64 = 0.85;
        const GRAVITY: f64 = 230.; // scene pixels / second²
        const INITIAL_FALL_SPEED: f64 = 12.; // scene pixels / second
        const SPLASH_LIFETIME: f64 = 0.15;
        let mut n = ((time - flow.start - MAX_DROP_AGE) / DROP_INTERVAL)
            .floor()
            .max(0.);
        while n <= (time - flow.start) / DROP_INTERVAL {
            let born = flow.start + n * DROP_INTERVAL;
            if born > flow.end {
                break;
            }
            let cel_time = exposure(born);
            let sample = event.sample(cel_time);
            let emitted = sample.props.iter().find(|q| q.id == p.id).unwrap();
            let pose = self.prop_position(emitted, cel_time);
            let [sx, sy] = Self::spout(&pose);
            let height = (flow.target[1] - sy).max(1.);
            let life = (-INITIAL_FALL_SPEED
                + (INITIAL_FALL_SPEED * INITIAL_FALL_SPEED + 2. * GRAVITY * height).sqrt())
                / GRAVITY;
            let age = time - born;
            for stream in 0..3 {
                let tx = flow.target[0] + (stream as f64 - 1.) * 2.;
                let vx = (tx - sx) / life;
                let drop = if age <= life {
                    Some((
                        sx + vx * age,
                        sy + INITIAL_FALL_SPEED * age + GRAVITY * age * age / 2.,
                        2.,
                    ))
                } else if age - life < SPLASH_LIFETIME {
                    let u = (age - life) / SPLASH_LIFETIME;
                    Some((
                        tx + (stream as f64 - 1.) * u * 4.,
                        flow.target[1] - (u * std::f64::consts::PI).sin() * 2.,
                        1.,
                    ))
                } else {
                    None
                };
                if let Some((x, y, h)) = drop {
                    c.pixel_rect(
                        [round(x), round(y), 1., h],
                        if stream == 1 { "#e2eee0" } else { "#93c8ca" },
                    );
                }
            }
            n += 1.;
        }
    }
    pub fn prop(&self, c: &mut Drawing, source: &Prop, time: f64, effect_time: f64) {
        if !source.visible {
            return;
        }
        let p = self.prop_position(source, time);
        let mut local = Drawing::new(PROP_LAYER, PROP_LAYER);
        local.save();
        local.translate(PROP_ORIGIN, PROP_ORIGIN);
        let art = &self.assets.data.props;
        let key = if p.kind == PropKind::Bowl && p.empty {
            "bowl-empty"
        } else {
            p.kind.asset()
        };
        let sprite = || self.assets.image(&art[key].image);
        let draw = |c: &mut Drawing, key: &str| {
            let asset = &art[key];
            c.image_rect(
                &self.assets.image(&asset.image),
                None,
                asset.rect,
                1.,
                BlendMode::SrcOver,
            );
        };
        let blit = |c: &mut Drawing, im: &crate::drawing::Image, s: [f64; 4], d: [f64; 4]| {
            c.image_rect(im, Some(s), d, 1., BlendMode::SrcOver);
        };
        match p.kind {
            PropKind::CharmCord => {
                let im = sprite();
                let r = p.tree_radius.floor().max(1.);
                let left = (r - 2. * r * clamp(p.wrapped)).ceil() as i32;
                for x in left..=r as i32 {
                    blit(
                        &mut local,
                        &im,
                        [((x as f64 + r) % im.width() as f64), 0., 1., 4.],
                        [
                            x as f64,
                            round(2. * (1. - (x as f64 / r).powi(2))) - 1.,
                            1.,
                            3.,
                        ],
                    );
                }
            }
            PropKind::ManaSpark => {
                let s = p.strength;
                if s > 0.15 {
                    local.pixel_rect([0., 0., 1., 1.], "#f1efce");
                    if s > 0.55 {
                        for [x, y] in [[-1., 0.], [1., 0.], [0., -1.], [0., 1.]] {
                            local.pixel_rect([x, y, 1., 1.], "#b7dbc1");
                        }
                    }
                }
            }
            PropKind::Bread => {
                let part = p.bread_half as f64;
                let cel = if p.bitten {
                    if part == 0. { 1. } else { 2. }
                } else {
                    0.
                };
                let rect = art[key].rect;
                blit(
                    &mut local,
                    &sprite(),
                    [(cel * 2. + part) * rect[2], 0., rect[2], rect[3]],
                    rect,
                );
            }
            PropKind::Flower => {
                draw(&mut local, key);
                if p.pinned {
                    local.pixel_rect([-2., 0., 5., 2.], "#a98856");
                    local.pixel_rect([-1., 0., 3., 1.], "#f3dba2");
                }
            }
            PropKind::HearthStone => {
                let rect = art[key].rect;
                blit(
                    &mut local,
                    &sprite(),
                    [p.cel as f64 * rect[2], 0., rect[2], rect[3]],
                    rect,
                );
            }
            PropKind::Tie => {
                if p.support_wrap {
                    let braid = self.assets.image(&art["charm-cord"].image);
                    let left = round(self.assets.data.support_x - p.x);
                    let right = round(p.wrap_centre + p.wrap_radius - p.x);
                    for x in left as i32..=right as i32 {
                        blit(
                            &mut local,
                            &braid,
                            [(x as f64 - left) % braid.width() as f64, 0., 1., 4.],
                            [
                                x as f64,
                                round(
                                    2. * (std::f64::consts::PI * (x as f64 - left)
                                        / (right - left).max(1.))
                                    .sin(),
                                ),
                                1.,
                                3.,
                            ],
                        );
                    }
                }
                draw(&mut local, key);
            }
            PropKind::Book => {
                let open = clamp(p.open);
                if open < 0.15 {
                    draw(&mut local, "book-closed");
                } else {
                    let half = round(12. + 7. * open);
                    let im = sprite();
                    blit(
                        &mut local,
                        &im,
                        [0., 0., 19., 24.],
                        [-half, -12., half, 24.],
                    );
                    blit(&mut local, &im, [19., 0., 19., 24.], [0., -12., half, 24.]);
                }
            }
            PropKind::Log => {
                let im = sprite();
                draw(&mut local, key);
                if p.charcoal > 0. {
                    local.fill(
                        [
                            -im.width() as f64 / 2. - 1.,
                            -im.height() as f64 / 2. - 1.,
                            im.width() as f64 + 2.,
                            im.height() as f64 + 2.,
                        ],
                        "#281f23",
                        clamp(p.charcoal),
                        BlendMode::SrcATop,
                    );
                }
            }
            PropKind::Blanket => {
                let im = sprite();
                let open = p.open;
                let w = im.width() as f64;
                let h = im.height() as f64;
                let hw = w / 2.;
                let hh = h / 2.;
                let fx = (std::f64::consts::PI * (1. - smooth(open * 2.))).cos();
                let fy = (std::f64::consts::PI * (1. - smooth(open * 2. - 1.))).cos();
                for (left, top) in [(false, false), (true, false), (false, true), (true, true)] {
                    local.save();
                    local.scale(if left { fx } else { 1. }, if top { fy } else { 1. });
                    blit(
                        &mut local,
                        &im,
                        [
                            if left { 0. } else { hw },
                            if top { 0. } else { hh },
                            hw,
                            hh,
                        ],
                        [
                            if left { -hw } else { 0. },
                            if top { -hh } else { 0. },
                            hw,
                            hh,
                        ],
                    );
                    local.restore();
                }
            }
            PropKind::Campfire => {
                let heat = p.heat;
                if heat > 0. {
                    for i in 0..7 {
                        local.pixel_rect(
                            [-18. + i as f64 * 6., 7. + (i % 2) as f64, 3., 1.],
                            if heat > 0.3 { "#d88943" } else { "#985444" },
                        );
                    }
                }
                if heat > 0.13 {
                    let size = round(48. * clamp(heat).sqrt());
                    let baseline = round(size * 44. / 48.);
                    let cel = (effect_time * 8.).floor() % 8.;
                    blit(
                        &mut local,
                        &self.assets.image(&art["camp-flames"].image),
                        [cel * 48., 0., 48., 48.],
                        [-round(size / 2.), 5. - baseline, size, size],
                    );
                }
            }
            _ => draw(&mut local, key),
        }
        local.restore();
        if p.kind == PropKind::Bowl && p.grounded {
            c.ellipse(
                round(p.x),
                round(p.y + 7. * p.scale),
                9.,
                2.,
                "#232a27",
                0.22,
            );
        }
        if p.kind == PropKind::Can && p.grounded {
            c.ellipse(round(p.x), round(p.y + 22.), 10., 2., "#23392b", 0.18);
        }
        c.save();
        c.translate(round(p.x), round(p.y));
        c.scale(p.face() * p.scale, p.scale);
        c.rotate(p.angle * std::f64::consts::PI / 180.);
        c.image(&local.snapshot(), -PROP_ORIGIN, -PROP_ORIGIN);
        c.restore();
        if let Some(a) = &p.actor {
            if p.kind == PropKind::Book && a.id == "raine" {
                let masks = &self.assets.data.overlays.book;
                let mask = if let Some(cel) = a.clip() {
                    &masks.acting[cel.index]
                } else {
                    &masks.profile
                };
                self.forearm(c, a, mask);
            }
            if p.kind == PropKind::Log
                && a.id == "presea"
                && a.clip().is_none_or(|c| c.name != "gather" || c.index == 5)
            {
                self.forearm(c, a, &self.assets.data.overlays.log);
            } else if p.kind == PropKind::Can || p.grip.is_some() {
                self.grip(
                    c,
                    a,
                    p.grip.unwrap_or(Grip::Right),
                    p.finger_width,
                    p.finger_height.unwrap_or(3.),
                );
            }
        }
        if p.kind == PropKind::Can {
            self.water(c, &p, effect_time);
        }
    }
    pub fn draw(&self, c: &mut Drawing, state: &SceneState, time: f64, rest_time: f64) {
        let cel_time = state.time;
        let effect_time = exposure(if state.finished { rest_time } else { time });
        let loose: Vec<_> = state.props.iter().filter(|p| p.actor.is_none()).collect();
        let held: Vec<_> = state.props.iter().filter(|p| p.actor.is_some()).collect();
        for p in loose.iter().filter(|p| p.layer == Some(Layer::Back)) {
            self.prop(c, p, cel_time, effect_time);
        }
        enum Item<'a> {
            Actor(&'a Actor),
            Prop(&'a Prop),
        }
        let mut things: Vec<_> = state
            .actors
            .iter()
            .map(|a| (a.y, Item::Actor(a)))
            .chain(
                loose
                    .iter()
                    .filter(|p| p.layer.is_none())
                    .map(|p| (p.z.unwrap_or(p.y), Item::Prop(p))),
            )
            .collect();
        things.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, item) in things {
            match item {
                Item::Prop(p) => self.prop(c, p, cel_time, effect_time),
                Item::Actor(a) => {
                    let own: Vec<_> = held
                        .iter()
                        .filter(|p| p.actor.as_ref().unwrap().id == a.id)
                        .collect();
                    for p in own.iter().filter(|p| p.layer == Some(Layer::Back)) {
                        self.prop(c, p, cel_time, effect_time);
                    }
                    self.actor(
                        c,
                        a,
                        if a.pose == Pose::Sleep {
                            exposure(rest_time)
                        } else {
                            cel_time
                        },
                    );
                    for p in own.iter().filter(|p| p.layer != Some(Layer::Back)) {
                        self.prop(c, p, cel_time, effect_time);
                    }
                    if a.id == "noishe" {
                        for a in state.actors.iter().filter(|a| {
                            a.clip()
                                .is_some_and(|c| c.name == "pet" && matches!(c.index, 2..=4))
                                && !a.moving
                        }) {
                            let cel = self.held_cel(a);
                            let image = self.assets.image(&cel.cel.image);
                            let mask = self.mask(&image, &self.assets.data.overlays.pet);
                            self.overlay(c, a, &mask, 116., 0., 0.);
                        }
                    }
                }
            }
        }
        for p in loose.iter().filter(|p| p.layer == Some(Layer::Front)) {
            self.prop(c, p, cel_time, effect_time);
        }
    }
}
