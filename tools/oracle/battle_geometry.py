"""Read count-bounded authored bone labels and initialized body geometry."""
import hashlib
import json
import math
from pathlib import Path
import re
import struct


def place_actor(state, actor, x, z):
    """Move a fixture root while preserving its observed center offset."""
    for offset, value in [(0x18C0, x), (0x18C8, z),
                          (0x195C, actor["center"][0] + x - actor["position"][0]),
                          (0x1964, actor["center"][2] + z - actor["position"][2])]:
        state.write(actor["address"] + offset, "f", value)


def model_world(origin, offset, angles, scale, flags, elevation, parent=None):
    """Ordinary model instance TRS; shared displayed scene matrices are not instances."""
    if flags & (0x40000000 | 0x20000) and parent is None:
        return None
    if flags & 0x20000:
        return parent
    position = list(offset) if flags & 0x40000000 else [a + b for a, b in zip(origin, offset)]
    if flags & 0x1000:
        position[1] = offset[1]
    if flags & 2:
        position[1] += elevation
    x, y, z = map(math.radians, angles)
    sx, sy, sz, cx, cy, cz = math.sin(x), math.sin(y), math.sin(z), math.cos(x), math.cos(y), math.cos(z)
    rotation = [[cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
                [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
                [-sy, cy * sx, cy * cx]]
    world = [value for i in range(3) for value in
             [*[rotation[i][j] * scale[j] for j in range(3)], position[i]]]
    if parent is None:
        return world
    return [sum(parent[row * 4 + axis] * world[axis * 4 + column] for axis in range(3))
            + (parent[row * 4 + 3] if column == 3 else 0)
            for row in range(3) for column in range(4)]


def escape_ascii(raw):
    """Rust byte escape_ascii representation; preserve every byte, including CRLF."""
    escapes = {9: r"\t", 10: r"\n", 13: r"\r", 34: r'\"', 39: r"\'", 92: r"\\"}
    return "".join(escapes.get(byte, chr(byte) if 32 <= byte <= 126 else f"\\x{byte:02x}")
                   for byte in raw)


def read_names(state, address, expected):
    """Read exactly the authored count of NUL names, bounded separately per label."""
    start = cursor = address - 0x80000000
    labels, raw_names = [], []
    for index, label in enumerate(expected):
        limit = len(label.encode("ascii")) + 1
        if not 0 <= cursor < cursor + limit <= len(state.ram):
            raise ValueError(f"initialized name {index} is outside main RAM")
        window = bytes(state.ram[cursor:cursor + limit])
        terminator = window.find(b"\0")
        if terminator < 0:
            raise ValueError(f"initialized name {index} exceeds its authored label bound")
        raw = window[:terminator]
        if not raw.isascii():
            raise ValueError("this correction only admits verified ASCII-authored labels")
        raw_names.append(raw.hex())
        labels.append(escape_ascii(raw))
        cursor += terminator + 1
    if labels != expected:
        raise ValueError("initialized ordered escaped bone names differ from authored descriptor")
    return dict(labels=labels, raw_names_hex=raw_names, count=len(labels),
                consumed_bytes=cursor - start, address=hex(address),
                read_bound="each escaped authored label byte length plus one NUL",
                comparison="exact ordered Rust escape_ascii labels; no trimming or normalization")


def read_initialized_body(state, actor, descriptor):
    descriptor = Path(descriptor)
    descriptor_bytes = descriptor.read_bytes()
    bones = json.loads(descriptor_bytes)["body"]["skeleton"]["bones"]
    if not all(math.isfinite(value) for value in [*actor["position"], *actor["center"], actor["heading"]]):
        raise ValueError("nonfinite initialized actor placement")

    def read(address, fmt="I"):
        if not 0x80000000 <= address <= 0x80000000 + len(state.ram) - struct.calcsize(">" + fmt):
            raise ValueError("initialized body field is outside main RAM")
        return state.read(address, fmt)

    address = actor["address"]
    profile = read(address + 4)
    scale = read(profile + 0x84, "f")
    if not math.isfinite(scale) or scale <= 0:
        raise ValueError("invalid initialized model scale")
    model = read(address + 0x2c0)
    if read(model + 0x1c, "B") & (0x10 | 4):
        raise ValueError("initialized body uses an unsupported model placement")

    def transform(base, position_at, angles_at, scale_at):
        position = [read(base + position_at + i * 4, "f") for i in range(3)]
        angles = [read(base + angles_at + i * 4, "f") for i in range(3)]
        scales = [read(base + scale_at + i * 4, "f") for i in range(3)]
        if not all(math.isfinite(value) for value in [*position, *angles, *scales]) or min(scales) <= 0:
            raise ValueError("invalid initialized body transform")
        return model_world(position, [0, 0, 0], angles, scales, 0, 0)

    published = transform(model, 0x3c, 0x2c, 0x20)
    current = transform(address, 0x18c0, 0x18cc, 0x18d8)
    skeleton = read(model)
    count = read(model + 4)
    if not 0 < count <= len(bones) or count != read(skeleton + 4):
        raise ValueError("initialized bone count differs from authored descriptor")
    expected = [bone["name"] for bone in bones]
    if count < len(bones):
        # Renderers can omit cosmetic subtrees. Keep every mechanical anchor,
        # and bind the remaining matrices by exact authored names and order.
        cursor = read(skeleton + 0x1c) - 0x80000000
        bound = max(len(name.encode("ascii")) for name in expected) + 1
        selected, start = [], 0
        for _ in range(count):
            if not 0 <= cursor < cursor + bound <= len(state.ram):
                raise ValueError("initialized bone name is outside main RAM")
            raw = bytes(state.ram[cursor:cursor + bound]).split(b"\0", 1)[0]
            if len(raw) == bound or not raw.isascii():
                raise ValueError("initialized bone name exceeds its authored bound")
            try:
                index = expected.index(escape_ascii(raw), start)
            except ValueError as error:
                raise ValueError("initialized bone names differ from authored order") from error
            selected.append(index)
            start = index + 1
            cursor += len(raw) + 1
        retained = set(selected)
        if any(index not in retained and re.match(r"^(?:mo[0-9]+|kk[0-9]+)", bone["name"], re.IGNORECASE)
               or index in retained and bone["parent"] is not None and bone["parent"] not in retained
               for index, bone in enumerate(bones)):
            raise ValueError("initialized skeleton omits an authored anchor or parent")
        bones = [bones[index] for index in selected]
    names = read_names(state, read(skeleton + 0x1c), [bone["name"] for bone in bones])
    names["declared_bytes"] = read(skeleton + 0x18)
    names["declared_minus_consumed_bytes"] = names["declared_bytes"] - names["consumed_bytes"]
    poses, matrices = read(model + 0x18), read(model + 0x50)
    spheres = []
    for index, bone in enumerate(bones):
        match = re.match(r"^mo([0-9]+)", bone["name"], re.IGNORECASE)
        if not match:
            continue
        units = int(match[1])
        if not 1 <= units <= 15:
            raise ValueError("body marker radius is outside native supported units")
        if read(read(poses + index * 4), "H") != index:
            raise ValueError("body pose record differs from authored index")
        matrix_at = matrices + index * 48
        matrix = [read(matrix_at + word * 4, "f") for word in range(12)]
        if not all(math.isfinite(value) for value in matrix):
            raise ValueError("nonfinite initialized displayed matrix")
        center = [matrix[3], matrix[7], matrix[11]]
        # Entry and fixture placement can precede the next displayed matrix
        # publication. Retain the observed pose in the actor's current placement.
        local = [sum(published[i * 4 + j] * (center[i] - published[i * 4 + 3])
                     for i in range(3)) / sum(published[i * 4 + j] ** 2 for i in range(3))
                 for j in range(3)]
        world = [sum(current[i * 4 + j] * local[j] for j in range(3)) + current[i * 4 + 3]
                 for i in range(3)]
        spheres.append(dict(index=index, name=bone["name"], units=units, radius=units * 10 * scale,
            matrix_address=hex(matrix_at), displayed_center=center, world_center=world,
            root_offset=[world[i] - actor["position"][i] for i in range(3)]))
    if not spheres:
        raise ValueError("body has no named sphere markers; do not guess a source capsule")
    return dict(actor_address=hex(address), root_position=actor["position"], heading=actor["heading"],
        effect_center=actor["center"], profile_address=hex(profile), model_scale=scale,
        model_address=hex(model), skeleton_address=hex(skeleton), displayed_matrices_address=hex(matrices),
        published_transform=published, current_transform=current,
        descriptor=dict(path=str(descriptor), sha256=hashlib.sha256(descriptor_bytes).hexdigest()),
        all_authored_names_verified=count, names=names, spheres=spheres)


def surface_gap(enemy, party, enemy_xz, party_xz):
    gap = min(math.hypot(enemy_xz[0] + e['root_offset'][0] - party_xz[0] - p['root_offset'][0],
                        enemy_xz[1] + e['root_offset'][2] - party_xz[1] - p['root_offset'][2])
              - e['radius'] - p['radius'] for e in enemy['spheres'] for p in party['spheres'])
    return 0.01 if gap < 0 else gap


def separation_for_gap(enemy, party, gap):
    """Exact right-hand solution at the retained pose, with both root Z values0."""
    if not math.isfinite(gap) or gap < 0:
        raise ValueError('expected finite nonnegative gap')
    bounds = []
    for e in enemy['spheres']:
        for p in party['spheres']:
            radius = gap + e['radius'] + p['radius']
            dz = e['root_offset'][2] - p['root_offset'][2]
            if radius >= abs(dz):
                bounds.append(p['root_offset'][0] - e['root_offset'][0]
                              + math.sqrt(max(0, radius * radius - dz * dz)))
    if not bounds:
        raise ValueError('no right-hand sphere pair can attain requested gap')
    distance = max(bounds)
    if distance < 0:
        raise ValueError('right-hand placement requires a negative root separation')
    if abs(surface_gap(enemy, party, [distance, 0], [0, 0]) - gap) > 0.001:
        raise ValueError('right-hand placement does not reproduce requested gap')
    return distance
