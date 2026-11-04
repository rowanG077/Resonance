//! Decode Steam model geometry directly into the common GLB/scene representation.
use crate::{package, pe};
use anyhow::{Context, Result, ensure};
mod appearance;
mod bindings;
mod glb;
mod material;
mod mesh;
mod skeleton;
mod write;
pub(crate) use appearance::{party_appearance, tagged_appearance};
pub(super) use material::Materials;
pub(super) use mesh::Mesh;
pub(super) use skeleton::Node;
pub(crate) use write::{cook, cook_on_rig, material_alpha_cutoffs, material_parameters};

pub struct Rig {
    pub nodes: Vec<Node>,
    pub animation_bindings: Vec<(u32, u32)>,
}

pub struct Model {
    pub rig: Rig,
    pub meshes: Vec<Mesh>,
    pub materials: Materials,
}

impl Rig {
    pub fn joint_names(&self) -> Vec<String> {
        joint_names(
            &self
                .nodes
                .iter()
                .map(|n| canonical_joint_name(&n.name))
                .collect::<Vec<_>>(),
        )
    }

    /// Preserve authored visibility; only numbered native effect anchors are
    /// hidden by convention. Ordinary prop roots such as KK_OBJ000_01 draw.
    pub(crate) fn hidden_geometry(&self) -> Vec<String> {
        self.nodes
            .iter()
            .zip(self.joint_names())
            .filter_map(|(node, name)| (!node.visible || attachment_marker(&name)).then_some(name))
            .collect()
    }

    pub fn read(descriptor: &[u8]) -> Result<Self> {
        let source = Reader(descriptor);
        let sections = sections(&source)?;
        let pose = bindings::read(&source, sections[11])?;
        Ok(Self {
            nodes: skeleton::read(&source, sections[0], &pose)?,
            animation_bindings: pose.channels,
        })
    }
}

fn attachment_marker(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 4
        && bytes[..2].eq_ignore_ascii_case(b"KK")
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && (bytes.len() == 4 || bytes[4] == b'_')
}

/// Reuse the GC semantic dictionary. Only source spelling is case folded, and
/// conflicting meanings in a folded group never select an arbitrary target.
fn canonical_joint_name(name: &str) -> String {
    use std::{collections::BTreeMap, sync::LazyLock};
    static ALIASES: LazyLock<BTreeMap<String, String>> =
        LazyLock::new(|| serde_json::from_str(include_str!("model/joint-names.json")).unwrap());
    static NAMES: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../../importer/src/model-joints.json")).unwrap()
    });
    static FOLDED: LazyLock<BTreeMap<String, Option<String>>> =
        LazyLock::new(|| casefold_names(&NAMES));
    let name = ALIASES
        .get(&name.to_ascii_uppercase())
        .map_or(name, String::as_str);
    NAMES
        .get(name)
        .or_else(|| {
            FOLDED
                .get(&name.to_ascii_uppercase())
                .and_then(Option::as_ref)
        })
        .cloned()
        .unwrap_or_else(|| {
            // KKnn is a native effect attachment marker, not an arbitrary
            // model label. GC publishes its reserved prefix as kk; preserve
            // the source slot and descriptive suffix, including its case.
            if attachment_marker(name) {
                format!("kk{}", &name[2..])
            } else {
                name.to_owned()
            }
        })
}

fn casefold_names(
    names: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, Option<String>> {
    let mut folded = std::collections::BTreeMap::new();
    for (name, target) in names {
        let entry = folded
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| Some(target.clone()));
        if entry.as_ref().is_some_and(|previous| previous != target) {
            *entry = None;
        }
    }
    folded
}

#[cfg(test)]
mod joint_tests {
    use super::*;

    #[test]
    fn steam_source_spellings_use_gc_semantic_joints_without_renaming_unknowns() {
        for (source, target) in [
            ("BONE_ATAMA", "head"),
            ("BONE_SEBONE01", "spine-01"),
            ("BONE_KATA_R", "shoulder-right"),
            ("Bone_ude01_R", "arm-01-right"),
            ("HIDE04_A", "hide04_a"),
            ("WORLD2", "world2"),
            ("PA00_ARM_R", "PA00_Arm_R"),
            ("PA01_ARM_R", "PA01_Arm_R"),
            ("PA02_ARM_L", "PA02_Arm_L"),
            ("PA03_ARM_L", "PA03_Arm_L"),
            ("KK00", "kk00"),
            ("KK00_BE19_00", "kk00_BE19_00"),
            ("KK02_WEP1_L", "kk02_WEP1_L"),
            ("KK00_TUE", "kk00_TUE"),
            ("KK00NotAnAttachment", "KK00NotAnAttachment"),
            ("KK0X_Other", "KK0X_Other"),
            ("UntouchedNativeThing", "UntouchedNativeThing"),
        ] {
            assert_eq!(canonical_joint_name(source), target);
        }
        let names = ["BONE_ATAMA", "head", "head-2"].map(canonical_joint_name);
        assert_eq!(joint_names(&names), ["head", "head-3", "head-2"]);
        let names = ["KK00_Marker", "kk00_Marker"].map(canonical_joint_name);
        assert_eq!(joint_names(&names), ["kk00_Marker", "kk00_Marker-2"]);
    }

    #[test]
    fn ambiguous_casefolds_have_no_implicit_target() {
        let names = [
            ("Bone".into(), "first".into()),
            ("BONE".into(), "second".into()),
        ]
        .into();
        assert_eq!(casefold_names(&names)["BONE"], None);
    }
}

impl Model {
    pub fn read(descriptor: &[u8], payload: &[u8]) -> Result<Self> {
        let source = Reader(descriptor);
        let sections = sections(&source)?;
        let dependencies = package::dependencies(descriptor)?;
        ensure!(
            dependencies
                .first()
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(".tomdlp_p")),
            "Steam model has no geometry payload binding"
        );
        let rig = Rig::read(descriptor)?;
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
            &rig.nodes,
            materials.records.len(),
        )?;
        Ok(Self {
            rig,
            meshes,
            materials,
        })
    }
}

fn sections(source: &Reader<'_>) -> Result<[usize; 12]> {
    ensure!(
        source.0.get(12..16) == Some(b"BLDM") && source.word(16)? == 210,
        "unsupported Steam model descriptor"
    );
    let mut sections = [0; 12];
    for (i, section) in sections.iter_mut().enumerate() {
        *section = source.relative(20 + i * 4)?;
        ensure!(
            (68..source.0.len()).contains(section),
            "model section exceeds descriptor"
        );
    }
    Ok(sections)
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
