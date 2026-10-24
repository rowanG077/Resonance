//! Native dynamic resource directories publish directly into scoped owners.
use super::*;
use resonance_content::resource::{self, Definition, Resource};
use std::path::PathBuf;

impl Publisher<'_> {
    pub(crate) fn resource_catalogue(
        &mut self,
        discs: &[PathBuf],
        catalogue: &crate::resource::Catalogue,
    ) -> Result<Vec<String>> {
        let mut standalone = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let mut paths = Vec::new();
        for (index, source) in catalogue.standalone.iter().enumerate() {
            let id = u32::try_from(index)?;
            let name = resource_name(id, source.as_deref());
            let owner = format!("resources/standalone/{name}");
            let content = match original(discs, source.as_deref())? {
                Some(bytes) => {
                    self.resource_payload(&owner, &bytes, None, &mut Default::default(), 0)?
                }
                None => unavailable(source.as_deref()),
            };
            paths.push(self.resource_definition(&owner, content)?);
            standalone.insert(id, name);
        }
        for (index, group) in catalogue.groups.iter().enumerate() {
            // The original executable indexes this table with group - 1.
            let id = u32::try_from(index + 1)?;
            let name = resource_name(id, group.path.as_deref());
            let owner = format!("resources/groups/{name}");
            let content = match original(discs, group.path.as_deref())? {
                Some(bytes) => {
                    let entries = crate::all_assets::archive_entries(&bytes)
                        .with_context(|| format!("invalid dynamic resource group {id}"))?;
                    let mut mappings = BTreeMap::new();
                    for (member, range) in entries.iter().enumerate() {
                        let name = format!("member-{member:05}");
                        if range.is_none() && member != 0 {
                            // This alias is native directory behavior, not byte deduplication.
                            mappings.insert(u32::try_from(member)?, "member-00000".into());
                            continue;
                        }
                        let root = format!("{owner}/members/{name}");
                        let content = match range {
                            Some(range) => self
                                .resource_payload(
                                    &root,
                                    &bytes[range.clone()],
                                    None,
                                    &mut Default::default(),
                                    0,
                                )
                                .with_context(|| format!("resource group {id}, member {member}"))?,
                            None => Resource::Empty,
                        };
                        self.resource_definition(&root, content)?;
                        mappings.insert(u32::try_from(member)?, name);
                    }
                    let path = format!("{owner}/members/mappings.json");
                    self.json(&path, &Mappings::new(mappings)?)?;
                    Resource::Members { mappings: path }
                }
                None => unavailable(group.path.as_deref()),
            };
            paths.push(self.resource_definition(&owner, content)?);
            groups.insert(id, name);
        }
        self.json(resource::STANDALONE, &Mappings::new(standalone)?)?;
        self.json(resource::GROUPS, &Mappings::new(groups)?)?;
        self.json(
            resource::PATH,
            &resource::Catalogue {
                version: resource::VERSION,
                standalone: resource::STANDALONE.into(),
                groups: resource::GROUPS.into(),
            },
        )?;
        paths.extend([
            resource::PATH.into(),
            resource::STANDALONE.into(),
            resource::GROUPS.into(),
        ]);
        Ok(paths)
    }

    fn resource_definition(&mut self, owner: &str, content: Resource) -> Result<String> {
        let path = format!("{owner}/resource.json");
        self.json(
            &path,
            &Definition {
                version: resource::VERSION,
                content,
            },
        )?;
        Ok(path)
    }

    fn resource_payload(
        &mut self,
        owner: &str,
        bytes: &[u8],
        primary: Option<&[u8]>,
        decoded: &mut crate::scene::decoded::Package,
        depth: usize,
    ) -> Result<Resource> {
        ensure!(
            depth <= 16,
            "dynamic resource nesting exceeds 16 levels in {owner}"
        );
        let expanded = crate::compression::payload(bytes.to_vec())?;
        let bytes = expanded.as_slice();
        if crate::all_assets::geometry::is_model(bytes) {
            let model = decoded.decode_model(bytes, primary.unwrap_or(bytes), self.source)?;
            let prepared = model.prepare_scene(self.source)?;
            let scene = self.physical_scene(prepared, &format!("{owner}/geometry"))?;
            return Ok(Resource::Geometry { scene });
        }
        if crate::animation::is_animation(bytes) {
            let animation = crate::animation::read_member(bytes)?.editable()?;
            let path = format!("{owner}/animation.json");
            self.json(&path, &animation)?;
            return Ok(Resource::Animation { animation: path });
        }
        if crate::read::u32(bytes, 0).ok() == Some(0x0020af30) {
            let decoded = crate::texture::decode_source(bytes)?;
            let mut textures = decoded.write(self.source)?;
            for (index, texture) in textures.textures.iter_mut().enumerate() {
                let Some(texture) = texture else { continue };
                for (palette, image) in texture.images.iter_mut().enumerate() {
                    *image = self.image(
                        image,
                        &format!("{owner}/textures/image-{index:03}-palette-{palette:02}"),
                    )?;
                }
            }
            let path = format!("{owner}/textures/textures.json");
            self.json(&path, &textures)?;
            return Ok(Resource::Textures { textures: path });
        }
        if crate::read::u32(bytes, 0).ok() == Some(31) {
            let sections = crate::field::sections(bytes)?;
            let body = sections
                .first()
                .and_then(Option::as_ref)
                .map(|range| &bytes[range.clone()]);
            let mut mappings = BTreeMap::new();
            for (slot, range) in sections.iter().enumerate() {
                let name = match slot {
                    0 => "body".into(),
                    1 => "outline".into(),
                    29 => "floor-collision".into(),
                    30 => "solid-collision".into(),
                    _ => format!("motion-{slot:02}"),
                };
                let root = format!("{owner}/parts/{name}");
                let content = match range {
                    Some(range) => {
                        let part = &bytes[range.clone()];
                        if crate::character::collision_slot(slot, part) {
                            use crate::field::collision_data::{Format, Mesh};
                            let groups = Mesh::read(part, Format::Short)?.groups;
                            let path = format!("{root}/collision.json");
                            self.json(&path, &groups)?;
                            Resource::Collision { collision: path }
                        } else {
                            self.resource_payload(&root, part, body, decoded, depth + 1)?
                        }
                    }
                    None => Resource::Empty,
                };
                self.resource_definition(&root, content)?;
                mappings.insert(u32::try_from(slot)?, name);
            }
            let path = format!("{owner}/parts/mappings.json");
            self.json(&path, &Mappings::new(mappings)?)?;
            return Ok(Resource::Members { mappings: path });
        }
        if let Some(script) = crate::all_assets::decode_script(bytes)? {
            let program = format!("{owner}/scenario.ssb");
            let messages = format!("{owner}/messages.json");
            self.write(&program, script.bytes)?;
            self.json(&messages, &script.messages)?;
            return Ok(Resource::Script {
                script: program,
                messages,
            });
        }
        Ok(Resource::Unsupported {
            description: "This resource requires a specialized field binding.".into(),
        })
    }
}

fn original(discs: &[PathBuf], source: Option<&str>) -> Result<Option<Vec<u8>>> {
    let Some(source) = source else {
        return Ok(None);
    };
    crate::source_paths::shared_source(discs.iter().map(PathBuf::as_path), source)?
        .map(fs::read)
        .transpose()
        .map_err(Into::into)
}

fn unavailable(source: Option<&str>) -> Resource {
    match source {
        Some(_) => Resource::Unavailable,
        None => Resource::Empty,
    }
}

fn resource_name(id: u32, source: Option<&str>) -> String {
    let Some(source) = source else {
        return format!("empty-{id:04}");
    };
    let source = source.rsplit_once('.').map_or(source, |(stem, _)| stem);
    let mut name = String::new();
    for byte in source.bytes() {
        if byte.is_ascii_alphanumeric() {
            name.push(char::from(byte.to_ascii_lowercase()));
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_end_matches('-');
    format!(
        "{}-{id:04}",
        if name.is_empty() { "resource" } else { name }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_scopes_keep_absence_empty_slots_and_explicit_member_aliases() -> Result<()> {
        let extracted = tempfile::tempdir()?;
        fs::create_dir_all(extracted.path().join("files"))?;
        fs::write(extracted.path().join("files/Model.bin"), [0xff; 16])?;
        let mut archive: Vec<u8> = [3u32, 28, 16, 0, 0, 28, 16]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        archive.extend([0xff; 16]);
        fs::write(extracted.path().join("files/Models.bin"), archive)?;
        let catalogue = crate::resource::Catalogue {
            standalone: vec![
                Some("Model.bin".into()),
                None,
                Some("missing.bin".into()),
                Some("Model.bin".into()),
            ],
            groups: vec![
                crate::resource::Group {
                    path: Some("Models.bin".into()),
                    storage: [0; 3],
                },
                crate::resource::Group {
                    path: None,
                    storage: [0; 3],
                },
            ],
            party_bodies: vec![],
            party_battle_motions: vec![],
            party_field_motions: vec![],
            field_services: vec![],
            save_point: "unused.bin".into(),
            colette_wings: "unused.bin".into(),
            treasures: std::array::from_fn(|_| "unused.bin".into()),
        };
        let raw = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let mut publisher = Publisher::new(raw.path(), output.path())?;
        publisher.resource_catalogue(&[extracted.path().into()], &catalogue)?;
        publisher.descriptor("GQSEAF", 0)?;
        let mut snapshot = library::Library::open(output.path(), &[])?.snapshot();
        let standalone = snapshot.mappings(resource::STANDALONE)?.clone();
        assert_eq!(standalone.iter().count(), 4);
        assert_ne!(
            standalone.target(0)?,
            standalone.target(3)?,
            "equal source bytes do not couple distinct resource IDs"
        );
        let absent: Definition = snapshot.json(
            &standalone.resolve(resource::STANDALONE, 2, Some("resource.json"))?,
            1024 * 1024,
        )?;
        assert!(matches!(absent.content, Resource::Unavailable));
        let groups = snapshot.mappings(resource::GROUPS)?.clone();
        assert!(groups.target(0).is_err());
        assert_eq!(groups.iter().map(|(id, _)| id).collect::<Vec<_>>(), [1, 2]);
        let group: Definition = snapshot.json(
            &groups.resolve(resource::GROUPS, 1, Some("resource.json"))?,
            1024 * 1024,
        )?;
        let Resource::Members { mappings } = group.content else {
            anyhow::bail!("lost native group")
        };
        let members = snapshot.mappings(&mappings)?.clone();
        assert_eq!(
            members.target(1)?,
            members.target(0)?,
            "empty native member selects member zero"
        );
        assert_ne!(
            members.target(2)?,
            members.target(0)?,
            "explicit slots retain independent owners"
        );
        let limitations = resource::capture(&mut snapshot, resource::PATH, &|| false)?;
        assert_eq!(limitations.len(), 8);
        assert!(limitations[&0x20000].contains("empty"));
        assert!(limitations[&2].contains("no playable content"));
        assert!(resource::capture(&mut snapshot, resource::PATH, &|| true).is_err());
        let modification = tempfile::tempdir()?;
        let write = |path: &str, value: &serde_json::Value| -> Result<()> {
            let target = modification.path().join(path);
            fs::create_dir_all(target.parent().unwrap())?;
            Ok(fs::write(target, serde_json::to_vec(value)?)?)
        };
        write(resource::STANDALONE, &serde_json::json!({"0":"edited"}))?;
        write(
            "resources/standalone/edited/resource.json",
            &serde_json::json!({
                "version":1, "content":{"kind":"members", "mappings":"resources/standalone/edited/parts/mappings.json"}
            }),
        )?;
        write(
            "resources/standalone/edited/parts/mappings.json",
            &serde_json::json!({"0":"missing"}),
        )?;
        let modified = library::Library::open(
            output.path(),
            &[library::Mod {
                name: "resource edit".into(),
                root: modification.path().into(),
            }],
        )?;
        let error =
            resource::capture(&mut modified.snapshot(), resource::PATH, &|| false).unwrap_err();
        assert!(format!("{error:#}").contains("missing/resource.json"));
        write(
            "resources/standalone/edited/parts/missing/resource.json",
            &serde_json::json!({
                "version":1, "content":{"kind":"unsupported", "description":"edited resource"}
            }),
        )?;
        let mut edited = modified.snapshot();
        let edited_limits = resource::capture(&mut edited, resource::PATH, &|| false)?;
        assert_eq!(
            edited_limits.len(),
            limitations.len(),
            "partial mapping overrides retain inherited IDs"
        );
        fs::remove_file(
            modification
                .path()
                .join("resources/standalone/edited/parts/missing/resource.json"),
        )?;
        assert!(
            resource::capture(&mut edited, resource::PATH, &|| false).is_ok(),
            "captured dependencies remain available"
        );
        assert!(resource::capture(&mut modified.snapshot(), resource::PATH, &|| false).is_err());
        assert_eq!(
            resource::capture(&mut snapshot, resource::PATH, &|| false)?,
            limitations
        );
        write(
            "resources/standalone/edited/resource.json",
            &serde_json::json!({
                "version":1, "content":{"kind":"members", "mappings":"resources/standalone/mappings.json"}
            }),
        )?;
        let error =
            resource::capture(&mut modified.snapshot(), resource::PATH, &|| false).unwrap_err();
        assert!(format!("{error:#}").contains("cyclic resource"));
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; publishes the complete native dynamic catalogue"]
    fn original_dynamic_catalogue_preserves_all_native_scopes_and_current_dependencies()
    -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let discs = [local.join("disc1"), local.join("disc2")];
        let catalogue = crate::resource::read(&fs::read(discs[0].join("sys/main.dol"))?)?;
        let raw = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let mut publisher = Publisher::new(raw.path(), output.path())?;
        publisher.resource_catalogue(&discs, &catalogue)?;
        publisher.descriptor("GQSEAF", 0)?;
        let mut snapshot = library::Library::open(output.path(), &[])?.snapshot();
        let standalone = snapshot.mappings(resource::STANDALONE)?.clone();
        let groups = snapshot.mappings(resource::GROUPS)?.clone();
        assert_eq!(standalone.iter().count(), 53);
        assert_eq!(groups.iter().count(), 14);
        let mut absent = 0;
        for (id, _) in standalone.iter() {
            let path = standalone.resolve(resource::STANDALONE, id, Some("resource.json"))?;
            let definition: Definition = snapshot.json(&path, 1024 * 1024)?;
            absent += usize::from(matches!(definition.content, Resource::Unavailable));
        }
        assert_eq!(absent, 19);
        let mut expected = 53;
        for (index, group) in catalogue.groups.iter().enumerate() {
            let id = u32::try_from(index + 1)?;
            let path = groups.resolve(resource::GROUPS, id, Some("resource.json"))?;
            let definition: Definition = snapshot.json(&path, 1024 * 1024)?;
            match original(&discs, group.path.as_deref())? {
                Some(bytes) => {
                    let original = crate::all_assets::archive_entries(&bytes)
                        .context("invalid native group")?;
                    let Resource::Members { mappings } = definition.content else {
                        anyhow::bail!("lost group {id}")
                    };
                    let members = snapshot.mappings(&mappings)?.clone();
                    assert_eq!(
                        members
                            .iter()
                            .map(|(id, _)| id as usize)
                            .collect::<Vec<_>>(),
                        (0..original.len()).collect::<Vec<_>>()
                    );
                    for (member, range) in original.iter().enumerate().skip(1) {
                        if range.is_none() {
                            assert_eq!(members.target(member as u32)?, members.target(0)?);
                        }
                    }
                    expected += original.len();
                }
                None => {
                    assert!(matches!(definition.content, Resource::Empty));
                    expected += 1;
                }
            }
        }
        let captured = resource::capture(&mut snapshot, resource::PATH, &|| false)?;
        assert_eq!(captured.len(), expected);
        Ok(())
    }
}
