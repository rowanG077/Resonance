"""Copied-state renderer fixtures for the pinned GQSEAF field profile.

Only live field data is edited. Texture handles and pool layouts were observed
in Dolphin captures; this tool neither reads nor patches executable code.
"""
import hashlib
import json
import shutil
from state import digest

SPRITE_POOL = 0x8035A4FC
MODEL_POOL = 0x8035A4F8
SPRITE_STRIDE = 0x6C
MODEL_STRIDE = 0x25C
# Live GX texture objects observed in the field renderer.
TEXTURES = {0: 0x802BFF84, 2: 0x802BFF64, 3: 0x802BFF04,
            4: 0x802BFF44, 5: 0x802BFEE4}


def prepare(source, target, observation, sprites, model=None):
    state = source.fork()
    put, read = state.write, state.read
    globals_ = read(0x8035A578)
    script = read(globals_ + 0x5820)
    words = []
    def command(op, args):
        for v in args:
            words.extend([0x0200, v & 65535, (v >> 16) & 65535, 0x3000, 0x4000])
        words.append(0x2000 | op)
    # The player is stored separately from the inspected scene-actor list.
    command(0x1D, [1, 12, 1])
    command(0x1D, [1, 11, 1])
    # Hide actors and stop the two naturally active teleporter emitters.
    for actor in observation['actors']:
        if actor['id'] == 0:
            continue
        command(0x1D, [actor['id'], 12, 1])
        command(0x1D, [actor['id'], 11, 1])
    for actor in [10000, 10001]:
        command(0x12, [actor])
    if model:
        handle = read(MODEL_POOL)
        for prop in [423, 424, 425, 429, 430, 431, 435, 436, 437]:
            command(0xDE, [handle, prop, 0])
        command(0xDE, [handle, 447, model['blend']])
        command(0xDE, [handle, 441, model['rgba'][3]])
        command(0xDE, [handle, 449, 0])
    words.append(0x20FF)
    if len(words) * 2 >= 2048:
        raise ValueError('fixture script exceeds reserved space')
    for i, word in enumerate(words):
        put(script + i*2, 'H', word)
    for i in range(32):
        put(globals_ + 0x581C + i*0x360 + 2, 'B', 0)
    slot = globals_ + 0x581C
    for i in range(0, 0x360, 4):
        put(slot+i, 'I', 0)
    for offset, fmt, value in [(2, 'B', 1), (4, 'I', script), (0x346, 'H', 2)]:
        put(slot+offset, fmt, value)
    for base in [0x802BF4C0, 0x802BDBA0, 0x802BEC60, 0x802BE400]:
        put(base + 0x100, 'I', 0)
    put(0x8035A460, 'I', 0)
    pool = read(SPRITE_POOL)
    for i in range(2048):
        put(pool+i*SPRITE_STRIDE, 'h', -1)
    for i, sprite in enumerate(sprites):
        at = pool+i*SPRITE_STRIDE
        for offset in range(0, SPRITE_STRIDE, 4):
            put(at+offset, 'I', 0)
        flags = (0x8000 if sprite['world_space'] else 0xC000) | 0x100 | (sprite['blend'] << 10)
        if i == 0:
            flags = 0x8400
        if sprite.get('refraction'):
            flags |= 0x2000
        put(at, 'h', 30000)
        put(at+2, 'H', flags)
        for offset, values in [(4, sprite['position']), (16, sprite['rotation']), (40, sprite['size'])]:
            for j, value in enumerate(values):
                put(at+offset+j*4, 'f', value)
        for offset, values in [(28, sprite['uv_bytes']), (32, sprite['rgba'])]:
            for j, value in enumerate(values):
                put(at+offset+j, 'B', value)
        put(at+36, 'I', TEXTURES[sprite['texture']])
        # Ordinary probes retain their sampled UVs with animation disabled.
        put(at+88, 'h', sprite['rgba'][3]*16)
        if i == 0:
            # Persistent matte pose observed in the field's live particle data.
            recipe = script + 2048
            for j, value in enumerate([1, 1, 0, 1, 185, 31, 127, 255, 0, 0, 0, 0]):
                put(recipe+j, 'B', value)
            put(at+48, 'I', recipe)
            put(at+52, 'I', 0x01000001)
            put(at+92, 'I', 0x02000000)
    models = read(MODEL_POOL)
    for i in range(12):
        if i or model is None:
            put(models+i*MODEL_STRIDE, 'h', -1)
    if model:
        if read(models, 'h') < 0:
            raise ValueError('fixture needs a live model particle in slot zero')
        put(models, 'h', 32767)
        # The pool updates before the isolation script's first turn.
        # Stop motion now, so that turn cannot rotate or translate the probe.
        for offset in [0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c, 0x70, 0x74, 0x78]:
            put(models+offset, 'f', 0.)
        # Freeze the retained ring; its resource and child transforms stay intact.
        for offset, values in [(4, model['position']), (16, model['rotation']), (100, model['scale'])]:
            for j, value in enumerate(values):
                put(models+offset+j*4, 'f', value)
        for j, value in enumerate(model['rgba']):
            put(models+32+j, 'B', value)
    marker = int.from_bytes(hashlib.sha256(json.dumps([sprites, model], sort_keys=True).encode()).digest()[:4])
    marker_address = globals_ + 0x581C + 31*0x360 + 0x300
    put(marker_address, 'I', marker)
    state.save(target)
    shutil.copyfile(str(source.path)+'.dtm', str(target)+'.dtm')
    return {'source_sha256': source.sha256, 'fixture_sha256': digest(target),
            'game_code_modified': False, 'ram_changes': state.changes,
            'marker_address': marker_address, 'case_marker': marker}
