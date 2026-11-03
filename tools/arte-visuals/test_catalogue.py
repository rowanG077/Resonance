"""Small catalogue invariants; no renderer, original game or real assets needed."""
import json
from pathlib import Path
import tempfile
import unittest

import catalogue
import gallery


def library(root, names, combinations=()):
    techniques = {key: {"text": {"name": name}, "rules": rules}
                  for _, key, name, rules in names}
    documents = {
        "content.json": {"game": "tales-of-symphonia", "version": catalogue.CURRENT_VERSION},
        "game/techniques.json": {
            "characters": {"6": "zelos"}, "learning": {"zelos": list(techniques)},
            "mappings": {str(number): key for number, key, _, _ in names},
            "techniques": techniques},
        "battle/unison.json": {"characters": {}, "host": {}, "openers": {}, "placements": [],
                               "mappings": {str(n): key for n, key, _, _ in names},
                               "combinations": list(combinations)},
        "battle/enemies/mappings.json": {},
    }
    for name, value in documents.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))


class CatalogueTests(unittest.TestCase):
    def test_proven_unison_exceptions_preserve_rows_and_do_not_spill_to_legal_recipes(self):
        def combination(edition, name, scene, ingredients=()):
            return {"edition": edition, "category": "unison", "display_name": name,
                    "scene": scene, "flags": [], "capture": {"status": "pending"},
                    "review": {"status": "pending"}, "recipe_variants": [
                        {"id": f"recipe:{index}", "arte_keys": keys, "artes": native,
                         "flags": [], "capture": {"status": "pending"},
                         "review": {"status": "pending"}}
                        for index, (keys, native) in enumerate(ingredients)]}
        recipes = [(["stalagmite", "grave-genis"], [213, 215]),
                   (["grave-genis", "grave-genis"], [215, 215]),
                   (["stalagmite", "ground-dasher"], [213, 214]),
                   (["grave-genis", "ground-dasher"], [215, 214])]
        rows = [combination("gamecube", "Innocent Edge", "innocent-edge-0306"),
                combination("steam", "Rock Pillar", "combination-0321", recipes),
                combination("steam", "Innocent Edge", "innocent-edge-0306"),
                combination("gamecube", "Innocent Edge", "innocent-edge-0306", recipes[:1]),
                combination("steam", "Rock Pillar", "combination-0321",
                            [(["stalagmite", "ground-dasher"], [999, 214])])]
        catalogue.annotate_scope(rows)
        self.assertEqual([r["scope_status"] for r in rows], ["source_unselectable"] +
                         ["original_game_required"] * 4)
        self.assertFalse(rows[0]["requires_visual_comparison"])
        self.assertTrue(rows[1]["requires_visual_comparison"])
        self.assertEqual([r["scope_status"] for r in rows[1]["recipe_variants"]],
                         ["original_game_required"] * 2 + ["source_unselectable",
                                                           "original_game_required"])
        self.assertTrue(rows[4]["recipe_variants"][0]["requires_visual_comparison"])
        self.assertEqual(len(rows[1]["recipe_variants"]), 4)
        self.assertTrue(all(r["capture"]["status"] == r["review"]["status"] == "pending"
                            for r in rows + rows[1]["recipe_variants"]))
        self.assertIn("unison_source_exceptions.json", rows[0]["scope_evidence"]["document"])
        dormant = dict(rows[0], id="gamecube:unison:7")
        status, _, _ = gallery.review_status(dormant, {}, {
            dormant["id"]: {"status": "match", "reference": "invented"}})
        self.assertEqual(status, "pending")

    def test_proven_scope_is_semantic_and_keeps_capture_history_without_passing(self):
        rows = [
            {"edition": "gamecube", "character_name": "lloyd", "asset_key": "final-justice",
             "binding": {"character": 1, "arte": 999}, "flags": [],
             "capture": {"status": "failed", "error": "no executable action"},
             "review": {"status": "pending"}},
            {"edition": "steam", "character_name": "lloyd", "asset_key": "sword-rain-tempest",
             "flags": [], "capture": {"status": "pending"}, "review": {"status": "pending"}},
            {"edition": "gamecube", "character_name": "lloyd", "asset_key": "ghost-wolf-lloyd",
             "binding": {"character": 1, "arte": 404}, "action": "battle/artes/ghost-wolf/action.json",
             "flags": [], "capture": {"status": "captured", "directory": "retained"},
             "review": {"status": "pending"}},
            {"edition": "gamecube", "character_name": "lloyd", "asset_key": "unproven-slot",
             "capabilities": {"target": "unavailable"}, "flags": [],
             "capture": {"status": "failed"}, "review": {"status": "pending"}},
        ]
        catalogue.annotate_scope(rows)
        self.assertEqual([r["scope_status"] for r in rows], ["source_unimplemented_slot",
                         "original_game_required", "repository_authored", "original_game_required"])
        self.assertEqual(rows[0]["capture"], {"status": "failed", "error": "no executable action"})
        self.assertEqual(rows[2]["capture"], {"status": "captured", "directory": "retained"})
        self.assertTrue(all(r["review"]["status"] == "pending" for r in rows))

    def test_source_scope_does_not_spill_across_editions_or_into_nonnull_and_derived_slots(self):
        rows = [{"edition": edition, "character_name": character, "asset_key": key,
                 "flags": [], "capture": {"status": "pending"}, "review": {"status": "pending"}}
                for edition, character, key in [
                    ("gamecube", "colette", "grand-cross"),
                    ("steam", "colette", "grand-cross"),
                    ("gamecube", "colette", "grand-fall"),
                    ("steam", "lloyd", "sword-rain-tempest"),
                    ("gamecube", "lloyd", "lightning-thrust")]]
        catalogue.annotate_scope(rows)
        self.assertEqual([r["scope_status"] for r in rows],
                         ["source_unimplemented_slot"] + ["original_game_required"] * 4)
        self.assertTrue(all(r["capture"]["status"] == r["review"]["status"] == "pending" for r in rows))

    def test_reused_numeric_id_and_key_do_not_inherit_counterpart_or_exclusion(self):
        with tempfile.TemporaryDirectory() as temp:
            gc, steam = Path(temp) / "gc", Path(temp) / "steam"
            library(gc, [(153, "eternal-chaos", "Eternal Chaos", {"tp_cost": 42}),
                         (58, "judgment", "Judgment", {})],
                    [{"name": "Shared combination", "scene": "shared-combination",
                      "duration": 100, "recipes": [["eternal-chaos", "judgment"]]}])
            library(steam, [(153, "eternal-chaos", "Holy Binds", {"tp_cost": 100}),
                            (124, "magic-shell", "Judgment", {})],
                    [{"name": "Shared combination", "scene": "shared-combination",
                      "duration": 100, "recipes": [["eternal-chaos", "magic-shell"]]}])
            result = catalogue.build(gc, steam, {"excluded_comparisons": ["party:6:153"]})
            rows = {row["id"]: row for row in result["cases"]}
            changed = rows["steam:party:6:153"]
            self.assertEqual(changed["counterpart"]["candidates"], [])
            self.assertIn("numeric_binding_name_changed", changed["flags"])
            self.assertTrue(changed["legacy_gc_manifest"]["same_numeric_binding_excluded"])
            self.assertFalse(changed["legacy_gc_manifest"]["applies_to_current_case"])
            self.assertEqual(changed["capture"]["status"], "pending")
            self.assertEqual(changed["review"]["status"], "pending")
            self.assertEqual(rows["steam:party:6:124"]["counterpart"]["candidates"],
                             ["gamecube:party:6:58"])
            combination = rows["steam:unison:1"]
            self.assertEqual(combination["counterpart"]["status"], "candidate")
            self.assertEqual(combination["recipe_variants"][0]["counterpart"]["status"], "unmatched")

    def test_every_unison_and_recipe_survives_including_empty_and_new_combinations(self):
        with tempfile.TemporaryDirectory() as temp:
            gc, steam = Path(temp) / "gc", Path(temp) / "steam"
            common = {"name": "Innocent Edge", "scene": "innocent-edge", "duration": 180,
                      "participants": 2, "recipes": []}
            added = {"name": "Mirage Infliction", "scene": "mirage-infliction", "duration": 120,
                     "recipes": [["one", "two"], ["one", "three"]]}
            names = [(1, "one", "One", {}), (2, "two", "Two", {}), (3, "three", "Three", {})]
            library(gc, names, [common])
            library(steam, names, [common, added])
            result = catalogue.build(gc, steam)
            rows = {row["id"]: row for row in result["cases"]}
            self.assertEqual(len([r for r in rows.values() if r["category"] == "unison"]), 3)
            self.assertEqual(rows["steam:unison:1"]["recipe_variants"], [])
            addition = rows["steam:unison:2"]
            self.assertEqual(addition["participants"], 2)
            self.assertIn("steam_only_candidate", addition["flags"])
            self.assertEqual([r["artes"] for r in addition["recipe_variants"]], [[1, 2], [1, 3]])
            self.assertTrue(all(r["review"]["status"] == "pending"
                                for r in addition["recipe_variants"]))
            self.assertTrue(all(r["counterpart"]["status"] == "unmatched"
                                for r in addition["recipe_variants"]))
            self.assertEqual(addition["recipe_variants"][1]["binding"],
                             {"combination": 2, "recipe": 1})

    def test_stale_descriptor_and_runtime_unison_dump_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            library(root, [])
            (root / "content.json").write_text('{"game":"tales-of-symphonia","version":49}')
            with self.assertRaisesRegex(ValueError, "expected.*cooked layout"):
                catalogue.enumerate_library(root, "gamecube")
            library(root, [], [{"id": 7, "native_id": 306, "name": "Innocent Edge",
                                "participants": 2, "recipes": []}])
            with self.assertRaisesRegex(ValueError, "expected current named Unison"):
                catalogue.enumerate_library(root, "gamecube")

    def test_blank_names_require_unchanged_key_and_rules_and_remain_in_inventory(self):
        with tempfile.TemporaryDirectory() as temp:
            gc, steam = Path(temp) / "gc", Path(temp) / "steam"
            library(gc, [(248, "indignation", "Indignation Judg.", {"tp_cost": 100}),
                         (144, "unused", "", {})])
            library(steam, [(248, "indignation", "", {"tp_cost": 100}),
                            (144, "unused", "", {"tp_cost": 150})])
            rows = {r["id"]: r for r in catalogue.build(gc, steam)["cases"]}
            self.assertEqual(len(rows), 4)
            self.assertEqual(rows["steam:party:6:248"]["counterpart"]["candidates"],
                             ["gamecube:party:6:248"])
            self.assertEqual(rows["steam:party:6:144"]["counterpart"]["status"], "unmatched")

    def test_enemy_row_number_and_attack_label_cannot_pair_changed_content(self):
        with tempfile.TemporaryDirectory() as temp:
            gc, steam = Path(temp) / "gc", Path(temp) / "steam"
            for root, duration in [(gc, 60), (steam, 90)]:
                library(root, [])
                (root / "battle/enemies/mappings.json").write_text('{"0":"torent-0000"}')
                path = root / "battle/enemies/torent-0000/enemy.json"
                path.parent.mkdir()
                path.write_text(json.dumps({"rules": {
                    "actions": {"rows": [{"execution": {"attack": "attack-1"}},
                                         {"execution": {"spell": 283}}]},
                    "attacks": {"attack-1": {"duration": duration}}}}))
            rows = catalogue.build(gc, steam)["cases"]
            self.assertEqual(len(rows), 4)
            self.assertTrue(all(r["counterpart"]["status"] == "unmatched" for r in rows))

    def test_enemy_native_identity_uses_placements_and_runtime_order_not_equal_numbers(self):
        with tempfile.TemporaryDirectory() as temp:
            roots = [Path(temp) / edition for edition in ["gc", "steam"]]
            for root in roots:
                library(root, [(9, "barrier-shared", "Barrier", {"spell": True}),
                               (17, "barrier", "Later Barrier", {"spell": True}),
                               (248, "wrong-number", "Wrong spell", {"power": 999})])
                unison = catalogue.read(root / "battle/unison.json")
                unison["mappings"] = {"248": "barrier"}
                # List order is deliberately the reverse of prepared runtime-ID order.
                unison["placements"] = [{"techniques": ["barrier", {
                    "technique": "barrier-shared", "matches": "barrier"}]}]
                (root / "battle/unison.json").write_text(json.dumps(unison))
                (root / "battle/enemies/mappings.json").write_text('{"233":"meredy-0233"}')
                path = root / "battle/enemies/meredy-0233/enemy.json"
                path.parent.mkdir()
                path.write_text(json.dumps({"rules": {"actions": {"rows": [
                    {"execution": {"spell": 248}},
                    {"execution": {"physical": {"technique": 248, "attack": "one"}}},
                    {"execution": {"spell": 17}}]}, "attacks": {"one": {"duration": 60}}}}))
            rows = {row["id"]: row for row in catalogue.build(*roots)["cases"]}
            spell = rows["steam:enemy:233:0"]
            self.assertEqual(spell["display_name"], "meredy: Barrier")
            self.assertEqual(spell["native_technique_binding"],
                             {"native_id": 248, "catalogue_id": 9, "asset_key": "barrier-shared"})
            self.assertEqual(rows["steam:enemy:233:1"]["native_technique_binding"],
                             spell["native_technique_binding"])
            self.assertEqual(spell["counterpart"]["candidates"], ["gamecube:enemy:233:0"])
            unresolved = rows["steam:enemy:233:2"]
            self.assertFalse(unresolved["execution_identity_resolved"])
            self.assertEqual(unresolved["display_name"], "meredy: spell 17")
            self.assertEqual(unresolved["counterpart"]["candidates"], [])
            self.assertEqual(unresolved["capture"]["status"], "pending")
            self.assertEqual(unresolved["review"]["status"], "pending")

    def test_enemy_visual_candidates_ignore_audio_and_power_but_keep_motion_and_effect_graph(self):
        with tempfile.TemporaryDirectory() as temp:
            gc, steam = Path(temp) / "gc", Path(temp) / "steam"
            for root, sound, power in [(gc, 81, "normal"), (steam, 777, "strong")]:
                library(root, [])
                (root / "battle/enemies/mappings.json").write_text('{"0":"torent-0000"}')
                path = root / "battle/enemies/torent-0000/enemy.json"
                path.parent.mkdir()
                path.write_text(json.dumps({"effects": "effects.json", "rules": {
                    "actions": {"rows": [{"execution": {"attack": "attack-1"}}]},
                    "attacks": {"attack-1": {"duration": 60, "events": [
                        [0, {"sound": sound}], [0, {"pose": {"clip": "slot-31"}}],
                        [20, {"projectile": "shot-1"}]]}},
                    "projectiles": {"shot-1": {"definition": {"velocity": [0, 1, 2]},
                                                "hit": {"power": power, "effect": 3}}}}}))
                (root / "effects.json").write_text('{"programs":[[0, {"effect": 3}]],"templates":[]}')
            rows = catalogue.build(gc, steam)["cases"]
            self.assertTrue(all(r["counterpart"]["status"] == "candidate" for r in rows))
            self.assertEqual(rows[0]["counterpart"]["basis"], "visual_action_structure")
            path = steam / "battle/enemies/torent-0000/enemy.json"
            enemy = catalogue.read(path)
            enemy["rules"]["projectiles"]["shot-1"]["definition"]["velocity"][0] = 4
            path.write_text(json.dumps(enemy))
            self.assertTrue(all(r["counterpart"]["status"] == "unmatched"
                                for r in catalogue.build(gc, steam)["cases"]))
            enemy["rules"]["projectiles"]["shot-1"]["definition"]["velocity"][0] = 0
            path.write_text(json.dumps(enemy))
            (steam / "effects.json").write_text('{"programs":[[0, {"effect": 4}]],"templates":[]}')
            self.assertTrue(all(r["counterpart"]["status"] == "unmatched"
                                for r in catalogue.build(gc, steam)["cases"]))


if __name__ == "__main__":
    unittest.main()
