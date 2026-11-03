import struct
import unittest

from source_enemy import expand


def resource(method, payload, size):
    return struct.pack('<BII', method, len(payload), size) + payload


class SourcePackageTests(unittest.TestCase):
    def test_stored_and_overlapping_native_reference(self):
        self.assertEqual(expand(resource(0, b'abc', 3)), b'abc')
        self.assertEqual(expand(resource(1, bytes([1, 65, 0xee, 0xf2]), 6)), b'AAAAAA')

    def test_variant_three_repeated_byte_forms(self):
        self.assertEqual(expand(resource(3, bytes([0, 0, 15, 7]), 19)), bytes([7]) * 19)
        self.assertEqual(expand(resource(3, bytes([0, 9, 31]), 4)), bytes([9]) * 4)

    def test_uninitialized_or_overrunning_reference_is_rejected(self):
        with self.assertRaises(ValueError):
            expand(resource(1, bytes([0, 255, 255]), 18))
        with self.assertRaises(ValueError):
            expand(resource(3, bytes([0, 0, 15, 7]), 18))

    def test_truncated_token_is_rejected(self):
        with self.assertRaises(ValueError):
            expand(resource(1, bytes([0, 0]), 3))
