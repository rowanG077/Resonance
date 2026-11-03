"""Canonical capture planning leaves execution and visual acceptance pending."""
import unittest

from unison_plan import plan_recipe


def owner(name, character, arte, *, key="shared-arte", signature="track", null=False):
    return {"character_name": name, "character": character, "arte": arte,
            "arte_key": key, "action_signature": signature,
            "placement": {"distance": 350, "duration": 90},
            "scope_status": "source_unimplemented_slot" if null else "original_game_required",
            "scope_evidence": {"entry": "proven-null"} if null else None}


class UnisonPlannerTests(unittest.TestCase):
    def test_repeated_ingredient_uses_distinct_owners_and_semantic_deterministic_order(self):
        choices = [owner("zelos", 6, 207), owner("lloyd", 1, 1), owner("kratos", 9, 207)]
        row = plan_recipe("unison:2:recipe:0", {"combination": 2, "recipe": 0},
                          [choices, list(reversed(choices))], "cross-thrust-scene")
        self.assertEqual(row["assignment"], [[9, 207], [1, 1]])
        self.assertEqual(len(row["alternatives"]), 6)
        self.assertTrue(all(a["assignment"][0][0] != a["assignment"][1][0]
                            for a in row["alternatives"]))
        self.assertEqual(row["capture"]["status"], "pending")
        self.assertEqual(row["review"]["status"], "pending")

    def test_null_ingredient_does_not_block_valid_group_alternative(self):
        row = plan_recipe("recipe", {"combination": 1, "recipe": 0}, [
            [owner("a-placeholder", 1, 30, null=True), owner("lloyd", 1, 6)],
            [owner("colette", 2, 40)]], "scene")
        self.assertEqual(row["assignment"], [[1, 6], [2, 40]])
        self.assertEqual(len(row["alternatives"]), 2)
        self.assertEqual(row["alternatives"][0]["blockers"], [{"entry": "proven-null"}])

    def test_impossible_owner_and_only_null_ingredient_stay_explicit_pending(self):
        same = owner("genis", 3, 80)
        row = plan_recipe("rock-pillar", {"combination": 23, "recipe": 2}, [[same], [same]], "scene")
        self.assertEqual(row["pending_reason"], "no_distinct_owner_assignment")
        self.assertIsNone(row["assignment"])
        row = plan_recipe("prototype", {"combination": 1, "recipe": 0}, [
            [owner("lloyd", 1, 30, null=True)], [owner("colette", 2, 40)]], "scene")
        self.assertEqual(row["pending_reason"], "source_null_ingredients")
        self.assertEqual(row["capture"]["status"], "pending")

    def test_primary_scene_signature_cannot_replace_different_ingredient_choreography(self):
        rows = [plan_recipe("recipe", {"combination": 1, "recipe": index}, [
            [owner("lloyd", 1, 6, signature=f"track-{index}")],
            [owner("colette", 2, 40)]], "same-scene") for index in range(2)]
        self.assertEqual(rows[0]["primary_choreography_signature"], rows[1]["primary_choreography_signature"])
        self.assertNotEqual(rows[0]["choreography_signature"], rows[1]["choreography_signature"])
        self.assertTrue(all(row["review"]["status"] == row["capture"]["status"] == "pending" for row in rows))


if __name__ == "__main__":
    unittest.main()
