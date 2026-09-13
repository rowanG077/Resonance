"""Repack the supplied Dolphin UI/font replacements into the cooked atlas layout."""
import hashlib
import glob
import json
import shutil
import struct
import subprocess

from PIL import Image


def dol_slice(data, address, length):
    for i in range(18):
        offset = struct.unpack_from('>I', data, i * 4)[0]
        base = struct.unpack_from('>I', data, 0x48 + i * 4)[0]
        size = struct.unpack_from('>I', data, 0x90 + i * 4)[0]
        if base <= address and address + length <= base + size:
            return data[offset + address - base:offset + address - base + length]
    raise ValueError(f'Unmapped executable address: {address:#x}')


def inventory(cooked, extracted, texture_names, digest):
    art = json.loads((cooked / 'ui/dialogue.json').read_text())
    tpl = (extracted / 'files/system.tpl').read_bytes()
    if hashlib.sha256(tpl).hexdigest() != art['source_sha256']:
        raise ValueError('UI source does not match the cooked game revision')
    names = list(texture_names(tpl, digest))
    if len(names) != len(art['textures']):
        raise ValueError('UI texture inventory mismatch')
    result = list(zip((t['path'] for t in art['textures']), names))
    # The common emote atlas is entry 1 of EFFECT.TPL inside effect.cab.
    effects = json.loads((cooked / 'effects/field.json').read_text())
    seven_zip = shutil.which('7z') or shutil.which('7zz')
    if seven_zip is None:
        seven_zip = next(iter(glob.glob('/nix/store/*p7zip-*/bin/7z')), None)
    if seven_zip is None:
        raise RuntimeError('Install 7-Zip to unpack the original emote atlas')
    effect_tpl = subprocess.run([seven_zip, 'x', '-so', str(extracted / 'files/effect.cab'), 'EFFECT.TPL'],
                                check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout
    if len(effect_tpl) > 16 * 1024 * 1024:
        raise ValueError('Effect atlas exceeds the cooker size limit')
    effect_names = list(texture_names(effect_tpl, digest))
    result.append((effects['emote_texture'], effect_names[1]))
    return result


def glyph_name(character, source, mapping, palette_hash, digest):
    if 32 <= ord(character) < 127:
        offset = (ord(character) - 32) * 2
        code = 0x81a7 if character == '^' else int.from_bytes(mapping[offset:offset + 2], 'big')
    else:
        try:
            code = int.from_bytes(character.encode('shift_jis'), 'big')
        except UnicodeEncodeError:
            code = 0xffff
    if 0x8140 <= code < 0x8440 and (code & 255) >= 0x40:
        base = ((code >> 8) - 0x81) * 0x6c00 + (((code & 0xf0) - 0x40) >> 4) * 0x900 + (code & 15) * 6
    else:
        base = 0x7e3c
    pixels = [(source[base + y * 96 + x // 4] >> (6 - (x % 4) * 2)) & 3
              for y in range(24) for x in range(24)]
    # The game expands its two-bit font into a tiled GX C4 texture per glyph.
    tiled = bytes((pixels[(by + y) * 24 + bx + x] << 4) | pixels[(by + y) * 24 + bx + x + 1]
                  for by in range(0, 24, 8) for bx in range(0, 24, 8)
                  for y in range(8) for x in range(0, 8, 2))
    return f'tex1_24x24_{digest(tiled)}_{palette_hash}_8'


def build_font(cooked, extracted, pack, digest):
    font = json.loads((cooked / 'fonts/dialogue.json').read_text())
    source = (extracted / 'files/u_f_fontb0.dat').read_bytes()
    executable = (extracted / 'sys/main.dol').read_bytes()
    if (hashlib.sha256(source).hexdigest() != font['source_sha256'] or
            hashlib.sha256(executable).hexdigest() != font['executable_sha256']):
        raise ValueError('Font source does not match the cooked game revision')
    mapping = dol_slice(executable, 0x801f8984, 192)
    palette_hash = digest(dol_slice(executable, 0x801f88a0, 8))
    candidates = {}
    # GQSEAF is the US release. The pack also contains different Japanese and
    # prototype typefaces under identical glyph hashes; do not mix those.
    for path in sorted((pack / 'UI/Font/NTSC-US').glob('*.dds')):
        key = path.stem.removesuffix('_arb')
        if key in candidates and path.read_bytes() != candidates[key].read_bytes():
            raise ValueError(f'Ambiguous HD glyph: {key}')
        candidates[key] = path
    replacements, missing, seen = [], [], set()
    for character, glyph in font['glyphs'].items():
        rect = tuple(glyph['rect'])
        if rect in seen:
            continue
        seen.add(rect)
        name = glyph_name(character, source, mapping, palette_hash, digest)
        path = candidates.get(name)
        if path is None:
            if not character.isspace():
                missing.append(character)
        else:
            with Image.open(path) as image:
                if image.width != image.height or image.width % 24 or image.width <= 24:
                    raise ValueError(f'Invalid HD glyph size: {path}')
                replacements.append((character, rect, path, image.width // 24))
    if not replacements:
        raise ValueError('No matching HD dialogue glyphs')
    scale = max(r[3] for r in replacements)
    # Retain unsupported glyphs and the solid white texel in the atlas gutter.
    # Only actual supplied glyph art is reported as an HD replacement.
    with Image.open(cooked / 'intermediate/fonts/dialogue.png') as original:
        atlas = original.convert('RGBA').resize((font['width'] * scale, font['height'] * scale), Image.Resampling.NEAREST)
    matches = []
    for character, (x, y, width, height), path, glyph_scale in replacements:
        with Image.open(path) as image:
            image = image.convert('RGBA')
            if glyph_scale != scale:
                image = image.resize((width * scale, height * scale), Image.Resampling.NEAREST)
            atlas.paste(image, (x * scale, y * scale))
        matches.append({'character': character, 'source': str(path.relative_to(pack)), 'scale': glyph_scale,
                        'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    destination = cooked / 'overrides/classroom/dialogue-font.png'
    atlas.save(destination)
    return font['texture'], destination.relative_to(cooked).as_posix(), {
        'original_size': [font['width'], font['height']], 'replacement_size': list(atlas.size),
        'matches': matches, 'missing_glyphs': missing,
    }
