"""Regression checks for publishing the authored atlas; no rasterizer needed."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import build_input_prompts as builder


class AtlasBuildTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.assets = root / 'resources/input-prompts'
        shutil.copytree(builder.OUTPUT, self.assets)
        constants = patch.multiple(builder, ROOT=root, OUTPUT=self.assets,
                                   SOURCES=self.assets / 'source')
        constants.start()
        self.addCleanup(constants.stop)
        self.before = {name: (self.assets / name).read_bytes()
                       for name in ['atlas.svg', 'atlas.json', 'atlas.ktx2']}
        # A new tile moves existing rectangles without changing atlas dimensions.
        for state in ['', 'pressed/']:
            shutil.copy(self.assets / f'source/{state}key_a.svg',
                        self.assets / f'source/{state}key_aa.svg')

    def test_svg_only_leaves_runtime_texture_and_layout_unchanged(self):
        with patch.object(builder.subprocess, 'run') as run:
            builder.main(['--svg-only'])
            run.assert_not_called()
        for name in ['atlas.json', 'atlas.ktx2']:
            self.assertEqual((self.assets / name).read_bytes(), self.before[name])
        self.assertNotEqual((self.assets / 'atlas.svg').read_bytes(), self.before['atlas.svg'])

    def test_failed_rasterization_preserves_all_existing_outputs(self):
        with patch.object(builder.subprocess, 'run',
                          side_effect=subprocess.CalledProcessError(1, 'magick')):
            with self.assertRaises(subprocess.CalledProcessError):
                builder.main([])
        self.assert_preserved()

    def test_failed_encoding_preserves_all_existing_outputs(self):
        with patch.object(builder.subprocess, 'run',
                          side_effect=[None, subprocess.CalledProcessError(1, 'encoder')]) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                builder.main([])
            self.assertEqual(run.call_count, 2)
        self.assert_preserved()

    def assert_preserved(self):
        for name, original in self.before.items():
            self.assertEqual((self.assets / name).read_bytes(), original)
        self.assertFalse(list(self.assets.glob('.input-icons-*')))


if __name__ == '__main__':
    unittest.main()
