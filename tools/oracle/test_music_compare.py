"""Known in-memory signals verify reporting; no game audio is generated."""
import json
import unittest
import numpy as np
from music_compare import acceptance, alignment, compare, metrics


class MusicComparisonTests(unittest.TestCase):
    def test_registration_keeps_stereo_order_and_reports_gain_and_dc_error(self):
        source = np.random.default_rng(57).normal(0, 1000, (4096, 2))
        actual = np.vstack([np.zeros((37, 2)), source * 0.8 + [120, -240], np.zeros((100, 2))])
        offset, correlation = alignment(source[:1000], actual)
        self.assertEqual(offset, 37)
        self.assertAlmostEqual(correlation, 1)
        result = metrics(source, actual[37:37 + len(source)])
        self.assertAlmostEqual(result['level_difference_db'], 20 * np.log10(0.8))
        self.assertGreater(result['raw_error_rms_pcm16'], result['mean_removed_error_rms_pcm16'])
        self.assertEqual(result['changed_samples'], source.size)

    def test_later_lag_is_diagnosed_without_realigning_reported_errors(self):
        source = np.random.default_rng(93).normal(0, 1000, (8192, 2))
        actual = np.vstack([source[:4096], np.zeros((3, 2)), source[4096:]])
        result = compare(source, actual, 8000, 0, 0, 2048, 1024, 8192, 2048, 8)
        self.assertEqual(result['registration']['actual_start_frame'], 0)
        self.assertEqual(result['windows'][-1]['diagnostic_best_lag_frames'], 3)
        self.assertGreater(result['windows'][-1]['raw_error_rms_pcm16'], 1000)
        self.assertAlmostEqual(result['windows'][-1]['diagnostic_best_correlation'], 1)

    def test_silence_cannot_produce_a_registration(self):
        with self.assertRaises(ValueError):
            alignment(np.zeros((100, 2)), np.zeros((200, 2)))

    def test_acceptance_uses_fixed_alignment_quality_and_rejects_phase_error(self):
        source = np.random.default_rng(57).integers(-2000, 2001, (4096, 2)).astype(float)
        exact = metrics(source, source.copy())
        result = acceptance(exact, [exact])
        self.assertTrue(result['passed'])
        self.assertEqual(result['zero_mean_removed_error_windows'], 1)
        self.assertIsNone(result['minimum_window_mean_removed_signal_to_error_db'])
        json.dumps(result, allow_nan=False)

        small_error = metrics(source, source * 0.999)
        self.assertTrue(acceptance(small_error, [exact, small_error])['passed'])
        shifted = metrics(source, np.roll(source, 1, axis=0))
        self.assertFalse(acceptance(shifted, [shifted])['passed'])
        silence = metrics(np.zeros_like(source), np.zeros_like(source))
        self.assertFalse(acceptance(silence, [silence])['passed'])
        self.assertFalse(acceptance(exact, [exact, silence])['passed'])


if __name__ == '__main__':
    unittest.main()
