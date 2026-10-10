"""Requested observation boundaries; no emulator or private checkpoint required."""
import json
import struct
import unittest
from types import SimpleNamespace

from capture import explicit_locations, field_locations
from state import observations


class CaptureTests(unittest.TestCase):
    def test_default_origin_ignores_unrequested_runtime_internals(self):
        ram = bytearray(0x1800000)
        struct.pack_into(">I", ram, 0x35a768, 0x80010000)
        struct.pack_into(">I", ram, 0x110d0, 332)
        struct.pack_into(">I", ram, 0x35a578, 0x80020000)
        struct.pack_into(">i", ram, 0x20040, 2500)
        # These unrelated, potentially inactive records must not block replay.
        struct.pack_into(">I", ram, 0x35a7e0, 0x802ce560)
        struct.pack_into(">i", ram, 0x35a1e0, 1)
        struct.pack_into(">H", ram, 0x25814, 65535)
        ram[0x35ad7d] = ram[0x35a594] = 255
        observed = observations(ram, field_origin=True)
        self.assertEqual(observed['field']['map_id'], 332)
        self.assertEqual(observed['progress']['story'], 2500)
        self.assertEqual(set(observed), {'mode', 'field', 'progress'})
        self.assertEqual(field_locations(observed)['8035a768 10d0'], 'map_id')
        at = 0x10000 + 0x2b8
        ram[at:at + 5] = b'Lloyd'
        ram[at + 0x10] = 3
        struct.pack_into('>I', ram, at + 0x18, 123)
        struct.pack_into('>H', ram, at + 0x26, 300)
        struct.pack_into('>H', ram, at + 0x2e, 420)
        ram[0x10e9d:0x10ea0] = bytes([1, 3, 2])
        party = observations(ram, party=True)['party_menu']
        self.assertEqual(party['formation'], [1, 3, 2])
        self.assertEqual(len(party['members']), 9)
        member = party['members'][0]
        self.assertEqual((member['id'], member['name'], member['level'], member['experience']),
                         (1, 'Lloyd', 3, 123))
        self.assertEqual(member['base_stats'], [300, 0, 0, 0, 0, 0, 0])
        self.assertEqual(member['luck'], 42)
        self.assertEqual(set(member), {'id', 'name', 'level', 'experience', 'hp', 'tp',
                                     'max_hp', 'max_tp', 'base_stats', 'luck', 'equipment',
                                     'title', 'ex_skills'})
        struct.pack_into(">I", ram, 0x35a768, 0)
        self.assertNotIn('field', observations(ram))
        with self.assertRaisesRegex(ValueError, 'field origin'):
            observations(ram, field_origin=True)
        with self.assertRaisesRegex(ValueError, 'party observation origin'):
            observations(ram, party=True)

    def test_requested_field_discovery_keeps_actor_dialogue_and_particle_watches(self):
        ram = bytearray(0x1800000)
        struct.pack_into(">i", ram, 0x2c7ea0 + 0xb8, 999999)
        struct.pack_into(">I", ram, 0x35a4e4, 0x80020000)
        struct.pack_into(">I", ram, 0x20000, 1)
        struct.pack_into(">i", ram, 0x200b8, 7)
        struct.pack_into(">I", ram, 0x20860, 0xffffffff)
        struct.pack_into(">I", ram, 0x35a3e4, 0x80040000)
        struct.pack_into(">I", ram, 0x35a4fc, 0x80080000)
        self.assertEqual(observations(ram), {'mode': 0})
        observed = observations(ram, actors=True, particles=True)
        self.assertEqual(observed['controlled_actor']['id'], 999999)
        self.assertEqual(observed['actors'], [{'slot': 0, 'id': 7, 'address': 0x80020000}])
        self.assertEqual(observed['dialogue_windows'][2]['address'], 0x80040000 + 2 * 0x13660)
        self.assertEqual(observed['particle_pool_address'], '80080000')
        struct.pack_into(">I", ram, 0x35a4fc, 0)
        with self.assertRaisesRegex(ValueError, 'particle pool'):
            observations(ram, particles=True)

    def test_explicit_names_override_defaults_and_conflicting_requests_fail(self):
        def source(values):
            return SimpleNamespace(read_text=lambda: json.dumps(values))
        defaults = field_locations({'field': {}, 'progress': {}})
        request = source({'8035A768': 'session_pointer'})
        actual = explicit_locations(defaults, [request])
        self.assertEqual(actual['8035a768'], 'session_pointer')
        self.assertEqual(actual['8035a768 10d0'], 'map_id')
        self.assertEqual(defaults['8035a768'], 'field_address')
        with self.assertRaisesRegex(ValueError, 'conflicting watcher location'):
            explicit_locations(defaults, [request, source({'8035a768': 'other'})])


if __name__ == '__main__':
    unittest.main()
