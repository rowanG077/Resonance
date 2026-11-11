//! Explicit ownership policies. No implicit recursive merging of arbitrary JSON.
use super::*;

fn object(fields: Vec<(&'static str, Node)>) -> Node {
    Node::Object(fields)
}

fn map(node: Node) -> Node {
    Node::Map(Box::new(node))
}
fn indexed(node: Node) -> Node {
    Node::Indexed(Box::new(node))
}
fn values() -> Node {
    map(Node::File)
}
fn ordered(identity: Identity, entry: Node) -> Node {
    Node::Ordered {
        identity,
        entry: Box::new(entry),
    }
}
fn contributions(node: Node) -> Node {
    ordered(Identity::Authored, node)
}
fn registrations() -> Node {
    ordered(Identity::Value, Node::File)
}
fn awards() -> Node {
    ordered(Identity::Fields(&["character", "title"]), Node::File)
}
fn record() -> Node {
    object(vec![("text", Node::File)])
}

pub(super) fn for_document(path: &str) -> Option<Layout> {
    use Node::File;
    let node = match path {
        "game/items.json" => object(vec![
            ("mappings", values()),
            ("items", map(record())),
            ("equipment_descriptions", values()),
        ]),
        "game/techniques.json" => object(vec![
            ("mappings", values()),
            ("characters", File),
            (
                "techniques",
                map(object(vec![
                    ("rules", File),
                    ("menu", File),
                    ("text", File),
                ])),
            ),
            ("learning", map(registrations())),
        ]),
        "game/characters.json" => object(vec![
            ("mappings", values()),
            (
                "characters",
                map(object(vec![
                    ("name", File),
                    ("full_name", File),
                    (
                        "initial",
                        object(vec![
                            ("techniques", registrations()),
                            ("stats", values()),
                            ("equipment", values()),
                            ("shortcuts", values()),
                            ("cooking", values()),
                            ("ex_gems", values()),
                            ("level", File),
                            ("luck", File),
                            ("experience", File),
                            ("affinity", File),
                            ("overlimit", File),
                            ("technique_balance", File),
                        ]),
                    ),
                    ("growth", File),
                ])),
            ),
            ("experience", File),
            ("growth_profiles", map(values())),
        ]),
        "game/titles.json" => object(vec![
            ("mappings", map(values())),
            (
                "titles",
                map(object(vec![
                    ("text", File),
                    ("growth", values()),
                    ("costume", File),
                ])),
            ),
            (
                "awards",
                object(vec![
                    ("levels", awards()),
                    ("combos", awards()),
                    ("equipment", awards()),
                ]),
            ),
        ]),
        "game/cooking.json" => object(vec![
            ("mappings", values()),
            (
                "recipes",
                map(object(vec![("text", File), ("cooks", values())])),
            ),
            ("groups", map(object(vec![("name", File)]))),
            ("preferences", values()),
            ("labels", values()),
            ("effect_names", values()),
        ]),
        "game/ex-skills.json" => object(vec![
            ("mappings", values()),
            (
                "skills",
                map(object(vec![
                    ("text", File),
                    ("combinations", contributions(File)),
                ])),
            ),
            (
                "characters",
                map(object(vec![("levels", values()), ("compounds", values())])),
            ),
            ("labels", values()),
            ("activation_labels", values()),
            ("gem_items", values()),
        ]),
        "game/strategy.json" => object(vec![
            (
                "groups",
                map(object(vec![
                    ("mappings", values()),
                    ("name", File),
                    ("options", map(record())),
                ])),
            ),
            ("preset_mappings", values()),
            ("presets", values()),
            ("default_positions", values()),
            ("labels", values()),
            ("keyboard", File),
        ]),
        "game/options.json" => object(vec![
            (
                "options",
                map(object(vec![("name", File), ("description", File)])),
            ),
            ("difficulties", values()),
            ("actions", values()),
            ("color_groups", values()),
            ("volume_channels", values()),
            ("themes", values()),
            ("defaults", object(vec![("controls", values())])),
            ("labels", values()),
            ("confirmations", values()),
        ]),
        "game/menu/world-map.json" => object(vec![
            ("mappings", values()),
            (
                "shops",
                map(object(vec![("name", File), ("items", registrations())])),
            ),
            ("field_locations", values()),
            ("names", values()),
            ("shop_visit_groups", indexed(File)),
        ]),
        "game/menu/crafting.json" => object(vec![
            ("mappings", values()),
            (
                "vendors",
                // Vendor slots can deliberately repeat the same recipe.
                map(object(vec![("name", File), ("recipes", File)])),
            ),
            ("recipes", values()),
            ("labels", values()),
        ]),
        "game/menu/synopsis.json" => object(vec![
            ("mappings", values()),
            ("entries", values()),
            ("months", values()),
        ]),
        "game/menu/presentation.json" => object(vec![
            ("labels", values()),
            ("items", values()),
            ("status", values()),
        ]),
        "battle/party/rules.json" => object(vec![
            (
                "characters",
                map(object(vec![
                    ("profile", File),
                    ("body_collider", File),
                    ("default_strategy", File),
                    (
                        "casting_voices",
                        object(vec![
                            ("fallback", File),
                            ("default", File),
                            ("techniques", values()),
                        ]),
                    ),
                    ("chant_sequence", File),
                ])),
            ),
            ("technique_strategies", values()),
            ("target_strategies", values()),
        ]),
        "battle/party/normal-attacks.json" => object(vec![
            ("mappings", values()),
            (
                "characters",
                map(object(vec![("attacks", values()), ("hits", values())])),
            ),
        ]),
        "battle/party/techniques.json" => object(vec![
            ("mappings", values()),
            ("bindings", map(values())),
            (
                "techniques",
                map(object(vec![
                    ("mappings", values()),
                    ("variants", values()),
                    ("contacts", values()),
                    ("projectiles", values()),
                ])),
            ),
        ]),
        "battle/formations.json" => object(vec![
            ("mappings", values()),
            ("formations", values()),
            ("pools", ordered(Identity::First("ids"), File)),
        ]),
        "battle/unison.json" => object(vec![
            ("characters", values()),
            ("openers", values()),
            ("hits", values()),
            ("mappings", values()),
            ("placements", contributions(File)),
            ("combinations", ordered(Identity::Fields(&["name"]), File)),
        ]),
        "animations/bindings.json" => object(vec![
            ("rigs", values()),
            ("tracks", values()),
            ("joint_maps", values()),
            ("banks", values()),
        ]),
        "skits/catalogue.json" => object(vec![
            (
                "skits",
                ordered(Identity::Fields(&["id"]), object(vec![("title", File)])),
            ),
            (
                "resources",
                map(object(vec![("title", File), ("allow_skip", File)])),
            ),
            ("preview_order", registrations()),
            ("media", object(vec![("ids", registrations())])),
        ]),
        "skits/portraits.json" => object(vec![
            ("mappings", values()),
            ("images", values()),
            ("animations", values()),
        ]),
        "ui/menu.json" => object(vec![
            ("fill", File),
            ("popup_fill", File),
            ("shade", File),
            ("patterns", indexed(File)),
            ("images", values()),
            ("labels", values()),
            ("palette", values()),
            (
                "windows",
                map(object(vec![
                    ("fill", File),
                    ("patterns", indexed(File)),
                    ("frame", values()),
                    ("heading", File),
                    ("cursor", File),
                    ("cursor_motion", File),
                ])),
            ),
            (
                "sprites",
                object(vec![
                    ("rects", map(values())),
                    ("number_colors", values()),
                    ("bar_colors", map(values())),
                    ("names", values()),
                ]),
            ),
        ]),
        "fields/settings.json" => object(vec![
            ("effects", File),
            ("blink", File),
            ("contact_shadow", File),
            ("particles", values()),
            ("save_point", values()),
            ("environment", values()),
        ]),
        "fields/foraging.json" => object(vec![("pools", values()), ("areas", contributions(File))]),
        "audio/synthesis.json" => object(vec![
            ("mix", values()),
            ("dls", values()),
            ("modulation", values()),
            ("coefficients", File),
        ]),
        "audio/mix.json" => object(vec![
            ("reverbs", values()),
            ("field_reverbs", File),
            ("battle_reverbs", File),
            ("music_reverbs", map(registrations())),
            ("voice_gains", File),
        ]),
        "game/menu/grade-shop.json" => object(vec![
            (
                "options",
                ordered(
                    Identity::Fields(&["benefit"]),
                    object(vec![("name", File), ("description", File)]),
                ),
            ),
            ("labels", values()),
        ]),
        "game/menu/monster-book.json" | "game/menu/figurines.json" => object(vec![
            ("title", File),
            ("labels", values()),
            (
                "records",
                ordered(
                    Identity::Fields(&["id"]),
                    object(vec![
                        ("name", File),
                        ("description", File),
                        ("location", File),
                        ("category", File),
                        ("preview", File),
                    ]),
                ),
            ),
        ]),
        "game/menu/manual.json" => object(vec![
            ("title", File),
            (
                "chapters",
                indexed(object(vec![
                    ("name", File),
                    ("topics", contributions(File)),
                ])),
            ),
        ]),
        "game/menu/rename.json" => object(vec![
            ("defaults", values()),
            ("keyboard", File),
            ("commands", values()),
        ]),
        "textures/banks.json" => values(),
        "meshes/joint-mappings.json" => values(),
        "meshes/groups.json" => map(ordered(Identity::Model, File)),
        "ui/battle/battle.json" => object(vec![
            ("portraits", values()),
            (
                "meters",
                object(vec![("bars", values()), ("numbers", values())]),
            ),
            (
                "damage",
                object(vec![("colors", values()), ("sizes", values())]),
            ),
            ("recovery_colors", values()),
            ("result_colors", values()),
            ("targets", values()),
            ("stat_backdrops", values()),
            ("stat_icons", values()),
            ("unison_buttons", values()),
            (
                "notices",
                object(vec![
                    ("frame", File),
                    ("fill", File),
                    ("icons", values()),
                    ("icon_origins", values()),
                    ("faces", values()),
                    ("defeated", File),
                ]),
            ),
            (
                "combo",
                object(vec![
                    ("anchors", values()),
                    ("number_colors", File),
                    ("bar_colors", values()),
                ]),
            ),
            (
                "commands",
                object(vec![
                    ("texture", File),
                    ("icons", values()),
                    ("faces", values()),
                    ("names", values()),
                    ("cancel_escape", File),
                    ("player_format", File),
                    ("selector_labels", values()),
                    ("selector_colors", values()),
                    ("motion", values()),
                ]),
            ),
            (
                "feedback",
                object(vec![
                    ("scan_icons", values()),
                    ("escape", values()),
                    ("item_label", File),
                    ("unison_ready", File),
                ]),
            ),
        ]),
        "ui/dialogue.json" => object(vec![
            ("font", File),
            ("symbols", File),
            ("shading", File),
            ("patterns", indexed(File)),
            ("lined_fill", File),
            ("plain_fill", File),
            ("cursor", File),
            ("default_style", File),
            ("selection", values()),
        ]),
        "ui/title/title.json" => object(vec![
            ("day_field", File),
            ("night_field", File),
            ("music", File),
            ("cues", File),
            ("artwork", contributions(File)),
        ]),
        "battle/projectiles.json" => object(vec![
            ("records", indexed(File)),
            ("resident_spells", values()),
            ("support_durations", values()),
            ("fire_ball", File),
        ]),
        "battle/audio.json" => object(vec![("speech_rates", values()), ("reverbs", File)]),
        "ui/credits/credits.json" => object(vec![
            ("style", values()),
            ("scroll", values()),
            ("blocks", contributions(File)),
            ("music", File),
            ("final_hold_ticks", File),
        ]),
        "ui/boot/sequence.json" => contributions(File),
        "ui/story-subtitles.json" => ordered(Identity::Fields(&["frame"]), File),
        "audio/menu.json" => object(vec![("cues", values()), ("reverbs", File)]),
        "world/landmarks.json" => {
            let landmarks = ordered(
                Identity::Fields(&["id"]),
                object(vec![
                    ("name", File),
                    ("shops", registrations()),
                    ("entry_overrides", contributions(File)),
                    ("shop_variants", contributions(File)),
                ]),
            );
            object(vec![
                ("sylvarant", landmarks.clone()),
                ("tethe_alla", landmarks),
            ])
        }
        "world/movement.json" => object(vec![
            (
                "flight_zones",
                object(vec![
                    ("sylvarant", contributions(File)),
                    ("tethe_alla", contributions(File)),
                ]),
            ),
            ("camera_distances", values()),
            ("collision", values()),
        ]),
        "world/encounters.json" => {
            let world = object(vec![
                ("arenas", values()),
                ("regions", values()),
                ("surface_terrain", values()),
                ("grid", File),
            ]);
            object(vec![
                ("surface_terrain", values()),
                ("docks", contributions(File)),
                ("sylvarant", world.clone()),
                ("tethe_alla", world),
            ])
        }
        "world/world.json" => object(vec![
            ("script", File),
            ("messages", File),
            ("audio", File),
            ("movement", File),
            ("encounters", File),
            ("landmarks", File),
            ("continents", File),
            ("actors", File),
            ("markers", File),
            ("cinematics", File),
            ("dust", indexed(File)),
            ("skies", indexed(File)),
        ]),
        _ if path.starts_with("battle/") && path.ends_with("/effects.json") => object(vec![
            ("programs", indexed(File)),
            ("templates", values()),
            (
                "artwork",
                object(vec![("textures", values()), ("models", values())]),
            ),
        ]),
        _ if path.starts_with("world/") && path.ends_with("/world.json") => object(vec![(
            "terrain",
            ordered(Identity::Fields(&["column", "row"]), File),
        )]),
        _ if path.starts_with("world/")
            && (path.ends_with("/terrain.json") || path.ends_with("/alternate.json")) =>
        {
            object(vec![("collision", File), ("parts", indexed(File))])
        }
        _ if ["fields/", "world/", "skits/", "battle/events/"]
            .iter()
            .any(|root| path.starts_with(root))
            && path.ends_with("/messages.json") =>
        {
            indexed(File)
        }
        _ if field_document(path) == Some("field.json") => object(vec![
            ("script", File),
            ("messages", File),
            ("scene", File),
            ("collision", File),
            ("actors", File),
            ("audio", File),
            ("movies", registrations()),
        ]),
        _ if field_document(path) == Some("actors.json") => object(vec![
            ("models", ordered(Identity::Model, File)),
            ("animations", registrations()),
            ("local_animations", values()),
            ("local_resources", registrations()),
        ]),
        _ if field_document(path) == Some("scene.json") => object(vec![
            ("parts", indexed(File)),
            ("camera_tracks", values()),
            ("overlays", values()),
            ("particles", values()),
            ("environment", values()),
            ("water", File),
            ("panorama", File),
            ("contact_shadow", File),
            ("save_point", values()),
        ]),
        _ if path.starts_with("battle/enemies/") && path.ends_with("/enemy.json") => {
            object(vec![("rules", File), ("artwork", File), ("effects", File)])
        }
        // ID namespaces: each ID is one owner, so mods add or rebind single IDs.
        _ if path == "mappings.json" || path.ends_with("/mappings.json") => values(),
        _ => return None,
    };
    Some(Layout {
        document: path.into(),
        directory: path.strip_suffix(".json")?.into(),
        node,
    })
}

/// A field's own document, not a like-named file of one of its models.
fn field_document(path: &str) -> Option<&str> {
    let (_, document) = path.strip_prefix("fields/")?.split_once('/')?;
    (!document.contains('/')).then_some(document)
}

pub(super) fn discover(paths: &BTreeSet<String>) -> Vec<Layout> {
    let mut documents = BTreeSet::new();
    for path in paths {
        // The outermost owner wins: a source owner's private tree is not
        // another independent document. In particular, a template named
        // "effects" must not become a bank.
        let mut owner = None;
        let mut parent = path.rsplit_once('/').map(|(parent, _)| parent);
        while let Some(root) = parent {
            let document = format!("{root}.json");
            if for_document(&document).is_some_and(|layout| layout.node.owns(root, path)) {
                owner = Some(document);
            }
            parent = root.rsplit_once('/').map(|(parent, _)| parent);
        }
        documents.extend(owner);
    }
    documents
        .into_iter()
        .filter_map(|path| for_document(&path))
        .collect()
}
