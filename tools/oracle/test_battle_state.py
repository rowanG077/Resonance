"""Bounds checks for the battle savestate reader; real captures remain in local/."""
import struct
import unittest
from battle_state import inspect_battle, watch_locations


class BattleStateTests(unittest.TestCase):
    def setUp(self):
        self.ram = bytearray(0x1800000)
        self.word(0x8035a86c, 0x80001000)
        self.word(0x8035a518, 0x80002000)
        self.word(0x8035a768, 0x80010000)
        self.word(0x80001000, 1)
        self.word(0x8000100c, 2)
        self.word(0x80001010, 0x80003000)
        self.ram[0x1033] = 1
        self.word(0x80003008, 0x80020000)
        self.word(0x8000300c, 0x1000)
        self.word(0x80020540, 0x80030000)
        self.word(0x80030198, 0x800d0000)

    def word(self, address, value):
        struct.pack_into(">I", self.ram, address - 0x80000000, value)

    def test_watcher_discovers_the_loaded_bss_instead_of_assuming_its_address(self):
        battle = inspect_battle(self.ram)
        self.assertEqual(battle["pool_pointer"], 0x80020540)
        self.assertEqual(watch_locations(battle)["80020540 7a670"], "battle_random_state_word")

    def test_invalid_module_and_bss_are_rejected(self):
        self.word(0x80001000, 2)
        with self.assertRaisesRegex(ValueError, "module 1"):
            inspect_battle(self.ram)
        self.word(0x80001000, 1)
        self.word(0x8000300c, 0x540)
        with self.assertRaisesRegex(ValueError, "BSS"):
            inspect_battle(self.ram)

    def test_invalid_and_duplicate_roster_slots_are_rejected(self):
        self.ram[0x30000 + 0x15948] = 1
        self.ram[0x30000 + 0x1565c] = 4
        with self.assertRaisesRegex(ValueError, "roster slot"):
            inspect_battle(self.ram)
        self.ram[0x30000 + 0x15948] = 2
        self.ram[0x30000 + 0x1565c] = 0
        with self.assertRaisesRegex(ValueError, "roster slot"):
            inspect_battle(self.ram)

    def test_center_watch_distinguishes_the_sample_from_the_root_and_profile(self):
        self.ram[0x30000 + 0x15948] = 1
        actor = 0x80030000 + 0x1cea0
        profile = 0x80010000
        self.word(actor + 4, profile)
        struct.pack_into(">3f", self.ram, actor - 0x80000000 + 0x18c0, 10., 20., 30.)
        struct.pack_into(">3f", self.ram, actor - 0x80000000 + 0x195c, 9., 180., 30.)
        struct.pack_into(">3f", self.ram, profile - 0x80000000 + 0x60, 0., 80., 0.)
        struct.pack_into(">f", self.ram, profile - 0x80000000 + 0x84, 2.)
        battle = inspect_battle(self.ram)
        observed = battle["actors"][0]
        self.assertEqual(observed["center"], [9., 180., 30.])
        self.assertEqual(observed["position"], [10., 20., 30.])
        self.assertEqual(observed["center_offset"], [0., 80., 0.])
        self.assertEqual(observed["model_scale"], 2.)
        self.assertEqual(watch_locations(battle)["80020540 1e7fc"],
                         "battle_actor_0_center_x_bits")


if __name__ == "__main__":
    unittest.main()
