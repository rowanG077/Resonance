"""Current named documents and oracle-only pins; no legacy cooked reader.

The evidence directory is outside the editable library. It owns copies of the
original bytes used by the observer; cooked files remain pinned at their actual
paths and are admitted by the Rust production readers when evidence is consumed.
"""
import copy
import hashlib
import json
from pathlib import Path, PurePosixPath
import struct
import effect_defaults
import effect_sources


def logical(path):
    if (not isinstance(path, str) or not path or "\\" in path or ":" in path
            or "\0" in path or path.startswith("/")
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise ValueError(f"invalid current content path: {path!r}")
    return path


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def same_evidence(first, second):
    """Compare identities while allowing independent copies of original evidence."""
    return (first["version"] == second["version"] == 1
            and first["entry"] == second["entry"]
            and first["source"]["sha256"] == second["source"]["sha256"]
            and first.get("effect_templates") == second.get("effect_templates")
            and {path: pin["sha256"] for path, pin in first["files"].items()}
                == {path: pin["sha256"] for path, pin in second["files"].items()})


class Snapshot:
    def __init__(self, root, evidence_directory=None):
        self.root = Path(root).resolve(strict=True)
        self.evidence_directory = None if evidence_directory is None else Path(evidence_directory).resolve()
        if self.evidence_directory is not None and self.evidence_directory.is_relative_to(self.root):
            raise ValueError("oracle evidence must be outside the editable library")
        self.files = {}
        descriptor = self.json("content.json")
        if descriptor != {"version":92, "game": "tales-of-symphonia"}:
            raise ValueError("oracle requires a current named Tales of Symphonia library")

    def read(self, path):
        logical(path)
        if path not in self.files:
            physical = (self.root / path).resolve(strict=True)
            if not physical.is_relative_to(self.root) or not physical.is_file():
                raise ValueError(f"content resource escapes its library: {path}")
            with physical.open("rb") as stream:
                data = stream.read(256 * 1024 * 1024 + 1)
            if len(data) > 256 * 1024 * 1024:
                raise ValueError(f"oracle content exceeds read limit: {path}")
            self.files[path] = (data, dict(path=str(physical), sha256=hashlib.sha256(data).hexdigest()))
        return self.files[path][0]

    def json(self, path):
        return json.loads(self.read(path), object_pairs_hook=object_pairs)

    def document(self, path, fields):
        document = self.json(path)
        if document.get("version") != 1 or set(document) - (set(fields) | {"version"}):
            raise ValueError(f"unsupported current document: {path}")
        return document

    def mapping(self, path):
        mapping = self.json(path)
        if not isinstance(mapping, dict):
            raise ValueError(f"invalid ID mapping: {path}")
        for key, value in mapping.items():
            if not key.isascii() or not key.isdigit() or str(int(key)) != key or int(key) > 0xffffffff:
                raise ValueError(f"invalid machine ID in {path}: {key}")
            logical(value)
        return mapping

    def resolve(self, mapping, identity, entry=None):
        target = self.mapping(mapping).get(str(identity))
        if target is None:
            return None
        path = str(PurePosixPath(mapping).parent / target)
        return logical(f"{path}/{entry}" if entry else path)

    def unison_placements(self):
        source = self.json("battle/unison.json")
        technique_ids = {name:int(identity) for identity,name in self.json("game/techniques.json")["mappings"].items()}
        groups = {name:int(identity) for identity,name in source["mappings"].items()}
        result = {}
        for row in source["placements"]:
            for entry in row["techniques"]:
                name = entry if isinstance(entry,str) else entry["technique"]
                matches = entry if isinstance(entry,str) else entry["matches"]
                identity = technique_ids[name]
                if identity in result or identity == 0:
                    raise ValueError("duplicate or unavailable Unison technique")
                result[identity] = dict(native_id=groups[matches], altitude=row.get("altitude",0),
                    distance=row["distance"], duration=row["duration"])
        return result

    def formations(self):
        source = self.json("battle/formations.json")
        names = {name: int(identity) for identity, name in self.mapping("battle/enemies/mappings.json").items()}
        if set(source) != {"mappings", "formations", "pools"}:
            raise ValueError("unsupported formation document")
        definitions = {}
        for name, row in source["formations"].items():
            resources, actors = [], []
            def resource(entry):
                value = dict(enemy=names[entry["enemy"]], hidden_name=entry.get("hidden_name", False))
                if value not in resources:
                    resources.append(value)
                return resources.index(value)
            for entry in row["actors"]:
                actor = dict(resource=resource(entry), variant=entry.get("statistics_variant", 0),
                    texture_variant=entry.get("texture_variant", 0), weapon_variant=entry.get("weapon_variant", 0),
                    position=entry.get("position"), unsupported_reason=entry.get("unsupported_reason"))
                count = entry.get("count", 1)
                if not 1 <= count <= 255:
                    raise ValueError("invalid formation actor count")
                actors.extend(copy.deepcopy(actor) for _ in range(count))
            for entry in row.get("reserves", []):
                resource(entry)
            definitions[name] = dict(resources=resources, actors=actors, settings={
                key: row.get(key, key != "escape_restricted")
                for key in ("play_music", "celebrate", "entry_voice", "escape_restricted")})
        records = [definitions[source["mappings"][str(i)]] for i in range(len(source["mappings"]))]
        pool_ids = [identity for pool in source["pools"] for identity in pool["ids"]]
        pools = [None] * (max(pool_ids) + 1 if pool_ids else 0)
        for pool in source["pools"]:
            for identity in pool["ids"]:
                if pools[identity] is not None:
                    raise ValueError("duplicate encounter pool")
                pools[identity] = pool["choices"]
        if any(pool is None for pool in pools):
            raise ValueError("missing encounter pool")
        return dict(records=records, pools=pools)

    def scene_path(self, identity, *, skill=False):
        return self.resolve("battle/skills/mappings.json" if skill else "battle/scenes/mappings.json",
                            identity, "scene.json")

    def arena_path(self, identity):
        return self.resolve("battle/arenas/mappings.json", identity, "arena.json")

    def arena(self, path):
        document = self.json(path)
        fields = ("unsupported_reason", "light_position", "chain_acceleration",
            "actor_color", "translation", "yaw_degrees", "color", "camera_pitch_offset", "layers",
            "fade_step", "fog", "effects", "recurring_interval")
        if set(document) - set(fields) or not isinstance(document.get("layers"), list):
            raise ValueError(f"unsupported current arena: {path}")
        return dict(unsupported_reason=None, effects=None) | document

    def world_encounters(self):
        world = self.json("world/world.json")
        source = self.json(world["encounters"])
        terrains = ("grassland", "road", "wasteland", "snowfield", "desert",
                    "forest", "deep_forest", "beach", "bridge", "mountains")
        def pair(value):
            return [value, value] if isinstance(value, int) else value
        worlds = []
        for name, later in (("sylvarant", False), ("tethe_alla", False), ("tethe_alla", True)):
            continent = source[name]
            regions = continent["regions"]
            areas = [None] * (1 + max(r["id"] for r in regions.values()))
            for region in regions.values():
                override = later and "later" in region
                groups = region["later"] if override else region["groups"]
                if isinstance(groups, str):
                    target = regions[groups]
                    groups = target.get("later", target["groups"]) if override else target["groups"]
                if not isinstance(groups, dict):
                    raise ValueError("encounter regions must refer directly to inline groups")
                areas[region["id"]] = dict(encounter_groups=[
                    pair(groups.get(terrain, source["default_group"])) for terrain in terrains])
            worlds.append(dict(areas=areas, arena_by_terrain=[continent["arenas"][t] for t in terrains]))
        return world["encounters"], dict(worlds=worlds)

    def image(self, path):
        if not path.endswith(".ktx2") or not self.read(path).startswith(b"\xabKTX 20\xbb\r\n\x1a\n"):
            raise ValueError(f"texture must be KTX2: {path}")

    def part(self, part):
        self.read(part["mesh"])
        for image in part.get("textures", []):
            self.image(image)
        for clip in part.get("clips", []):
            data = self.read(clip["motion"])
            if len(data) < 20 or data[:4] != b"glTF" or struct.unpack_from("<I", data, 4)[0] != 2:
                raise ValueError("animation must be a glTF 2.0 GLB")
            length, kind = struct.unpack_from("<II", data, 12)
            if kind != 0x4e4f534a:
                raise ValueError("animation GLB lacks its JSON chunk")
            document = json.loads(data[20:20+length], object_pairs_hook=object_pairs)
            animations = document.get("animations", [])
            if len(animations) != 1:
                raise ValueError("animation GLB must have exactly one clip")
            # glTF requires input accessor bounds, in seconds. Production readers
            # consume the actual samples when the oracle's evidence is admitted.
            clip["duration_seconds"] = max(document["accessors"][sampler["input"]]["max"][0]
                                           for sampler in animations[0]["samplers"])

    def rig(self, mesh, targets):
        """Prepare the joint interface used by observers from the current GLB."""
        data = self.read(mesh)
        if len(data) < 20 or data[:4] != b"glTF" or struct.unpack_from("<I", data, 4)[0] != 2:
            raise ValueError(f"model must be a glTF 2.0 GLB: {mesh}")
        total, length, kind = struct.unpack_from("<III", data, 8)
        if total != len(data) or kind != 0x4e4f534a or 20 + length > total:
            raise ValueError(f"invalid model GLB chunks: {mesh}")
        nodes = json.loads(data[20:20+length], object_pairs_hook=object_pairs)["nodes"]
        indexed = {}
        for node, value in enumerate(nodes):
            bone = value.get("extras", {}).get("resonance_bone")
            if bone is not None:
                if not isinstance(bone, int) or bone < 0 or bone in indexed:
                    raise ValueError(f"invalid gameplay node index: {mesh}")
                indexed[bone] = node
        if sorted(indexed) != list(range(len(indexed))):
            raise ValueError(f"noncontiguous gameplay node indices: {mesh}")
        indices = {node: bone for bone, node in indexed.items()}
        parents = {}
        for parent, node in enumerate(nodes):
            for child in node.get("children", []):
                if child in parents or not 0 <= child < len(nodes):
                    raise ValueError(f"invalid model hierarchy: {mesh}")
                parents[child] = parent
        def indexed_parent(node):
            seen = {node}
            while node in parents:
                node = parents[node]
                if node in seen:
                    raise ValueError(f"cyclic model hierarchy: {mesh}")
                seen.add(node)
                if node in indices:
                    return indices[node]
            return None
        bones = [dict(name=nodes[node]["name"], parent=indexed_parent(node))
                 for _, node in sorted(indexed.items())]
        attachments = {}
        for slot, target in targets.items():
            matching = [index for index, bone in enumerate(bones) if bone["name"] == target]
            if len(matching) != 1:
                raise ValueError(f"absent or ambiguous attachment joint: {target}")
            attachments[slot] = matching[0]
        return dict(skeleton=dict(bones=bones), attachments=attachments)

    def body_mesh(self, definition):
        part = self.json(definition["model"])["parts"][0]
        return part["mesh"]

    def model(self, path):
        return self.model_definition(self.json(path))

    def model_definition(self, model):
        for layer in model["layers"]:
            self.part(layer)
        return dict(layers=model["layers"],
            rig=self.rig(model["layers"][0]["mesh"], model.get("attachment_nodes", {})),
            layer_skeletons={str(layer.get("resource", 0)): self.rig(layer["mesh"], {})["skeleton"]
                for layer in model["layers"] if layer.get("resource", 0) in model.get("independent_layers", [])})

    def enemy_entry(self, identity):
        path = self.resolve("battle/enemies/mappings.json", identity, "enemy.json")
        if path is None:
            raise ValueError(f"unmapped enemy: {identity}")
        entry = self.json(path)
        if not isinstance(entry, dict) or set(entry) - {"rules", "artwork", "effects"} or not isinstance(entry.get("rules"), dict):
            raise ValueError(f"unsupported enemy document: {path}")
        return path, entry

    def enemy_rules(self, identity):
        # Observers consume the same expanded action form as combat. Local named
        # definitions remain authoritative in the one captured enemy document.
        _, entry = self.enemy_entry(identity)
        rules = entry["rules"]
        def event(value):
            if not isinstance(value, dict):
                return value
            if isinstance(value.get("projectile"), str):
                shot = rules["projectiles"][value["projectile"]]
                result = copy.deepcopy(shot)
                if result.get("feedback") is None:
                    result.pop("feedback", None)
                return {"projectile":result}
            for key in ("contact", "continue_contact"):
                if isinstance(value.get(key), str):
                    return {key:copy.deepcopy(rules["contacts"][value[key]])}
            if "ground_projectiles" in value and "projectiles" in value["ground_projectiles"]:
                spec = value["ground_projectiles"]
                first, second = (rules["projectiles"][name] for name in spec["projectiles"])
                if first["hit"] != second["hit"] or first.get("feedback") or second.get("feedback"):
                    raise ValueError("paired ground projectiles need matching hits and no martial feedback")
                return {"ground_projectiles":dict(definitions=[copy.deepcopy(first["definition"]),
                    copy.deepcopy(second["definition"])], hit=copy.deepcopy(first["hit"]), retreat=spec["retreat"])}
            return value
        def attack(name):
            result = copy.deepcopy(rules["attacks"][name])
            if "events" in result:
                result["events"] = [[at, event(value)] for at, value in result["events"]]
            return result
        for row in rules["actions"]["rows"]:
            execution = row.get("execution")
            if not isinstance(execution, dict):
                continue
            if "attack" in execution:
                execution["attack"] = attack(execution["attack"])
            elif "physical" in execution:
                execution["physical"]["attack"] = attack(execution["physical"]["attack"])
        return rules

    def enemy_artwork(self, identity):
        _, entry = self.enemy_entry(identity)
        return entry.get("artwork")

    def enemy(self, identity):
        path, _ = self.enemy_entry(identity)
        rules = self.enemy_rules(identity)
        artwork = self.enemy_artwork(identity)
        self.rig(self.body_mesh(artwork), artwork.get("attachment_nodes", {}))
        preview = self.json(artwork["model"])
        for part in preview["parts"]:
            self.part(part)
        return path, rules, preview

    def effects(self, path):
        definition = self.json(path)
        if not isinstance(definition, dict) or set(definition) - {"programs", "templates", "artwork"}:
            raise ValueError(f"invalid effect bank: {path}")
        templates = definition.get("templates", {})
        if not isinstance(templates, dict):
            raise ValueError(f"effect templates must be named: {path}")
        names = sorted(templates)
        programs = effect_defaults.programs(definition.get("programs", []), names)
        actors = effect_defaults.templates([templates[name] for name in names])
        art = None
        if definition.get("artwork") is not None:
            artwork = definition["artwork"]
            art = dict(textures={str({"actor":2,"scene":6,"arena":9,"actor-secondary":14}.get(slot, slot.removeprefix("bank-"))): textures for slot,textures in artwork.get("textures", {}).items()}, models={
                slot: self.model_definition(model) for slot, model in artwork.get("models", {}).items()})
            for textures in art["textures"].values():
                for texture in textures:
                    for image in texture["images"]:
                        self.image(image)
        return dict(programs=programs, actors=actors, art=art)

    def scene(self, path):
        definition = self.json(path)
        allowed = {"effects", "cutin", "weapon", "projectiles", "actions", "actions_error",
            "duration", "stage_color", "camera", "strike_points", "recovery_radius", "spell_recipe"}
        if not isinstance(definition, dict) or set(definition) - allowed or not {"duration", "effects"} <= definition.keys():
            raise ValueError(f"invalid battle scene: {path}")
        rules = dict(version=1, projectiles=[], actions={}, actions_error=None, stage_color=None,
            camera=None, strike_points=[], recovery_radius=None, spell_recipe=None)
        rules.update({key:value for key,value in definition.items() if key not in {"effects", "cutin", "weapon"}})
        effects = self.effects(definition["effects"])
        art = effects["art"] or dict(models={}, textures={})
        weapon = definition.get("weapon")
        if weapon:
            weapon = dict(weapon, parts={slot: self.model_definition(model) for slot, model in weapon["parts"].items()})
        cutin = definition.get("cutin")
        if cutin:
            for image in cutin["images"]:
                self.image(image)
        return dict(rules, effects=definition["effects"], models=art["models"],
                    textures=art["textures"].get("6", []), weapon=weapon, cutin=cutin), effects

    def evidence(self, entry, original):
        """Seal the exact current inputs already read, plus independent original bytes."""
        if self.evidence_directory is None:
            raise ValueError("an external directory is required to write oracle evidence")
        logical(entry)
        if entry not in self.files:
            raise ValueError(f"evidence entry was not captured: {entry}")
        signature = hashlib.sha256(original).hexdigest()
        source = self.evidence_directory / "original" / f"{signature}.bin"
        source.parent.mkdir(parents=True, exist_ok=True)
        try:
            with source.open("xb") as stream:
                stream.write(original)
        except FileExistsError:
            if source.read_bytes() != original:
                raise ValueError(f"original evidence collision: {source}")
        result = dict(version=1, entry=entry, files={path: pin for path, (_, pin) in sorted(self.files.items())},
                    source=dict(path=str(source), sha256=signature))
        if original[:4] == b"ef1\0":
            result['effect_templates'] = effect_sources.members(original, self.effects(entry)['actors'])
        return result


def spell_package(usual, native):
    """Locate a spell by its executable table ID, independently of cooked metadata."""
    usual = Path(usual)
    module = usual.parent.parent / "US_r_Top2Btl.rel"
    data = module.read_bytes()
    table = struct.unpack_from(">I", data, 0x10)[0]
    directory = (struct.unpack_from(">I", data, table + 5 * 8)[0] & ~1) + 0xfe0
    offsets = struct.unpack_from(">121I", data, directory)
    index = native - 200
    if not 1 <= index < len(offsets):
        raise ValueError(f"invalid original spell ID: {native}")
    archive = usual.with_name("BTLmagic.dat")
    start = offsets[index]
    if not start and index != 1:
        raise ValueError(f"missing original spell: {native}")
    end = next((value for value in offsets[index + 1:] if value > start), archive.stat().st_size)
    if not 0 <= start < end <= archive.stat().st_size:
        raise ValueError(f"invalid original spell range: {native}")
    with archive.open("rb") as stream:
        stream.seek(start)
        package = stream.read(end - start)
    return package


def scene_effect_range(package):
    pointers = struct.unpack_from(">68I", package, 4)
    start = pointers[0]
    end = min([value for value in pointers if value > start] + [len(package)])
    if not 4 + 68 * 4 <= start < end <= len(package) or package[start:start + 4] != b"ef1\0":
        raise ValueError("invalid original stored effect range")
    return [start, end]
