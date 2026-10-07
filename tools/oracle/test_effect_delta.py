"""Failure behavior of the primitive image gate, independent of game assets."""
import unittest
from PIL import Image, ImageDraw
from effect_delta import compare, validate_matte, validate_checker, random_cases


class EffectDeltaTests(unittest.TestCase):
    def setUp(self):
        self.background = Image.new('RGB', (80, 60), (0, 231, 0))
        self.reference = self.background.copy()
        ImageDraw.Draw(self.reference).rectangle((30, 20, 49, 39), fill=(100, 240, 80))
        self.roi = [0, 0, 80, 60]

    def measure(self, image):
        return compare(self.reference, image, self.background, self.roi)

    def test_identical_visible_effect_passes(self):
        self.assertTrue(self.measure(self.reference)['passed'])

    def test_empty_pair_cannot_pass(self):
        self.assertFalse(compare(self.background, self.background, self.background, self.roi)['passed'])

    def test_missing_effect_fails(self):
        self.assertFalse(self.measure(self.background)['passed'])

    def test_geometry_drift_fails_even_on_a_large_blank_background(self):
        actual = self.background.copy()
        ImageDraw.Draw(actual).rectangle((27, 20, 52, 39), fill=(100, 240, 80))
        delta = self.measure(actual)
        self.assertEqual(delta['bounds_delta'], 3)
        self.assertFalse(delta['passed'])

    def test_color_drift_fails_without_gain_fitting(self):
        actual = self.background.copy()
        ImageDraw.Draw(actual).rectangle((30, 20, 49, 39), fill=(60, 240, 40))
        self.assertFalse(self.measure(actual)['passed'])

    def test_tolerated_rounding_fringe_is_not_geometry_drift(self):
        actual = self.reference.copy()
        actual.putpixel((5, 5), (3, 231, 0))
        self.assertTrue(self.measure(actual)['passed'])

    def test_missing_matte_is_a_fixture_error(self):
        validate_matte(self.background, (0, 231, 0))
        with self.assertRaises(ValueError):
            validate_matte(Image.new('RGB', self.background.size), (0, 231, 0))

    def test_missing_refraction_cannot_pass_from_background_differences(self):
        other = Image.new('RGB', self.background.size, (20, 220, 10))
        self.assertFalse(compare(self.background, other, self.background, self.roi, other)['passed'])

    def test_missing_checker_is_a_fixture_error(self):
        with self.assertRaises(ValueError):
            validate_checker(Image.new('RGB', (640, 480), (0, 231, 0)))

    def test_mismatched_resolution_is_rejected(self):
        with self.assertRaises(ValueError):
            self.measure(Image.new('RGB', (160, 120)))

    def test_seed_replays_combinations_and_covers_each_primitive(self):
        camera = {'position': [700, -324, 736], 'target': [0, 97, 87]}
        catalogue = {'sprites': {str(k): {'texture': {'effect': 2}, 'uv': [0, 0, .25, .25]}
                                for k in [0, 4, 42, 52]}}
        first = list(random_cases(camera, catalogue, 17, 12))
        self.assertEqual(first, list(random_cases(camera, catalogue, 17, 12)))
        self.assertNotEqual(first, list(random_cases(camera, catalogue, 18, 12)))
        effects = [p for _, _, probes, _ in first for p in probes[48:]]
        self.assertEqual({p['shape']['kind'] for p in effects}, {'sprite', 'leaf', 'model', 'refraction'})
        self.assertTrue(any(len(probes) > 49 for _, _, probes, _ in first))


if __name__ == '__main__':
    unittest.main()
