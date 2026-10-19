"""Identify initialized enemy data without relying on formation expectations."""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import struct

PROFILE_PREFIX_BYTES = 160
DISPLAY_NAME_BYTES = 24
RAM_BASE = 0x80000000
PROFILE_REWARDS_OFFSET = 0x40
ACTION_TABLE_POINTER = 0x50
ACTION_TABLE_HEADER_FIELD = 10


class EnemyCatalogue:
    def __init__(self, packages, assets):
        self.signatures = defaultdict(list)
        self.variants = defaultdict(list)
        self.records = []
        for monster in sorted((Path(assets) / 'monsters').glob('[0-9][0-9][0-9].json')):
            identity = json.loads(monster.read_text())
            package = Path(packages) / (monster.stem + '.bin')
            with package.open('rb') as stream:
                header = stream.read(8)
                if len(header) != 8 or header[:4] != b'em8\0':
                    raise ValueError(f'invalid initialized enemy package: {package}')
                offset = struct.unpack_from('>H', header, 4)[0]
                stream.seek(offset)
                profile = stream.read(PROFILE_PREFIX_BYTES)
            if len(profile) != PROFILE_PREFIX_BYTES:
                raise ValueError(f'truncated initialized profile: {package}')
            base_rewards = identity['statistics'][0]
            if struct.unpack_from('>2I', profile, PROFILE_REWARDS_OFFSET) != (
                    base_rewards['experience'], base_rewards['gald']):
                raise ValueError(f'cooked enemy rewards differ from package: {package}')
            for variant, stats in enumerate(identity['statistics']):
                # Encounter variants replace these two typed reward fields.
                initialized = bytearray(profile)
                struct.pack_into('>2I', initialized, PROFILE_REWARDS_OFFSET,
                                 stats['experience'], stats['gald'])
                signature = hashlib.sha256(initialized).hexdigest()
                if identity['id'] not in self.signatures[signature]:
                    self.signatures[signature].append(identity['id'])
                self.variants[signature].append(dict(enemy=identity['id'], variant=variant))
                self.records.append(dict(enemy=identity['id'], variant=variant, name=identity['name'],
                                         profile_sha256=signature, package=str(package),
                                         profile_offset=offset))
        if not self.records:
            raise ValueError('enemy catalogue is empty')

    def identify(self, state, actor):
        # These data pointers are also used by the maintained battle observer.
        profile = state.read(actor['address'] + 4)
        profile_offset = profile - RAM_BASE
        name_offset = actor['vitals_address'] - RAM_BASE + 4
        if not (0 <= profile_offset <= len(state.ram) - PROFILE_PREFIX_BYTES and
                0 <= name_offset <= len(state.ram) - DISPLAY_NAME_BYTES):
            raise ValueError('enemy identity data lies outside main RAM')
        profile_bytes = bytes(state.ram[profile_offset:profile_offset + PROFILE_PREFIX_BYTES])
        signature = hashlib.sha256(profile_bytes).hexdigest()
        name = bytes(state.ram[name_offset:name_offset + DISPLAY_NAME_BYTES])
        candidates = self.signatures.get(signature, [])
        bindings = []
        if len(candidates) > 1:
            from battle_fixture import enemy_package
            for enemy in candidates:
                record = next(record for record in self.records
                              if record['enemy'] == enemy and record['profile_sha256'] == signature)
                raw = Path(record['package']).read_bytes()
                try:
                    address, proof = enemy_package(state, raw)
                except ValueError:
                    continue
                table = address + struct.unpack_from('>H', raw, ACTION_TABLE_HEADER_FIELD)[0]
                if state.read(actor['address'] + ACTION_TABLE_POINTER) == table:
                    bindings.append(dict(enemy=enemy, action_table=table, package=proof))
            candidates = [binding['enemy'] for binding in bindings]
        # Hidden names contain question marks; initialized data decides identity.
        return dict(slot=actor['slot'], name=name.split(b'\0', 1)[0].decode('shift_jis', errors='replace'),
                    profile_address=profile, profile_sha256=signature,
                    profile_bytes=profile_bytes.hex(),
                    variant_candidates=self.variants.get(signature, []),
                    candidates=candidates, package_bindings=bindings,
                    enemy=candidates[0] if len(candidates) == 1 else None)
