//! Decode Steam model geometry directly into the common GLB/scene representation.
use crate::{package, pe};
use anyhow::{Context, Result, ensure};
mod bindings;
mod glb;
mod material;
mod mesh;
mod skeleton;
mod write;
pub(super) use material::Materials;
pub(super) use mesh::Mesh;
pub(super) use skeleton::Node;
pub(crate) use write::cook;

pub struct Model {
    pub nodes: Vec<Node>,
    pub animation_bindings: Vec<(u32, u32)>,
    pub meshes: Vec<Mesh>,
    pub materials: Materials,
}

impl Model {
    pub fn joint_names(&self) -> Vec<String> {
        // These shared gameplay anchors have stable names in both cooks.
        // Normalize before writing either the body or its animation tracks.
        static NAMES: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
            std::sync::LazyLock::new(|| {
                serde_json::from_str(include_str!("model/joint-names.json")).unwrap()
            });
        joint_names(
            &self
                .nodes
                .iter()
                .map(|n| {
                    NAMES
                        .get(&n.name.to_ascii_uppercase())
                        .unwrap_or(&n.name)
                        .clone()
                })
                .collect::<Vec<_>>(),
        )
    }

    pub fn read(descriptor: &[u8], payload: &[u8]) -> Result<Self> {
        let source = Reader(descriptor);
        ensure!(
            descriptor.get(12..16) == Some(b"BLDM") && source.word(16)? == 210,
            "unsupported Steam model descriptor"
        );
        let dependencies = package::dependencies(descriptor)?;
        ensure!(
            dependencies
                .first()
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(".tomdlp_p")),
            "Steam model has no geometry payload binding"
        );
        let mut sections = [0; 12];
        for (i, section) in sections.iter_mut().enumerate() {
            *section = source.relative(20 + i * 4)?;
            ensure!(
                (68..descriptor.len()).contains(section),
                "model section exceeds descriptor"
            );
        }
        let pose = bindings::read(&source, sections[11])?;
        let nodes = skeleton::read(&source, sections[0], &pose)?;
        let materials = material::read(&source, sections[7])?;
        for texture in &materials.textures {
            let descriptor = format!("{}.totexb_d", texture.name);
            ensure!(
                dependencies
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&descriptor)),
                "material texture {descriptor} is absent from model dependencies"
            );
        }
        let meshes = mesh::read(
            &source,
            sections[6],
            payload,
            &nodes,
            materials.records.len(),
        )?;
        Ok(Self {
            nodes,
            animation_bindings: pose.channels,
            meshes,
            materials,
        })
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn word(&self, at: usize) -> Result<u32> {
        pe::word(self.0, at)
    }
    fn half(&self, at: usize) -> Result<u16> {
        pe::half(self.0, at)
    }
    fn signed(&self, at: usize) -> Result<i16> {
        Ok(self.half(at)? as i16)
    }
    fn relative(&self, at: usize) -> Result<usize> {
        at.checked_add(self.word(at)? as usize)
            .context("Steam model pointer overflow")
    }
    fn slice(&self, at: usize, length: usize) -> Result<&'a [u8]> {
        self.0
            .get(at..)
            .and_then(|bytes| bytes.get(..length))
            .context("Steam model data exceeds resource")
    }
    fn table(&self, at: usize, stride: usize) -> Result<Table> {
        let start = self.relative(at)?;
        let count = self.word(at + 4)? as usize;
        let size = count
            .checked_mul(stride)
            .context("Steam model table size overflow")?;
        ensure!(
            count <= 65536 && (count == 0 || start > at + 4),
            "invalid Steam model table"
        );
        self.slice(start, size)?;
        Ok(Table {
            start,
            count,
            stride,
        })
    }
    fn floats<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut values = [0.; N];
        for (i, value) in values.iter_mut().enumerate() {
            *value = f32::from_bits(self.word(at + i * 4)?);
            ensure!(value.is_finite(), "non-finite model scalar");
        }
        Ok(values)
    }
    fn name(&self, pointer: usize) -> Result<String> {
        let at = self.relative(pointer)?;
        let bytes = pe::string(self.0, at)?.as_bytes();
        ensure!(
            !bytes.is_empty() && bytes.len() <= 4096,
            "invalid Steam model label"
        );
        Ok(std::str::from_utf8(bytes)?.to_owned())
    }
}

struct Table {
    start: usize,
    count: usize,
    stride: usize,
}
impl Table {
    fn at(&self, index: usize) -> usize {
        self.start + index * self.stride
    }
}

fn joint_names(names: &[String]) -> Vec<String> {
    let reserved: std::collections::BTreeSet<_> = names.iter().cloned().collect();
    let mut used = std::collections::BTreeSet::new();
    names
        .iter()
        .map(|name| {
            if used.insert(name.clone()) {
                return name.clone();
            }
            let mut suffix = 2;
            loop {
                let candidate = format!("{name}-{suffix}");
                if !reserved.contains(&candidate) && used.insert(candidate.clone()) {
                    return candidate;
                }
                suffix += 1;
            }
        })
        .collect()
}
