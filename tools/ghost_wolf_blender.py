#!/usr/bin/env python3
"""Prepare Ghost Wolf's local Blender resources or export authored motion deltas."""
import argparse
import bisect
import json
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile


def sources(assets):
    body = json.loads((assets / 'meshes/characters/lloyd/model.json').read_text())
    sword = json.loads((assets / 'meshes/equipment/wooden-blade/equipment.json').read_text())['parts']['0']['layers'][0]
    return [
        ('body', body['parts'][0], body['shared']['textures']),
        ('sword', sword, sword['textures']),
    ]


def decode_texture(source, target):
    from compression import zstd
    from PIL import Image
    data = source.read_bytes()
    header = struct.unpack_from('<9I', data, 12)
    fmt, size, width, height, depth, layers, faces, levels, compression = header
    if data[:12] != b'\xabKTX 20\xbb\r\n\x1a\n' or (fmt, size, depth, layers, faces, compression) != (37, 1, 0, 0, 1, 2) or levels < 1:
        raise ValueError(f'Expected imported RGBA8/Zstd KTX2: {source}')
    offset, length, raw_length = struct.unpack_from('<3Q', data, 80)
    pixels = zstd.decompress(data[offset:offset + length])
    if len(pixels) != raw_length or raw_length != width * height * 4:
        raise ValueError(f'Invalid texture dimensions: {source}')
    target.parent.mkdir(parents=True, exist_ok=True)
    Image.frombytes('RGBA', (width, height), pixels).save(target)


def preview_material(bpy, source, textures, output, label):
    material = bpy.data.materials.new(label)
    material.use_nodes = True
    shader = material.node_tree.nodes.get('Principled BSDF')
    color = source.get('color', {})
    if 'texture' in color:
        shader.inputs['Roughness'].default_value = 0.8
        image_path = output / 'textures' / Path(textures[color['texture']]).with_suffix('.png').name
        image = bpy.data.images.load(str(image_path), check_existing=True)
        image.pack()
        node = material.node_tree.nodes.new('ShaderNodeTexImage')
        node.image = image
        node.interpolation = 'Closest'
        material.node_tree.links.new(node.outputs['Color'], shader.inputs['Base Color'])
        material.node_tree.links.new(node.outputs['Alpha'], shader.inputs['Alpha'])
    else:
        node = material.node_tree.nodes.new('ShaderNodeVertexColor')
        node.layer_name = 'Color'
        material.node_tree.links.new(node.outputs['Color'], shader.inputs['Base Color'])
    material.surface_render_method = 'DITHERED'
    return material


# The distributable GLB contains named animation deltas, never the locally
# reconstructed skeleton, reference pose, meshes or textures.
_COMPONENTS = {'SCALAR': 1, 'VEC3': 3, 'VEC4': 4}
_DEFAULTS = {'translation': (0.0, 0.0, 0.0), 'rotation': (0.0, 0.0, 0.0, 1.0), 'scale': (1.0, 1.0, 1.0)}


def read_glb(path):
    data = Path(path).read_bytes()
    if len(data) < 20 or struct.unpack_from('<4sII', data) != (b'glTF', 2, len(data)):
        raise ValueError(f'Invalid GLB: {path}')
    document, binary = None, None
    offset = 12
    while offset < len(data):
        length, kind = struct.unpack_from('<II', data, offset)
        chunk = data[offset + 8:offset + 8 + length]
        if len(chunk) != length:
            raise ValueError(f'Truncated GLB: {path}')
        if kind == 0x4E4F534A:
            document = json.loads(chunk)
        elif kind == 0x004E4942:
            binary = chunk
        offset += 8 + length
    if document is None or binary is None:
        raise ValueError(f'Missing GLB chunks: {path}')
    return document, binary


def accessor(document, binary, index):
    item = document['accessors'][index]
    if item['componentType'] != 5126 or 'sparse' in item:
        raise ValueError('Motion export requires nonsparse float32 accessors')
    view = document['bufferViews'][item['bufferView']]
    if view.get('buffer', 0) != 0:
        raise ValueError('Motion export requires a single embedded buffer')
    width = _COMPONENTS[item['type']]
    stride = view.get('byteStride', width * 4)
    offset = view.get('byteOffset', 0) + item.get('byteOffset', 0)
    return [struct.unpack_from('<' + 'f' * width, binary, offset + i * stride) for i in range(item['count'])]


def unit(q):
    length = math.sqrt(sum(v * v for v in q))
    if length == 0:
        raise ValueError('Zero quaternion in motion')
    return tuple(v / length for v in q)


def multiply(a, b):
    x, y, z, w = a
    u, v, s, t = b
    return (w*u + x*t + y*s - z*v,
            w*v - x*s + y*t + z*u,
            w*s + x*v - y*u + z*t,
            w*t - x*u - y*v - z*s)


def sample(times, values, time, interpolation, rotation=False):
    if interpolation == 'CUBICSPLINE':
        if time <= times[0]:
            value = values[1]
        elif time >= times[-1]:
            value = values[-2]
        else:
            i = bisect.bisect_right(times, time) - 1
            span = times[i + 1] - times[i]
            t = (time - times[i]) / span
            weights = (2*t**3 - 3*t*t + 1, t**3 - 2*t*t + t,
                       -2*t**3 + 3*t*t, t**3 - t*t)
            value = tuple(weights[0]*a + weights[1]*span*b + weights[2]*c + weights[3]*span*d
                          for a,b,c,d in zip(values[3*i+1], values[3*i+2], values[3*i+4], values[3*i+3]))
    else:
        i = min(len(times) - 1, max(0, bisect.bisect_right(times, time) - 1))
        value = values[i]
        if interpolation == 'LINEAR' and i + 1 < len(times) and time > times[i]:
            t = (time - times[i]) / (times[i + 1] - times[i])
            other = values[i + 1]
            if rotation:
                value, other = unit(value), unit(other)
                dot = sum(a*b for a,b in zip(value, other))
                if dot < 0:
                    other = tuple(-v for v in other)
                    dot = -dot
                if dot < 0.9995:
                    theta = math.acos(min(1.0, dot))
                    sin_theta = math.sin(theta)
                    first, second = math.sin((1-t)*theta)/sin_theta, math.sin(t*theta)/sin_theta
                    value = tuple(first*a + second*b for a,b in zip(value, other))
                else:
                    value = tuple((1-t)*a + t*b for a,b in zip(value, other))
            else:
                value = tuple((1-t)*a + t*b for a,b in zip(value, other))
    return unit(value) if rotation else tuple(value)


def reference_pose(skeleton_path, reference_path, frame=0.0):
    skeleton, _ = read_glb(skeleton_path)
    joints = {index for skin in skeleton.get('skins', []) for index in skin['joints']}
    pose = {}
    for index in joints:
        node = skeleton['nodes'][index]
        if 'matrix' in node:
            raise ValueError('Cooked skeleton must use TRS nodes')
        pose[node['name']] = {path: tuple(node.get(path, default)) for path, default in _DEFAULTS.items()}
        pose[node['name']]['rotation'] = unit(pose[node['name']]['rotation'])
    motion, binary = read_glb(reference_path)
    animation = motion['animations'][0]
    for channel in animation['channels']:
        target = channel['target']
        name = motion['nodes'][target['node']]['name']
        if name not in pose:
            raise ValueError(f'Reference motion targets unknown bone: {name}')
        sampler = animation['samplers'][channel['sampler']]
        times = [row[0] for row in accessor(motion, binary, sampler['input'])]
        values = accessor(motion, binary, sampler['output'])
        path = target['path']
        pose[name][path] = sample(times, values, frame / 30.0, sampler.get('interpolation', 'LINEAR'), path == 'rotation')
    return pose


def delta_values(path, reference, values, interpolation):
    inverse_rotation = (-reference[0], -reference[1], -reference[2], reference[3]) if path == 'rotation' else None
    output = []
    for index, value in enumerate(values):
        tangent = interpolation == 'CUBICSPLINE' and index % 3 != 1
        if path == 'translation':
            delta = value if tangent else tuple(a-b for a,b in zip(value, reference))
        elif path == 'rotation':
            delta = multiply(inverse_rotation, value if tangent else unit(value))
        elif path == 'scale':
            # Additive offsets also represent animated visibility from a zero reference scale.
            delta = value if tangent else tuple(a-b for a,b in zip(value, reference))
        else:
            raise ValueError(f'Unsupported motion channel: {path}')
        output.append(delta)
    return output


def export_reference_motion(absolute_path, skeleton_path, reference_path, output_path, reference_frame=0.0):
    original, source_binary = read_glb(absolute_path)
    reference = reference_pose(skeleton_path, reference_path, reference_frame)
    if len(original.get('animations', [])) != 1:
        raise ValueError('Ghost Wolf export expects one animation')
    original_animation = original['animations'][0]
    # Include static bones too: Blender may omit their animation channels.
    animated = [i for i, node in enumerate(original['nodes']) if node.get('name') in reference]
    if {original['nodes'][i]['name'] for i in animated} != set(reference):
        raise ValueError('Absolute export must contain every reference skeleton bone')
    if any('matrix' in original['nodes'][i] for i in animated):
        raise ValueError('Absolute export must use TRS bone nodes')
    nodes = [{'name': original['nodes'][index]['name']} for index in animated]
    if len({node['name'] for node in nodes}) != len(nodes):
        raise ValueError('Animation bone names must be unique')
    node_indices = {old: new for new, old in enumerate(animated)}
    animation = {'name': original_animation.get('name', 'Ghost Wolf'), 'channels': [], 'samplers': []}
    document = {'asset': {'version': '2.0', 'generator': 'Resonance reference motion export'},
                'scene': 0, 'scenes': [{'nodes': list(range(len(nodes)))}], 'nodes': nodes,
                'animations': [animation], 'accessors': [], 'bufferViews': [], 'buffers': []}
    binary = bytearray()
    time_accessors = {}

    def append(rows, kind):
        index = len(document['accessors'])
        offset = len(binary)
        for row in rows:
            binary.extend(struct.pack('<' + 'f' * len(row), *row))
        document['bufferViews'].append({'buffer': 0, 'byteOffset': offset, 'byteLength': len(binary) - offset})
        item = {'bufferView': index, 'componentType': 5126, 'count': len(rows), 'type': kind}
        if kind == 'SCALAR':
            item.update(min=[min(row[0] for row in rows)], max=[max(row[0] for row in rows)])
        document['accessors'].append(item)
        return index

    present = set()
    for channel in original_animation['channels']:
        target = channel['target']
        name = original['nodes'][target['node']]['name']
        if name not in reference:
            raise ValueError(f'Animation targets unknown bone: {name}')
        path = target['path']
        present.add((target['node'], path))
        sampler = original_animation['samplers'][channel['sampler']]
        interpolation = sampler.get('interpolation', 'LINEAR')
        if interpolation not in ('LINEAR', 'STEP', 'CUBICSPLINE'):
            raise ValueError(f'Unsupported interpolation: {interpolation}')
        times = tuple(accessor(original, source_binary, sampler['input']))
        if times not in time_accessors:
            time_accessors[times] = append(times, 'SCALAR')
        values = accessor(original, source_binary, sampler['output'])
        output = append(delta_values(path, reference[name][path], values, interpolation), 'VEC4' if path == 'rotation' else 'VEC3')
        animation['channels'].append({'sampler': len(animation['samplers']), 'target': {'node': node_indices[target['node']], 'path': path}})
        animation['samplers'].append({'input': time_accessors[times], 'output': output, 'interpolation': interpolation})
    for index in animated:
        node = original['nodes'][index]
        for path, default in _DEFAULTS.items():
            if (index, path) in present:
                continue
            times = ((0.0,),)
            if times not in time_accessors:
                time_accessors[times] = append(times, 'SCALAR')
            values = [tuple(node.get(path, default))]
            output = append(delta_values(path, reference[node['name']][path], values, 'LINEAR'), 'VEC4' if path == 'rotation' else 'VEC3')
            animation['channels'].append({'sampler': len(animation['samplers']), 'target': {'node': node_indices[index], 'path': path}})
            animation['samplers'].append({'input': time_accessors[times], 'output': output, 'interpolation': 'LINEAR'})
    document['buffers'] = [{'byteLength': len(binary)}]
    encoded = json.dumps(document, separators=(',', ':')).encode()
    encoded += b' ' * (-len(encoded) % 4)
    binary += b'\0' * (-len(binary) % 4)
    result = struct.pack('<4sII', b'glTF', 2, 28 + len(encoded) + len(binary))
    result += struct.pack('<II', len(encoded), 0x4E4F534A) + encoded
    result += struct.pack('<II', len(binary), 0x004E4942) + binary
    Path(output_path).write_bytes(result)
    return {'nodes': len(nodes), 'channels': len(animation['channels']), 'bytes': len(result)}


CONTROL_ALIASES = {
    "shadow": "Root",
    "mo6-hips": "Hips",
    "spine-01": "Spine lower",
    "spine-02": "Spine middle",
    "spine-03": "Chest",
    "neck": "Neck",
    "head": "Head",
    "shoulder-left": "Clavicle.L",
    "arm-01-left": "Upper arm.L",
    "arm-02-left": "Elbow.L",
    "arm-03-left": "Forearm roll.L",
    "wrist-left": "Wrist.L",
    "hand-left": "Palm.L",
    "shoulder-right": "Clavicle.R",
    "arm-01-right": "Upper arm.R",
    "arm-02-right": "Elbow.R",
    "arm-03-right": "Forearm roll.R",
    "wrist-right": "Wrist.R",
    "hand-right": "Palm.R",
    "leg-01-left": "Thigh.L",
    "leg-02-left": "Shin.L",
    "leg-03-left": "Foot.L",
    "leg-04-left": "Toe.L",
    "leg-01-right": "Thigh.R",
    "leg-02-right": "Shin.R",
    "leg-03-right": "Foot.R",
    "leg-04-right": "Toe.R"
}
IDLE_MOTION = 'battle/party/lloyd/default/animations/idle.glb'

def sanitize_blend_pose_cache(path):
    from compression import zstd
    path = Path(path)
    raw = path.read_bytes()
    data = bytearray(zstd.decompress(raw) if raw[:4] == b'\x28\xb5\x2f\xfd' else raw)
    if data[:17] != b'BLENDER17-01v0502':
        raise ValueError('Pose-cache sanitization requires the project Blender 5.2 format')
    blocks = []
    offset = 17
    while offset + 32 <= len(data):
        code, schema, address, size, count = struct.unpack_from('<4sIQQQ', data, offset)
        if offset + 32 + size > len(data):
            raise ValueError('Invalid Blender block size')
        blocks.append((code, schema, offset + 32, size, count))
        offset += 32 + size
        if code == b'ENDB':
            break
    if offset != len(data):
        raise ValueError('Unexpected trailing Blender data')
    dna_block = next(block for block in blocks if block[0] == b'DNA1')
    dna = data[dna_block[2]:dna_block[2] + dna_block[3]]
    if dna[:8] != b'SDNANAME':
        raise ValueError('Missing Blender structure definitions')
    position = 8

    def strings():
        nonlocal position
        count = struct.unpack_from('<I', dna, position)[0]
        position += 4
        result = []
        for _ in range(count):
            end = dna.index(0, position)
            result.append(dna[position:end].decode())
            position = end + 1
        position = (position + 3) & ~3
        return result

    names = strings()
    if dna[position:position + 4] != b'TYPE':
        raise ValueError('Missing Blender types')
    position += 4
    types = strings()
    if dna[position:position + 4] != b'TLEN':
        raise ValueError('Missing Blender type lengths')
    position += 4
    sizes = struct.unpack_from('<' + str(len(types)) + 'H', dna, position)
    position = (position + len(types) * 2 + 3) & ~3
    if dna[position:position + 4] != b'STRC':
        raise ValueError('Missing Blender structures')
    position += 4
    structure_count = struct.unpack_from('<I', dna, position)[0]
    position += 4
    pose_schema = None
    object_schema = None
    object_inverse = None
    fields = {}
    for schema in range(structure_count):
        type_index, count = struct.unpack_from('<HH', dna, position)
        position += 4
        field_offset = 0
        for _ in range(count):
            field_type, field_name = struct.unpack_from('<HH', dna, position)
            position += 4
            name = names[field_name]
            elements = 1
            for dimension in re.findall(r'\[(\d+)\]', name):
                elements *= int(dimension)
            size = elements * (8 if '*' in name else sizes[field_type])
            if types[type_index] == 'bPoseChannel':
                fields[name] = (field_offset, size, types[field_type])
            elif types[type_index] == 'Object' and name == 'constinv[4][4]':
                object_inverse = (field_offset, size, types[field_type])
            field_offset += size
        if types[type_index] == 'bPoseChannel':
            if field_offset != sizes[type_index]:
                raise ValueError('Unexpected pose-channel field alignment')
            pose_schema, pose_size = schema, field_offset
        elif types[type_index] == 'Object':
            if field_offset != sizes[type_index]:
                raise ValueError('Unexpected object field alignment')
            object_schema, object_size = schema, field_offset
    if pose_schema is None:
        raise ValueError('Missing pose-channel structure')

    values = {name: [0.0] * count for name, count in [
        ('loc[3]', 3), ('eul[3]', 3), ('rotAngle', 1),
        ('pose_head[3]', 3), ('pose_tail[3]', 3)]}
    values.update({'quat[4]': [1.0, 0.0, 0.0, 0.0], 'size[3]': [1.0] * 3,
                   'rotAxis[3]': [0.0, 1.0, 0.0]})
    for name in ['chan_mat', 'pose_mat', 'disp_mat', 'disp_tail_mat', 'constinv']:
        values[name + '[4][4]'] = [float(i % 5 == 0) for i in range(16)]
    patches = []
    for name, value in values.items():
        field_offset, size, field_type = fields[name]
        if field_type != 'float' or size != len(value) * 4:
            raise ValueError(f'Unexpected pose-channel field: {name}')
        patches.append((field_offset, struct.pack('<' + str(len(value)) + 'f', *value)))
    field_offset, size, _ = fields['runtime']
    patches.append((field_offset, bytes(size)))
    # Rebuilt from the linked armature on load; do not embed its parent graph.
    for name in ['*parent', '*child']:
        field_offset, size, field_type = fields[name]
        if size != 8 or field_type != 'bPoseChannel':
            raise ValueError(f'Unexpected pose hierarchy cache: {name}')
        patches.append((field_offset, bytes(size)))
    total = 0
    for code, schema, offset, size, count in blocks:
        if code in [b'DNA1', b'ENDB'] or schema != pose_schema:
            continue
        if size != count * pose_size:
            raise ValueError('Unexpected pose-channel block size')
        for index in range(count):
            for field_offset, value in patches:
                start = offset + index * pose_size + field_offset
                data[start:start + len(value)] = value
        total += count
    if object_schema is None or object_inverse is None:
        raise ValueError('Missing object constraint cache')
    field_offset, field_size, field_type = object_inverse
    if field_size != 64 or field_type != 'float':
        raise ValueError('Unexpected object constraint cache')
    identity = struct.pack('<16f', *[float(i % 5 == 0) for i in range(16)])
    for code, schema, offset, size, count in blocks:
        if code in [b'DNA1', b'ENDB'] or schema != object_schema:
            continue
        if size != count * object_size:
            raise ValueError('Unexpected object block size')
        for index in range(count):
            start = offset + index * object_size + field_offset
            data[start:start + 64] = identity
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.' + path.name, delete=False) as output:
        output.write(zstd.compress(data))
        temporary = Path(output.name)
    temporary.replace(path)
    return total


def control_name(native):
    return CONTROL_ALIASES.get(native, f'Detail · {native}')


def build_library(assets, preview):
    import bpy
    from mathutils import Matrix, Quaternion, Vector
    bpy.ops.wm.read_factory_settings(use_empty=True)
    datablocks = set()
    control_data = None
    native_data = None

    def placement(name, matrix):
        obj = bpy.data.objects.new(name, None)
        obj.rotation_mode = 'QUATERNION'
        obj.matrix_basis = matrix
        obj.empty_display_size = 0.5
        datablocks.add(obj)

    for kind, part, textures in sources(assets):
        path = assets / part['mesh']
        document, _ = read_glb(path)
        indices = {node['name']: node['mesh'] for node in document['nodes'] if 'mesh' in node}
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(path), bone_heuristic='BLENDER',
                                  guess_original_bind_pose=False, disable_bone_shape=True)
        imported = set(bpy.data.objects) - before
        if kind == 'body':
            native_data = next(obj.data for obj in imported if obj.type == 'ARMATURE')
            native_data.name = 'Ghost Wolf native skeleton'
            control_data = native_data.copy()
            control_data.name = 'Ghost Wolf controls skeleton'
            for collection in list(control_data.collections):
                control_data.collections.remove(collection)
            controls = control_data.collections.new('Controls · body, arms and legs')
            advanced = control_data.collections.new('Advanced · skin rings, fingers and attachments')
            advanced.is_visible = False
            for bone in control_data.bones:
                bone.name = control_name(bone.name)
                collection = advanced if bone.name.startswith('Detail · ') or bone.name.startswith('Wrist.') else controls
                collection.assign(bone)
            datablocks.update([native_data, control_data])
            # Native bone-tail/attachment calibration belongs to the local library.
            world = {}
            def visit(index, parent):
                node = document['nodes'][index]
                q = node.get('rotation', (0, 0, 0, 1))
                local = Matrix.LocRotScale(Vector(node.get('translation', (0, 0, 0))),
                    Quaternion((q[3], q[0], q[1], q[2])), Vector(node.get('scale', (1, 1, 1))))
                world[node['name']] = parent @ local
                for child in node.get('children', []):
                    visit(child, world[node['name']])
            for root in document['scenes'][document.get('scene', 0)]['nodes']:
                visit(root, Matrix.Identity(4))
            conversion = Matrix.Rotation(math.pi / 2, 4, 'X')
            for socket in ('kk00', 'kk01'):
                bone = native_data.bones[socket]
                native = conversion @ world[socket] @ conversion.inverted()
                calibration = Matrix.Translation((0, -bone.length, 0)) @ bone.matrix_local.inverted() @ native
                placement(f'Placement · socket · {socket}', calibration)
        for obj in imported:
            if kind == 'body' and obj.parent_type == 'BONE':
                placement(f'Placement · body · {obj.name}', obj.matrix_basis.copy())
            elif kind == 'sword' and obj.type == 'EMPTY':
                placement(f'Placement · sword · {obj.name}', obj.matrix_basis.copy())
        materials = {}
        for i, definition in enumerate(part['materials']):
            material = preview_material(bpy, definition, textures, preview, f'Ghost Wolf {kind} material {i}')
            for index in definition['meshes']:
                materials[index] = material
        found = set()
        for obj in imported:
            if obj.type != 'MESH':
                continue
            name = obj.name if obj.name in indices else obj.parent.name if obj.parent else None
            if name not in indices:
                continue
            index = indices[name]
            if kind == 'body':
                for group in obj.vertex_groups:
                    group.name = control_name(group.name)
            obj.data.name = f'Ghost Wolf {kind} mesh {index:02}'
            obj.data.materials.clear()
            obj.data.materials.append(materials[index])
            datablocks.add(obj.data)
            found.add(index)
        if found != set(range(len(document['meshes']))):
            raise ValueError(f'Missing imported preview meshes: {part["mesh"]}')
    # The native reference pose lives only in this ignored library.
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(assets / IDLE_MOTION))
    idle_rig = next(obj for obj in set(bpy.data.objects) - before if obj.type == 'ARMATURE')
    bpy.context.scene.frame_set(0)
    control_rig = bpy.data.objects.new('Preview reference controls', control_data)
    bpy.context.scene.collection.objects.link(control_rig)
    bpy.context.view_layer.update()
    reference_action = bpy.data.actions.new('Ghost Wolf native idle reference')
    control_rig.animation_data_create().action = reference_action
    for native in idle_rig.pose.bones:
        bone = control_rig.pose.bones[control_name(native.name)]
        location, rotation, scale = native.matrix_basis.decompose()
        bone.rotation_mode = 'QUATERNION'
        bone.location = location
        bone.rotation_quaternion = rotation
        # COMBINE multiplies scale. Identity permits authored visibility on
        # sheath bones whose native idle scale is zero.
        bone.scale = tuple(value if abs(value) > 1e-8 else 1.0 for value in scale)
        for frame in (0, 1):
            for property_name in ('location', 'rotation_quaternion', 'scale'):
                bone.keyframe_insert(property_name, frame=frame, group=bone.name)
    datablocks.add(reference_action)
    bpy.context.preferences.filepaths.file_preview_type = 'NONE'
    bpy.data.libraries.write(str(preview / 'preview.blend'), datablocks, fake_user=False, compress=True)
    print(f'Wrote local preview meshes, rigs and reference pose to {preview / "preview.blend"}', flush=True)


def export_absolute(source, preview):
    import bpy
    bpy.ops.wm.open_mainfile(filepath=str(source))
    expected_meshes = {'Plane', 'Shape · palm box', 'Shape · root diamond', 'Shape · rotation ring'}
    local_meshes = {mesh.name for mesh in bpy.data.meshes if not mesh.library}
    local_materials = {material.name for material in bpy.data.materials if not material.library}
    if (local_meshes != expected_meshes or local_materials != {'Warm grey floor'}
            or any(not armature.library for armature in bpy.data.armatures)
            or any(not image.library for image in bpy.data.images)
            or any(not action.library and action.name.startswith('Ghost Wolf native idle reference')
                   for action in bpy.data.actions)
            or any(not obj.library and obj.name.startswith('Placement · ') for obj in bpy.data.objects)):
        raise ValueError('Source contains unexpected local art, rig or reference data; keep native resources linked')
    if any(not Path(bpy.path.abspath(library.filepath)).is_file() for library in bpy.data.libraries):
        raise ValueError('Missing local preview library; run prepare before exporting')
    exporter = bpy.data.collections['03 — Runtime export'].exporters[0]
    exporter.filepath = bpy.path.relpath(str(preview / 'absolute.glb'), start=str(source.parent))
    exporter.export_properties.filepath = str(preview / 'absolute.glb')
    if bpy.ops.wm.collection_export_all() != {'FINISHED'}:
        raise RuntimeError('Blender animation export failed')
    exporter.export_properties.filepath = ''
    bpy.context.preferences.filepaths.save_version = 0
    bpy.context.preferences.filepaths.file_preview_type = 'NONE'
    bpy.ops.wm.save_as_mainfile(filepath=str(source), compress=True)


def main():
    in_blender = '--blender-stage' in sys.argv
    arguments = sys.argv[sys.argv.index('--') + 1:] if in_blender else sys.argv[1:]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('prepare', 'export'))
    parser.add_argument('--blender-stage', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--assets', type=Path, default=Path('local/all-assets'))
    parser.add_argument('--preview', type=Path, default=Path('local/ghost-wolf-preview'))
    parser.add_argument('--source', type=Path, default=Path('mods/ghost-wolf/source/ghost-wolf.blend'))
    parser.add_argument('--destination', type=Path, default=Path('mods/ghost-wolf/battle/artes/ghost-wolf/ghost-wolf.glb'))
    parser.add_argument('--blender', default='blender')
    args = parser.parse_args(arguments)
    assets, preview, source = args.assets.resolve(), args.preview.resolve(), args.source.resolve()
    preview.mkdir(parents=True, exist_ok=True)
    if in_blender:
        if args.operation == 'prepare':
            build_library(assets, preview)
        else:
            export_absolute(source, preview)
        # Background Blender's audio shutdown can hang in headless environments.
        os._exit(0)
    if args.operation == 'prepare':
        for _, _, textures in sources(assets):
            for path in textures:
                decode_texture(assets / path, preview / 'textures' / Path(path).with_suffix('.png').name)
    subprocess.run([args.blender, '--background', '--factory-startup', '--python-exit-code', '1',
                    '--python', str(Path(__file__).resolve()), '--', args.operation, '--blender-stage',
                    '--assets', str(assets), '--preview', str(preview), '--source', str(source)], check=True)
    if args.operation == 'export':
        sanitize_blend_pose_cache(source)
        reference = json.loads((source.parent.parent / 'example.json').read_text())['motion_reference']
        temporary = preview / 'delta.glb'
        export_reference_motion(preview / 'absolute.glb', assets / reference['skeleton'],
                                assets / reference['motion'], temporary, reference['frame'])
        temporary.replace(args.destination)
        print(f'Exported authored reference-relative motion to {args.destination}')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        if '--blender-stage' in sys.argv:
            import traceback
            traceback.print_exc()
            sys.stderr.flush()
            os._exit(1)
        raise
