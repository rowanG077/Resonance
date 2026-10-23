"""Observe authored model attachments through typed model descriptors."""
import hashlib
import json
from pathlib import Path


DESCRIPTOR_TABLE = 0x7a63c
DESCRIPTOR_BYTES = 0x7e0
STORED_MODEL_BANKS = (6, 7)


def prepare(case, assets):
    execution = case.get("definition", {}).get("execution")
    attack = execution.get("attack", {}) if isinstance(execution, dict) else {}
    natives = {event["release_spell"]["technique"]
               for _, event in attack.get("events", [])
               if isinstance(event, dict) and "release_spell" in event}
    if case.get("native") is not None:
        natives.add(case["native"])
    result = []
    for native in sorted(natives):
        path = Path(assets) / f"battle/scenes/{native:03}.json"
        if not path.is_file():
            continue
        scene = json.loads(path.read_text())
        effects = json.loads((Path(assets) / scene["effects"]).read_text())
        attachments = [actor["binding"]["attachment"] for actor in effects["actors"]
                       if actor.get("binding", {}).get("attachment")]
        if not attachments:
            continue
        joints = []
        for model, definition in scene["models"].items():
            names = [bone["name"] for bone in definition["rig"]["skeleton"]["bones"]]
            for joint in sorted({attachment["joint"] for attachment in attachments}):
                matches = [index for index, name in enumerate(names) if name.startswith(f"kk0{joint}")]
                if len(matches) > 1:
                    raise ValueError("model attachment joint name is ambiguous")
                if matches:
                    joints.append(dict(model=int(model), joint=joint, bone=matches[0]))
        if set(attachment["joint"] for attachment in attachments) - {entry["joint"] for entry in joints}:
            raise ValueError("model attachment has no authored joint")
        result.append(dict(bank=effects["source_sha256"],
            scene=dict(path=str(path.resolve()), sha256=hashlib.sha256(path.read_bytes()).hexdigest()),
            retained=sorted({attachment["retained"] for attachment in attachments}), joints=joints))
    return result


def retained_slots(plan):
    return sorted({index for entry in plan for index in entry["retained"]})


def locations(pool, plan):
    result = {}
    joints = {(entry["model"], entry["bone"]) for scene in plan for entry in scene["joints"]}
    for bank in STORED_MODEL_BANKS if joints else ():
        table = f"{pool} {DESCRIPTOR_TABLE + bank * 4:x}"
        result[table] = f"model_bank_{bank}"
        for model, bone in sorted(joints):
            node = f"{table} {model * DESCRIPTOR_BYTES:x} 18 {bone * 4:x}"
            prefix = f"model_joint_{bank}_{model}_{bone}"
            result[node + " 14"] = prefix + "_cache"
            for word in range(12):
                result[f"{node} 14 {0x18 + word * 4:x}"] = prefix + f"_{word}"
    return result


def observed(row, prefix, bank, binding, plan, records, actors, f32, *, relation=False):
    """Join the retained live parent to its observed descriptor and joint cache."""
    attachment = binding.get("attachment")
    entries = [entry for entry in plan if entry["bank"] == bank]
    if attachment is None or len(entries) != 1:
        return None
    packed = row[prefix + "_b4"]
    if packed >> 24 != attachment["joint"] or (packed >> 16) & 255 != attachment["retained"]:
        return None
    manager = row[prefix + "_184"]
    by_address = {address: name for name, address in records}
    scheduler = by_address.get(manager - 0x28)
    if scheduler is not None and row[scheduler + "_10"] >> 24 == 0:
        retained = row.get(f"{scheduler}_{0xc0 + attachment['retained'] * 8:x}")
    else:
        actor = next((actor for actor in actors if actor["address"] + 0x15b4 == manager), None)
        retained = row.get(f"actor_{actor['slot']}_retained_{attachment['retained']}") if actor else None
    parent = by_address.get(retained - 0x28) if retained else None
    if (parent is None or row[parent + "_10"] >> 24 != 1
            or row[parent + "_28"] >> 24 != 3 or row[parent + "_184"] != manager):
        return None
    descriptor, model = row[parent + "_168"], row[parent + "_fc"] >> 24
    joints = [joint for joint in entries[0]["joints"]
              if joint["model"] == model and joint["joint"] == attachment["joint"]]
    banks = [index for index in STORED_MODEL_BANKS
             if row.get(f"model_bank_{index}", 0) + model * DESCRIPTOR_BYTES == descriptor]
    if len(joints) != 1 or len(banks) != 1:
        return None
    observed = f"model_joint_{banks[0]}_{model}_{joints[0]['bone']}"
    cache = row.get(observed + "_cache", 0)
    if not 0x80000000 <= cache <= 0x81800000 - 0x48 or cache & 3:
        return None
    if any(observed + f"_{word}" not in row for word in range(12)):
        return None
    matrix = [f32(row[observed + f"_{word}"]) for word in range(12)]
    if any(value is None for value in matrix):
        return None
    if not relation:
        return matrix
    controllers = []
    for offset in (0x100, 0x134):
        resource = row.get(f"{parent}_{offset:x}", 0)
        entry = resource - descriptor - 0x640
        frame = f32(row[f"{parent}_{offset + 8:x}"]) if resource else None
        if 0 <= entry <= 0x7e0 - 0x640 - 20 and entry % 20 == 0 and frame is not None:
            controllers.append(dict(clip=entry // 20, frame=frame))
    return dict(world=matrix, parent=retained - 0x28, joint=attachment["joint"],
                model=model, controllers=controllers)
