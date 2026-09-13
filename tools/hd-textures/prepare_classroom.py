#!/usr/bin/env python3
"""Match a Dolphin texture pack to the cooked classroom, without changing originals.

Requires Pillow (DDS decoding) and libxxhash. Extract the .7z before running.
Dolphin's tex1 names hash the tiled level-zero GX bytes, plus the used palette
range for indexed textures: Source/Core/VideoCommon/TextureInfo.cpp.
"""
import argparse
import ctypes
import ctypes.util
import glob
import hashlib
import json
from pathlib import Path
import struct

from PIL import Image

BLOCKS = {0: (8, 8, 32), 1: (8, 4, 32), 2: (8, 4, 32), 3: (4, 4, 32),
          4: (4, 4, 32), 5: (4, 4, 32), 6: (4, 4, 64), 8: (8, 8, 32),
          9: (8, 4, 32), 10: (4, 4, 32), 14: (8, 8, 32)}


def hasher(library):
    library = library or ctypes.util.find_library("xxhash")
    if not library:
        library = next(iter(glob.glob('/nix/store/*-xxhash-*/lib/libxxhash.so')), None)
    if not library:
        raise RuntimeError("Install libxxhash or pass --xxhash-library PATH")
    xxhash = ctypes.CDLL(library)
    xxhash.XXH64.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint64]
    xxhash.XXH64.restype = ctypes.c_uint64
    return lambda data: f'{xxhash.XXH64(data, len(data), 0):016x}'


def texture_names(tpl, digest):
    def u32(offset):
        return struct.unpack_from('>I', tpl, offset)[0]

    if u32(0) != 0x0020AF30:
        raise ValueError('Expected a cooked TPL intermediate')
    for index in range(u32(4)):
        entry = u32(8) + index * 8
        header, palette = u32(entry), u32(entry + 4)
        height, width, fmt, offset = struct.unpack_from('>HHII', tpl, header)
        bw, bh, size = BLOCKS[fmt]
        length = ((width + bw - 1) // bw) * ((height + bh - 1) // bh) * size
        data = tpl[offset:offset + length]
        if len(data) != length:
            raise ValueError('Truncated TPL texture')
        palette_hash = ''
        if fmt in (8, 9, 10):
            if not palette:
                raise ValueError('Indexed texture without palette')
            if fmt == 8:
                indices = [v for byte in data for v in (byte & 15, byte >> 4)]
            elif fmt == 9:
                indices = data
            else:
                indices = [value[0] & 0x3fff for value in struct.iter_unpack('>H', data)]
            start = u32(palette + 8) + 2 * min(indices)
            length = 2 * (max(indices) - min(indices) + 1)
            colors = tpl[start:start + length]
            if len(colors) != length:
                raise ValueError('Truncated TPL palette')
            palette_hash = '_' + digest(colors)
        yield f'tex1_{width}x{height}_{digest(data)}{palette_hash}_{fmt}', (width, height)


def prepare(cooked, pack, digest, extracted=None):
    manifest = json.loads((cooked / 'fields/iselia-classroom.json').read_text())
    parts = manifest['parts'] + [part for actor in manifest['actors'] for part in actor['parts']]
    inventory = []
    for part in parts:
        tpl = cooked / 'intermediate' / Path(part['mesh']).parent / 'model.tpl'
        names = list(texture_names(tpl.read_bytes(), digest))
        if len(names) != len(part['textures']):
            raise ValueError(f'Texture inventory mismatch: {tpl}')
        inventory.extend(zip(part['textures'], names))
    if extracted is not None:
        from prepare_ui import inventory as ui_inventory
        inventory.extend(ui_inventory(cooked, extracted, texture_names, digest))
    requested = {name for _, (name, _) in inventory}
    candidates = {}
    for file in sorted(pack.rglob('*')):
        if file.suffix.lower() not in ('.dds', '.png') or '_old' in file.parts:
            continue
        key = file.stem.removesuffix('_arb')
        if key not in requested:
            continue
        if key in candidates:
            if hashlib.sha256(file.read_bytes()).digest() != hashlib.sha256(candidates[key].read_bytes()).digest():
                raise ValueError(f'Ambiguous replacement: {key}')
        candidates[key] = file
    output = cooked / 'overrides/classroom'
    output.mkdir(parents=True, exist_ok=True)
    textures, matches, missing = {}, {}, []
    for original, (name, size) in inventory:
        source = candidates.get(name)
        if source is None:
            missing.append({'original': original, 'dolphin_name': name})
            continue
        destination = output / (name + '.png')
        if name not in matches:
            with Image.open(source) as image:
                width, height = image.size
                if width < size[0] or height < size[1] or width * size[1] != height * size[0]:
                    raise ValueError(f'Replacement changes atlas aspect/size: {source}')
                image.convert('RGBA').save(destination)
            matches[name] = {'source': str(source.relative_to(pack)), 'original_size': size,
                             'replacement_size': [width, height],
                             'sha256': hashlib.sha256(source.read_bytes()).hexdigest()}
        textures[original] = destination.relative_to(cooked).as_posix()
    result = {'textures': textures, 'matches': matches, 'missing': missing}
    if extracted is not None:
        from prepare_ui import build_font
        original, replacement, report = build_font(cooked, extracted, pack, digest)
        textures[original] = replacement
        result['font'] = report
        print(f"Dialogue font: {len(report['matches'])} supplied HD glyphs")
    (cooked / 'overrides/classroom.json').write_text(json.dumps(result, indent=2) + '\n')
    print(f'{len(textures)} bindings, {len(matches)} unique replacement textures, {len(missing)} missing')
    if missing:
        print(json.dumps(missing, indent=2))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cooked', type=Path, default=Path('local/cooked'))
    parser.add_argument('--pack', type=Path, default=Path('local/hd-textures/GQS'))
    parser.add_argument('--xxhash-library')
    parser.add_argument('--extracted', type=Path, help='extracted disc directory; also match dialogue UI and repack HD glyphs')
    args = parser.parse_args()
    if not args.pack.is_dir():
        parser.error('Extract the texture pack first')
    prepare(args.cooked, args.pack, hasher(args.xxhash_library), args.extracted)


if __name__ == '__main__':
    main()
