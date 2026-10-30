"""Original-to-current effect bindings for observations, never cooked metadata.

Only the importer decodes original declarations. Matching here is a test-time
comparison of values; the game resolves local names without inspecting sources.
"""
import copy
from functools import lru_cache
import json
import os
from pathlib import Path
import subprocess
import effect_defaults


@lru_cache(maxsize=128)
def original_templates(raw):
    importer = os.environ.get('RESONANCE_IMPORT', str(Path(__file__).resolve().parents[2]
        / 'target/release/resonance-import'))
    result = subprocess.run([importer, 'inspect-effect-templates'], input=raw,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    return effect_defaults.templates(json.loads(result.stdout))


def comparable(declaration):
    declaration = copy.deepcopy(declaration)
    if declaration['kind'] == 'particle':
        # Atlas publication binds this local selector table to actual images.
        declaration['visual']['texture'].pop('palette_set', None)
    return declaration


def members(raw, current):
    if not current:
        return []
    source = [comparable(value) for value in original_templates(bytes(raw))]
    result = []
    for value in current:
        expected = comparable(value)
        matches = [index for index, original in enumerate(source) if original == expected]
        if not matches:
            raise ValueError('edited effect template has no equivalent original declaration')
        result.append(matches)
    return result


def bind(raw, current, declarations):
    indices = members(raw, current)
    aliases = [list(dict.fromkeys(declarations[index] for index in matches)) for matches in indices]
    return [matches[0] for matches in aliases], aliases
