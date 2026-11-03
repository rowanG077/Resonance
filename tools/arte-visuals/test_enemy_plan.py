"""Conservative enemy capture grouping; no renderer or original game required."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

from catalogue import CURRENT_VERSION, fingerprint, visual_structure
import enemy_plan


class EnemyPlanTests(unittest.TestCase):
    def test_same_owner_fixture_and_full_selection_are_required_for_reuse(self):
        row = {"weight": 100, "target_policy": "nearest", "range": [0, 400],
               "requirements": {"hp_percent": 50}, "execution": {"attack": "one"}}
        rules = {"actions": {"policy": {}}, "projectiles": {}, "contacts": {}}
        setup = {"formation": 2, "controller_stage": "initial", "owner_hp_percent": 100}
        target = {"kind": "living_opponent", "policy": "nearest"}
        def signature(member=row, fixture=setup, edition="steam", owner="torent-0000", visual="resolved"):
            return enemy_plan.cohort_signature(edition, owner, visual, member, rules, fixture, target)
        original = signature()
        for field, value in [("weight", 1), ("range", [1, 400]),
                             ("requirements", {"hp_percent": 25}), ("target_policy", "spread"),
                             ("chain_group", 2), ("followup_chance", 20)]:
            self.assertNotEqual(original, signature({**row, field: value}), field)
        self.assertNotEqual(original, signature(fixture={**setup, "controller_stage": "final"}))
        self.assertNotEqual(original, signature(fixture={**setup, "formation": 3}))
        self.assertNotEqual(original, signature(edition="gamecube"))
        self.assertNotEqual(original, signature(owner="torent-0001"))
        self.assertNotEqual(original, signature(visual="different_particle_motion"))
        self.assertIsNone(signature(visual=None))

    def test_existing_visual_filter_ignores_audio_and_hit_power_only(self):
        rules = {"projectiles": {}, "contacts": {}}
        visual = {"events": [[0, {"sound": 4}], [2, {"effect": 3}]],
                  "hit": {"power": 20, "radius": 50}, "duration": 40}
        changed = copy.deepcopy(visual)
        changed["events"][0][1]["sound"] = 99
        changed["hit"]["power"] = 80
        self.assertEqual(fingerprint(visual_structure(visual, rules)),
                         fingerprint(visual_structure(changed, rules)))
        changed["events"][1][1]["effect"] = 4
        self.assertNotEqual(fingerprint(visual_structure(visual, rules)),
                            fingerprint(visual_structure(changed, rules)))

    def test_fixture_requires_active_ally_and_actual_sword_dancer_variant(self):
        formations = {"mappings": {"1": "single", "2": "allied", "3": "final"}, "formations": {
            "single": {"actors": [{"enemy": "sword-dancer"}], "reserves": [{"enemy": "other"}]},
            "allied": {"actors": [{"enemy": "sword-dancer"}, {"enemy": "other"}]},
            "final": {"actors": [{"enemy": "sword-dancer", "statistics_variant": 2}]}}}
        self.assertEqual(enemy_plan.fixture(formations, "sword-dancer", 191, 0, True, {})["formation"], 2)
        self.assertEqual(enemy_plan.fixture(formations, "sword-dancer", 191, 5, False, {})["formation"], 3)
        self.assertIsNone(enemy_plan.fixture(formations, "missing", 0, 0, False, {}))

    def test_build_lists_every_row_and_associates_shared_evidence_without_passing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            requirements = {"difficulty": {"start": 0, "end": 2}}
            rows = [{"weight": 100, "requirements": requirements, "execution": {"attack": "one"}},
                    {"weight": 100, "requirements": requirements, "execution": {"attack": "two"}},
                    {"weight": 50, "requirements": requirements, "execution": {"attack": "two"}}]
            docs = {
                "content.json": {"game": "tales-of-symphonia", "version": CURRENT_VERSION},
                "game/techniques.json": {"mappings": {}, "characters": {}, "learning": {}, "techniques": {}},
                "battle/unison.json": {"mappings": {}, "placements": [], "combinations": []},
                "battle/enemies/mappings.json": {"0": "torent-0000"},
                "battle/enemies/torent-0000/enemy.json": {"rules": {"actions": {"rows": rows},
                    "attacks": {"one": {"duration": 60}, "two": {"duration": 60}}}},
                "battle/formations.json": {"mappings": {"2": "torent"},
                    "formations": {"torent": {"actors": [{"enemy": "torent-0000"}]}}},
            }
            for name, value in docs.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value))
            cases = [{"id": f"steam:enemy:0:{i}", "edition": "steam", "category": "enemy",
                      "asset_key": "torent-0000", "binding": {"enemy": 0, "row": i},
                      "display_name": f"Torent row{i}"} for i in range(3)]
            plan = enemy_plan.build({"cooked_layout_version": CURRENT_VERSION,
                                     "libraries": {"steam": str(root)}, "cases": cases}, batch_size=1)
            self.assertEqual(len(plan["cases"]), 3)
            self.assertEqual(len(plan["cohorts"]), 2)
            self.assertEqual(len(plan["batches"]), 2)
            self.assertEqual(plan["cases"][1]["shared_evidence"]["representative"], "steam:enemy:0:0")
            self.assertTrue(all(case["capture"]["status"] == case["review"]["status"] == "pending"
                                for case in plan["cases"]))


if __name__ == "__main__":
    unittest.main()
