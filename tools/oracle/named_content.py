"""Current named documents and oracle-only pins; no legacy cooked reader.

The evidence directory is outside the editable library. It owns copies of the
original bytes used by the observer; cooked files remain pinned at their actual
paths and are admitted by the Rust production readers when evidence is consumed.
"""
import hashlib
import json
from pathlib import Path, PurePosixPath
import struct


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
        if descriptor.get("version") != 1 or descriptor.get("game") != "GQSEAF":
            raise ValueError("oracle requires a current named GQSEAF library")

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

    def scene_path(self, identity, *, skill=False):
        return self.resolve("battle/skills/mappings.json" if skill else "battle/scenes/mappings.json",
                            identity, "scene.json")

    def arena_path(self, identity):
        return self.resolve("battle/arenas/mappings.json", identity, "arena.json")

    def arena(self, path):
        return self.document(path, ("unsupported_reason", "light_position", "chain_acceleration",
            "actor_color", "translation", "yaw_degrees", "color", "camera_pitch_offset", "layers",
            "scenery_bones", "scenery_fade", "animation_rates", "texture_animations", "fog", "effects",
            "recurring_interval", "textures"))

    def world_encounters(self):
        world = self.document("world/world.json", ("script", "messages", "audio", "movement", "collision",
            "encounters", "landmarks", "guideposts", "continents", "actors", "cinematics", "dust", "skies"))
        return world["encounters"], self.json(world["encounters"])

    def image(self, path):
        if path.endswith(".texture.json"):
            definition = self.document(path, ("image", "color_space", "mips"))
            self.read(definition["image"])
            mips = definition.get("mips", {"kind": "none"})
            if mips["kind"] == "authored":
                for level in mips["levels"]:
                    self.read(level)
            elif mips["kind"] not in ("none", "generate"):
                raise ValueError(f"invalid mip policy: {path}")
        else:
            self.read(path)

    def part(self, part):
        self.read(part["mesh"])
        for image in part["textures"]:
            self.image(image)
        for clip in part["clips"]:
            motion = self.document(clip["motion"], ("duration_frames", "tracks"))
            clip["duration_seconds"] = motion["duration_frames"] / 60.0

    def model(self, path):
        model = self.document(path, ("model",))["model"]
        for layer in model["layers"]:
            self.part(layer["scene"])
        return model

    def enemy_entry(self, identity):
        path = self.resolve("battle/enemies/mappings.json", identity, "enemy.json")
        if path is None:
            raise ValueError(f"unmapped enemy: {identity}")
        return path, self.document(path, ("rules", "artwork", "effects"))

    def enemy_rules(self, identity):
        _, entry = self.enemy_entry(identity)
        return self.document(entry["rules"], ("null_motions", "profile", "weapon_visibility",
            "projectiles", "actions", "target_strategy", "entry_row", "guard_recovery_bonus", "presentation"))

    def enemy_artwork(self, identity):
        _, entry = self.enemy_entry(identity)
        return self.document(entry["artwork"], ("rig", "model", "secondary_body", "body_variant",
            "attached_models", "attachments", "uv_animation", "weapon_variants", "trails"))

    def enemy(self, identity):
        path, _ = self.enemy_entry(identity)
        rules = self.enemy_rules(identity)
        artwork = self.enemy_artwork(identity)
        self.json(artwork["rig"])
        preview = self.document(artwork["model"], ("scale", "elevation", "parts", "hidden_geometry", "behavior"))
        for part in preview["parts"]:
            self.part(part["scene"])
        return path, rules, preview

    def effects(self, path):
        definition = self.document(path, ("programs", "actors", "art"))
        programs = self.json(definition["programs"])
        actors = self.json(definition["actors"])
        art = None
        if definition.get("art"):
            artwork = self.document(definition["art"], ("textures", "models"))
            art = dict(textures=artwork["textures"], models={
                slot: self.model(model) for slot, model in artwork["models"].items()})
            for textures in art["textures"].values():
                for texture in textures:
                    for image in texture["images"]:
                        self.image(image["path"])
        return dict(programs=programs, actors=actors, art=art)

    def scene(self, path):
        definition = self.document(path, ("rules", "effects", "cutin", "weapon"))
        rules = self.document(definition["rules"], ("projectiles", "actions", "actions_error", "duration",
            "stage_color", "camera", "strike_points", "recovery_radius", "spell_recipe"))
        effects = self.effects(definition["effects"])
        art = effects["art"] or dict(models={}, textures={})
        weapon = definition.get("weapon")
        if weapon:
            weapon = dict(weapon, parts={slot: self.model(model) for slot, model in weapon["parts"].items()})
        cutin = definition.get("cutin")
        if cutin:
            for image in cutin["images"]:
                self.image(image["path"])
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
        return dict(version=1, entry=entry, files={path: pin for path, (_, pin) in sorted(self.files.items())},
                    source=dict(path=str(source), sha256=signature))


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
