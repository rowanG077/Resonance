import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from named_content import Snapshot, same_evidence


class NamedContentTests(unittest.TestCase):
    def test_scene_uses_current_references_and_seals_nested_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "library"
            evidence = Path(directory) / "proof"
            def write(path, value):
                path = root / path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value))
            write("content.json", dict(version=1, game="GQSEAF", revision=0))
            write("battle/scenes/mappings.json", {"214": "rain"})
            write("battle/scenes/rain/scene.json", dict(version=1,
                rules="battle/scenes/rain/rules.json", effects="battle/scenes/rain/effects.json",
                cutin=None, weapon=None))
            write("battle/scenes/rain/rules.json", dict(version=1, projectiles=[], actions={},
                actions_error=None, duration=120, stage_color=None, camera=None, strike_points=[],
                recovery_radius=None, spell_recipe=None))
            write("battle/scenes/rain/effects.json", dict(version=1,
                programs="battle/scenes/rain/programs.json", actors="battle/scenes/rain/actors.json", art=None))
            write("battle/scenes/rain/programs.json", [[]])
            write("battle/scenes/rain/actors.json", [])
            snapshot = Snapshot(root, evidence)
            path = snapshot.scene_path(214)
            self.assertEqual(path, "battle/scenes/rain/scene.json")
            scene, bank = snapshot.scene(path)
            self.assertEqual(scene["duration"], 120)
            self.assertEqual(bank["programs"], [[]])
            self.assertNotIn("source_sha256", scene)
            self.assertNotIn("files", scene)
            original = b"original package bytes"
            proof = snapshot.evidence(path, original)
            self.assertEqual(Path(proof["source"]["path"]).read_bytes(), original)
            self.assertEqual(proof["source"]["sha256"], hashlib.sha256(original).hexdigest())
            self.assertIn("battle/scenes/rain/programs.json", proof["files"])
            write("battle/scenes/rain/programs.json", [])
            self.assertEqual(snapshot.effects(scene["effects"])["programs"], [[]])
            fresh = Snapshot(root, evidence)
            fresh.scene_path(214)
            fresh.scene(path)
            self.assertFalse(same_evidence(proof, fresh.evidence(path, original)))
            self.assertFalse((root / "sources.json").exists())
            self.assertFalse((root / "evidence").exists())

    def test_old_documents_and_escaping_paths_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "library"
            root.mkdir()
            (root / "content.json").write_text('{"version":1,"game":"GQSEAF","revision":0}')
            (root / "old.json").write_text('{"source_sha256":"old","programs":[],"actors":[]}')
            snapshot = Snapshot(root, Path(directory) / "proof")
            with self.assertRaises(ValueError):
                snapshot.effects("old.json")
            for path in ("../outside", "/absolute", "a//b", "a/./b", "a\\b"):
                with self.assertRaises(ValueError):
                    snapshot.read(path)
            with self.assertRaises(ValueError):
                Snapshot(root, root / "proof")


if __name__ == "__main__":
    unittest.main()
