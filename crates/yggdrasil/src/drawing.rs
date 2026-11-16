//! A retained drawing plan for Bevy's 2D renderer. No software rasterizer:
//! snapshots describe render-to-texture passes; Bevy draws their quads.
use crate::data::round;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Weak,
};
const MAX_CACHED_LAYERS: usize = 1024;
const MAX_CACHED_DRAWS: usize = 512;
static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);
thread_local! {
    // Weak references preserve live immutable parts without retaining old
    // animation frames. Each renderer thread has its own bounded lookup table.
    static LAYERS: RefCell<HashMap<u64, Weak<ImageData>>> = RefCell::new(HashMap::new());
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BlendMode {
    SrcOver,
    SrcIn,
    SrcATop,
    DstIn,
    DstOut,
    Screen,
}
use bevy::math::Vec4;
#[derive(Clone)]
pub(crate) struct Image(pub Arc<ImageData>);
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        // Immutable images are handles: comparing them never walks their pixels
        // or recursively compares the drawing graph.
        self.0.id == other.0.id
    }
}
pub(crate) struct ImageData {
    pub id: u64,
    pub w: i32,
    pub h: i32,
    pub content: Content,
}
pub(crate) enum Content {
    Pixels(Vec<u8>),
    Region { atlas: Image, rect: [u32; 4] },
    Layer(Vec<Draw>),
}
impl Image {
    fn new(w: i32, h: i32, content: Content) -> Self {
        Self(Arc::new(ImageData {
            id: NEXT_IMAGE.fetch_add(1, Ordering::Relaxed),
            w,
            h,
            content,
        }))
    }
    pub fn width(&self) -> i32 {
        self.0.w
    }
    pub fn height(&self) -> i32 {
        self.0.h
    }
    pub fn source_pixels(&self) -> Cow<'_, [u8]> {
        match &self.0.content {
            Content::Pixels(p) => Cow::Borrowed(p),
            Content::Region {
                atlas,
                rect: [x, y, w, h],
            } => {
                let source = atlas.source_pixels();
                let stride = atlas.width() as usize * 4;
                let mut pixels = Vec::with_capacity((w * h * 4) as usize);
                for row in source
                    .chunks_exact(stride)
                    .skip(*y as usize)
                    .take(*h as usize)
                {
                    pixels.extend_from_slice(&row[*x as usize * 4..(x + w) as usize * 4]);
                }
                Cow::Owned(pixels)
            }
            _ => panic!("ownership masks must use authored source pixels"),
        }
    }
}
pub(crate) fn image_rgba(w: i32, h: i32, rgba: &[u8]) -> Image {
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    Image::new(w, h, Content::Pixels(rgba.to_vec()))
}
pub(crate) fn image_region(atlas: &Image, rect: [u32; 4]) -> Image {
    Image::new(
        rect[2] as i32,
        rect[3] as i32,
        Content::Region {
            atlas: atlas.clone(),
            rect,
        },
    )
}
#[derive(Clone, PartialEq)]
pub(crate) enum Shape {
    Image {
        image: Image,
        source: [f32; 4],
        clean: bool,
    },
    Solid,
    Ellipse,
    Radial {
        center: [f32; 2],
        radii: [f32; 2],
        stops: Vec<(f32, Vec4)>,
    },
}
#[derive(Clone, PartialEq)]
pub(crate) struct Draw {
    pub shape: Shape,
    pub rect: [f32; 4],
    pub transform: [f32; 6],
    pub color: [f32; 4],
    pub blend: BlendMode,
}
pub(crate) struct Drawing {
    pub width: i32,
    pub height: i32,
    draws: Vec<Draw>,
    transform: [f32; 6],
    stack: Vec<[f32; 6]>,
}
impl Drawing {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            width: w,
            height: h,
            draws: Vec::new(),
            transform: [1., 0., 0., 0., 1., 0.],
            stack: Vec::new(),
        }
    }
    pub fn snapshot(&self) -> Image {
        let create = || Image::new(self.width, self.height, Content::Layer(self.draws.clone()));
        // Large branch/foliage plans already have a dedicated growth cache.
        if self.draws.len() > MAX_CACHED_DRAWS {
            return create();
        }
        let mut hash = DefaultHasher::new();
        (self.width, self.height).hash(&mut hash);
        for d in &self.draws {
            d.rect.map(f32::to_bits).hash(&mut hash);
            d.transform.map(f32::to_bits).hash(&mut hash);
            d.color.map(f32::to_bits).hash(&mut hash);
            d.blend.hash(&mut hash);
            std::mem::discriminant(&d.shape).hash(&mut hash);
            match &d.shape {
                Shape::Image {
                    image,
                    source,
                    clean,
                } => {
                    image.0.id.hash(&mut hash);
                    source.map(f32::to_bits).hash(&mut hash);
                    clean.hash(&mut hash);
                }
                Shape::Radial {
                    center,
                    radii,
                    stops,
                } => {
                    center.map(f32::to_bits).hash(&mut hash);
                    radii.map(f32::to_bits).hash(&mut hash);
                    for (t, c) in stops {
                        t.to_bits().hash(&mut hash);
                        c.to_array().map(f32::to_bits).hash(&mut hash);
                    }
                }
                _ => {}
            }
        }
        let key = hash.finish();
        LAYERS.with_borrow_mut(|cache| {
            if let Some(cached) = cache.get(&key).and_then(Weak::upgrade)
                && cached.w == self.width
                && cached.h == self.height
                && let Content::Layer(draws) = &cached.content
                && draws == &self.draws
            {
                return Image(cached);
            }
            if cache.len() >= MAX_CACHED_LAYERS {
                cache.retain(|_, image| image.strong_count() > 0);
                if cache.len() >= MAX_CACHED_LAYERS {
                    cache.clear();
                }
            }
            let image = create();
            cache.insert(key, Arc::downgrade(&image.0));
            image
        })
    }
    pub fn save(&mut self) {
        self.stack.push(self.transform);
    }
    pub fn restore(&mut self) {
        self.transform = self.stack.pop().expect("balanced yggdrasil transforms");
    }
    pub fn translate(&mut self, x: f64, y: f64) {
        let m = &mut self.transform;
        m[2] += m[0] * x as f32 + m[1] * y as f32;
        m[5] += m[3] * x as f32 + m[4] * y as f32;
    }
    pub fn scale(&mut self, x: f64, y: f64) {
        let m = &mut self.transform;
        m[0] *= x as f32;
        m[3] *= x as f32;
        m[1] *= y as f32;
        m[4] *= y as f32;
    }
    pub fn rotate(&mut self, r: f64) {
        let (s, c) = (r as f32).sin_cos();
        let m = self.transform;
        self.transform = [
            m[0] * c + m[1] * s,
            -m[0] * s + m[1] * c,
            m[2],
            m[3] * c + m[4] * s,
            -m[3] * s + m[4] * c,
            m[5],
        ];
    }
    fn push(&mut self, shape: Shape, rect: [f64; 4], color: [f32; 4], blend: BlendMode) {
        self.draws.push(Draw {
            shape,
            rect: rect.map(|v| v as f32),
            transform: self.transform,
            color,
            blend,
        });
    }
    pub fn image(&mut self, image: &Image, x: f64, y: f64) {
        self.image_scaled(image, x, y, image.width() as f64, image.height() as f64);
    }
    pub fn image_scaled(&mut self, image: &Image, x: f64, y: f64, w: f64, h: f64) {
        self.image_rect(image, None, [x, y, w, h], 1., BlendMode::SrcOver);
    }
    pub fn image_rect(
        &mut self,
        image: &Image,
        source: Option<[f64; 4]>,
        destination: [f64; 4],
        alpha: f64,
        blend: BlendMode,
    ) {
        let source = source
            .unwrap_or([0., 0., image.width() as f64, image.height() as f64])
            .map(|v| v as f32);
        self.push(
            Shape::Image {
                image: image.clone(),
                source,
                clean: false,
            },
            destination,
            [1., 1., 1., alpha as f32],
            blend,
        );
    }
    pub fn fill(&mut self, r: [f64; 4], color: &str, alpha: f64, blend: BlendMode) {
        self.push(Shape::Solid, r, color_rgba(color, alpha), blend);
    }
    pub fn pixel_rect(&mut self, r: [f64; 4], color: &str) {
        self.fill(r.map(round), color, 1., BlendMode::SrcOver);
    }
    pub fn ellipse(&mut self, x: f64, y: f64, rx: f64, ry: f64, color: &str, alpha: f64) {
        self.push(
            Shape::Ellipse,
            [x - rx, y - ry, rx * 2., ry * 2.],
            color_rgba(color, alpha),
            BlendMode::SrcOver,
        );
    }
    pub fn radial(
        &mut self,
        center: [f64; 2],
        r0: f64,
        r1: f64,
        stops: &[(f32, Vec4)],
        bounds: [f64; 4],
        blend: BlendMode,
    ) {
        self.push(
            Shape::Radial {
                center: center.map(|v| v as f32),
                radii: [r0 as f32, r1 as f32],
                stops: stops.to_vec(),
            },
            bounds,
            [1.; 4],
            blend,
        );
    }
    pub fn clean_specks(&mut self) {
        let im = self.snapshot();
        self.draws.clear();
        self.push(
            Shape::Image {
                source: [0., 0., self.width as f32, self.height as f32],
                image: im,
                clean: true,
            },
            [0., 0., self.width as f64, self.height as f64],
            [1.; 4],
            BlendMode::SrcOver,
        );
    }
    pub fn pixel_line(&mut self, mut a: [f64; 2], mut b: [f64; 2], color: &str) {
        a = a.map(round);
        b = b.map(round);
        let dx = (b[0] - a[0]).abs();
        let sx = if a[0] < b[0] { 1. } else { -1. };
        let dy = -(b[1] - a[1]).abs();
        let sy = if a[1] < b[1] { 1. } else { -1. };
        let mut e = dx + dy;
        loop {
            self.pixel_rect([a[0], a[1], 1., 1.], color);
            if a == b {
                break;
            }
            let ee = 2. * e;
            if ee >= dy {
                e += dy;
                a[0] += sx;
            }
            if ee <= dx {
                e += dx;
                a[1] += sy;
            }
        }
    }
}
fn color_rgba(color: &str, alpha: f64) -> [f32; 4] {
    let n = u32::from_str_radix(color.trim_start_matches('#'), 16).expect("authored colour");
    [
        ((n >> 16) & 255) as f32 / 255.,
        ((n >> 8) & 255) as f32 / 255.,
        (n & 255) as f32 / 255.,
        alpha as f32,
    ]
}
