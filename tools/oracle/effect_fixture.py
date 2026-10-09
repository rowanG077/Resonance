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

EFFECT_START = 0x1ff0
INTERACTION_START = 0x1ff4
SCRIPT_BYTES = 2048
FOREGROUND_SCRIPT = 2
BACKGROUND_SCRIPT = 4
SCENE_SETUP = [[0x0a, [0]], [0x0a, [1]], [0x0a, [2]],
               [0xb2, [1, 0]], [0x63, [0, 0]], [0x9b, [1, -1, 12, 0, 2]],
               [0xb2, [90020, 0]]] + [[0x1d, [actor, 12, 1]] for actor in [999996, 999997, 999998]]


def command_words(op, args, *, compact=False):
    if op == 'clock':
        return [0x1200, EFFECT_START, 0x0200, 1, 0, 0x3010, 0x3000]
    if op == 'copy':
        return [0x1200, args[0], 0x1200, args[1], 0x3010, 0x3000]
    words = [0x1200, 0x20] if op == 0x98 else []
    for value in args:
        words.extend(([0x1200, value['variable']] if isinstance(value, dict) else
                      [0x1200, 0x20] if value is None else
                      [0x0100, value & 65535] if compact and -32768 <= value <= 32767 else
                      [0x0200, value & 65535, (value >> 16) & 65535]) + [0x3000, 0x4000])
    words.append(0x2000 | op)
    if op == 0x98:
        words.extend([0x3010, 0x3000])
    return words


def interaction_words():
    return command_words('copy', [INTERACTION_START, EFFECT_START]) + [0x20ff]


def fixture_program(actors, commands, isolated=True, interactions=()):
    # The player is stored separately from the inspected scene-actor list.
    setup = [(0x1D, [1, 12, 1]), (0x1D, [1, 11, 1])]
    # Hide actors and stop the two naturally active teleporter emitters.
    for actor in actors:
        if actor == 0:
            continue
        setup += ([(0x12, [actor])] if isolated and actor != 90020 else
                  [(0x1D, [actor, 12, 1]), (0x1D, [actor, 11, 1])])
    setup += [(0x12, [10000]), (0x12, [10001])]
    for compact in (False, True):
        words = [word for op, args in [*setup, *commands]
                 for word in command_words(op, args, compact=compact)] + [0x20FF]
        if interactions:
            words += interaction_words()
        if len(words) * 2 < SCRIPT_BYTES:
            return words
    raise ValueError('fixture script exceeds reserved space')


def prepare(source, target, observation, sprites, commands=(), seed=1, *, matte=None, interactions=()):
    words = fixture_program([actor['id'] for actor in observation['actors']], commands, seed is not None, interactions)
    state = source.fork()
    put, read = state.write, state.read
    globals_ = read(0x8035A578)
    script = read(globals_ + 0x5820)
    for i, word in enumerate(words):
        put(script + i*2, 'H', word)
    for i in range(32):
        put(globals_ + 0x581C + i*0x360 + 2, 'B', 0)
    # The timed driver must leave foreground execution available to interactions.
    slot = globals_ + 0x581C + (0x360 if interactions else 0)
    for i in range(0, 0x360, 4):
        put(slot+i, 'I', 0)
    kind = BACKGROUND_SCRIPT if interactions else FOREGROUND_SCRIPT
    for offset, fmt, value in [(2, 'B', 1), (4, 'I', script), (0x346, 'H', kind)]:
        put(slot+offset, fmt, value)
    for base in [0x802BF4C0, 0x802BDBA0, 0x802BEC60, 0x802BE400]:
        put(base + 0x100, 'I', 0)
    put(0x8035A460, 'I', 0)
    pool = read(SPRITE_POOL)
    for i in range(2048):
        put(pool+i*SPRITE_STRIDE, 'h', -1)
    sprites = ([] if matte is None else [matte]) + sprites
    for i, sprite in enumerate(sprites):
        at = pool+i*SPRITE_STRIDE
        for offset in range(0, SPRITE_STRIDE, 4):
            put(at+offset, 'I', 0)
        flags = (0x8000 if sprite['world_space'] else 0xC000) | 0x100 | (sprite['blend'] << 10)
        if matte is not None and i == 0:
            flags = 0x8000 | (sprite['blend'] << 10)
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
        # Stage scenery retains its UVs with animation disabled.
        put(at+88, 'h', sprite['rgba'][3]*16)
        if matte is not None and i == 0:
            # Persistent matte pose observed in the field's live particle data.
            recipe = script + SCRIPT_BYTES
            for j, value in enumerate([1, 1, 0, 1, *sprite['uv_bytes'][:2], 127, 255, 0, 0, 0, 0]):
                put(recipe+j, 'B', value)
            put(at+48, 'I', recipe)
            put(at+52, 'I', 0x01000001)
            put(at+92, 'I', 0x02000000)
    models = read(MODEL_POOL)
    for i in range(12):
        put(models+i*MODEL_STRIDE, 'h', -1)
    if seed is not None:
        # The synthetic native program has no room events. Ring contacts must
        # not start the saved room's callbacks in the isolated source stage.
        for entry in range(read(globals_ + 0x5814, 'H')):
            put(globals_ + 0x2160 + entry*12, 'I', 0xFFFFFFFF)
            put(globals_ + 0x2164 + entry*12, 'I', 0x7FFFFFFF)
        put(globals_ + 0x5814, 'H', 0)
        if interactions:
            for entry, actor in enumerate(interactions):
                for word, value in enumerate([0, actor, len(words)-len(interaction_words())]):
                    put(globals_ + 0x2160 + entry*12 + word*4, 'I', value)
            put(globals_ + 0x5814, 'H', len(interactions))
            put(globals_ + INTERACTION_START, 'I', 0)
        put(globals_ + EFFECT_START, 'I', 0)
        put(0x8035A340, 'I', seed)
        # Keep the retained player/camera actors in their current idle pose.
        # Their decision timers must not consume the effect's random stream.
        for actor in [0x802C7EA0, read(0x8035A4E4)]:
            put(actor + 0xB0, 'i', 0x7FFFFFFF)
    identity = [sprites, commands, seed] + ([list(interactions)] if interactions else [])
    marker = int.from_bytes(hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).digest()[:4])
    marker_address = globals_ + 0x581C + 31*0x360 + 0x300
    put(marker_address, 'I', marker)
    state.save(target)
    shutil.copyfile(str(source.path)+'.dtm', str(target)+'.dtm')
    return {'source_sha256': source.sha256, 'fixture_sha256': digest(target),
            'game_code_modified': False, 'ram_changes': state.changes,
            'marker_address': marker_address, 'case_marker': marker}
