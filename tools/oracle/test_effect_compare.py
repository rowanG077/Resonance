"""Failure behavior of the primitive image gate, independent of game assets."""
import unittest
from PIL import Image, ImageDraw
from effect_compare import compare, validate_matte, validate_checker


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

    def test_matching_empty_frames_pass_during_birth_and_expiry(self):
        self.assertTrue(compare(self.background, self.background, self.background, self.roi)['passed'])

    def test_empty_effect_uses_the_same_background_tolerance_as_the_stage(self):
        reference = Image.new('RGB', (80, 60), (32,)*3)
        actual = reference.copy()
        ImageDraw.Draw(reference).rectangle((40, 0, 79, 59), fill=(192,)*3)
        ImageDraw.Draw(actual).rectangle((41, 0, 79, 59), fill=(192,)*3)
        blank = Image.new('RGB', reference.size)
        self.assertTrue(compare(reference, actual, blank, self.roi)['passed'])
        self.assertTrue(compare(reference, actual, reference, self.roi, actual)['passed'])

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

    def test_small_rounding_patch_and_scattered_edges_pass(self):
        actual = self.reference.copy()
        ImageDraw.Draw(actual).rectangle((35, 25, 36, 26), fill=(110, 250, 90))
        for point in [(30, 20), (49, 39), (30, 39)]:
            actual.putpixel(point, (140, 240, 80))
        self.assertTrue(self.measure(actual)['passed'])

    def test_one_pixel_texture_shift_is_not_a_missing_layer(self):
        reference = Image.new('RGB', (80, 60), (64,)*3)
        actual = reference.copy()
        for y in range(20, 24):
            for x in range(30, 34):
                if (x+y) % 2 == 0:
                    reference.putpixel((x, y), (240,)*3)
                    actual.putpixel((x+1, y), (240,)*3)
        self.assertTrue(compare(reference, actual, Image.new('RGB', (80, 60)), self.roi)['passed'])

    def test_empty_frames_tolerate_a_dark_fringe_but_not_a_missing_bright_pixel(self):
        blank = Image.new('RGB', self.reference.size)
        for value, passes in [(4, True), (255, False)]:
            actual = blank.copy()
            actual.putpixel((30, 20), (value,)*3)
            for reference, image in [(blank, actual), (actual, blank)]:
                self.assertEqual(compare(reference, image, blank, self.roi)['passed'], passes)

        reference = blank.copy()
        actual = blank.copy()
        reference.putpixel((30, 20), (255, 80, 10))
        actual.putpixel((30, 20), (80, 80, 10))
        self.assertFalse(compare(reference, actual, blank, self.roi)['passed'])

    def test_rounding_allowance_does_not_hide_a_coherent_dim_layer(self):
        blank = Image.new('RGB', self.reference.size)
        actual = blank.copy()
        ImageDraw.Draw(actual).rectangle((30, 20, 33, 23), fill=(4,)*3)
        self.assertFalse(compare(blank, actual, blank, self.roi)['passed'])

    def test_missing_matte_is_a_fixture_error(self):
        validate_matte(self.background, (0, 231, 0))
        with self.assertRaises(ValueError):
            validate_matte(Image.new('RGB', self.background.size), (0, 231, 0))

    def test_missing_refraction_cannot_pass_from_background_differences(self):
        other = Image.new('RGB', self.background.size, (20, 220, 10))
        self.assertFalse(compare(self.background, other, self.background, self.roi, other)['passed'])

    def test_large_effect_cannot_hide_a_missing_bright_layer(self):
        actual = Image.new('RGB', (640, 480))
        ImageDraw.Draw(actual).rectangle((100, 100, 300, 300), fill=(64, 64, 64))
        for size in (2, 4, 10):
            for start in (190, 191):
                reference = actual.copy()
                ImageDraw.Draw(reference).rectangle((start, start, start+size-1, start+size-1),
                                                    fill=(255, 255, 255))
                for left, right in ((reference, actual), (actual, reference)):
                    result = compare(left, right, Image.new('RGB', actual.size), (0, 0, 640, 400))
                    self.assertFalse(result['passed'])

    def test_checker_requires_the_visible_stage_pattern(self):
        stage = Image.new('RGB', (640, 480))
        draw = ImageDraw.Draw(stage)
        for x in range(7):
            shade = 48 if x % 2 else 208
            draw.rectangle((40+x*80, 40, 119+x*80, 119), fill=(shade,)*3)
        validate_checker(stage)
        for image in [Image.new('RGB', stage.size), Image.new('RGB', stage.size, (128,)*3)]:
            with self.assertRaises(ValueError):
                validate_checker(image)

    def test_missing_checker_is_a_fixture_error(self):
        with self.assertRaises(ValueError):
            validate_checker(Image.new('RGB', (640, 480), (0, 231, 0)))

    def test_mismatched_resolution_is_rejected(self):
        with self.assertRaises(ValueError):
            self.measure(Image.new('RGB', (160, 120)))



if __name__ == '__main__':
    unittest.main()
