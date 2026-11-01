import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import struct

from named_content import Snapshot, same_evidence


class NamedContentTests(unittest.TestCase):
    def test_unison_placement_groups_keep_matching_aliases_and_zero_durations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def write(path, value):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(json.dumps(value))
            write("content.json", dict(version=102, game="tales-of-symphonia"))
            write("game/techniques.json", {"mappings":{"1":"thrust","210":"other-thrust","9":"spell"}})
            write("battle/unison.json", dict(mappings={"10":"thrust","216":"spell"}, placements=[
                dict(techniques=["thrust",dict(technique="other-thrust",matches="thrust")],distance=350,duration=65),
                dict(techniques=["spell"],distance=400,duration=0,altitude=200)]))
            placements = Snapshot(root).unison_placements()
            self.assertEqual(placements[1], placements[210])
            self.assertEqual(placements[9],dict(native_id=216,distance=400,duration=0,altitude=200))
            self.assertNotIn(0,placements)

    def test_named_formations_keep_alias_ids_spawn_order_and_reserves(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def write(path, value):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(json.dumps(value))
            write("content.json", dict(version=102, game="tales-of-symphonia"))
            write("battle/enemies/mappings.json", {"7":"wolf", "12":"boar"})
            write("battle/formations.json", dict(
                mappings={"0":"wolves", "1":"wolves"},
                formations={"wolves":dict(actors=[dict(enemy="wolf", count=2, statistics_variant=1),
                    dict(enemy="wolf", hidden_name=True, position=[10,20])],
                    reserves=[dict(enemy="boar")], play_music=False)},
                pools=[dict(ids=[0,1], choices=[dict(formation=1,weight=30),dict(formation=0,weight=70)])]))
            table = Snapshot(root).formations()
            self.assertEqual(table["records"][0], table["records"][1])
            row = table["records"][0]
            self.assertEqual([actor["resource"] for actor in row["actors"]], [0,0,1])
            self.assertEqual([actor["variant"] for actor in row["actors"]], [1,1,0])
            self.assertEqual(row["resources"], [dict(enemy=7,hidden_name=False),
                dict(enemy=7,hidden_name=True),dict(enemy=12,hidden_name=False)])
            self.assertEqual(row["actors"][2]["position"], [10,20])
            self.assertEqual(row["settings"], dict(play_music=False,celebrate=True,
                entry_voice=True,escape_restricted=False))
            self.assertEqual(table["pools"][0], table["pools"][1])
            self.assertEqual([choice["formation"] for choice in table["pools"][0]], [1,0])

    def test_enemy_rules_and_optional_artwork_share_one_current_document(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def write(path, value):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(json.dumps(value))
            write("content.json", dict(version=102, game="tales-of-symphonia"))
            write("battle/enemies/mappings.json", {"0":"test"})
            path = "battle/enemies/test/enemy.json"
            shot = dict(definition=dict(velocity=[0,0,1]), hit=dict(power="normal"))
            rules = dict(actions=dict(rows=[dict(execution=dict(attack="spit")),
                dict(execution=dict(physical=dict(technique=2, attack="spit")))]),
                attacks=dict(spit=dict(duration=20, events=[[1,dict(projectile="seed")],
                    [2,dict(contact="touch")],[3,dict(continue_contact="touch")],
                    [4,dict(ground_projectiles=dict(projectiles=["seed","seed"],retreat=2))]])),
                contacts=dict(touch=dict(radius=3)), projectiles=dict(seed=shot))
            entry = dict(rules=rules, artwork=dict(model="preview.json"))
            write(path, entry)
            snapshot = Snapshot(root)
            result = snapshot.enemy_rules(0)
            attack = result["actions"]["rows"][0]["execution"]["attack"]
            self.assertEqual(attack["events"][0][1]["projectile"], shot)
            self.assertEqual(attack["events"][1][1], dict(contact=dict(radius=3)))
            self.assertEqual(attack["events"][2][1], dict(continue_contact=dict(radius=3)))
            self.assertEqual(attack["events"][3][1]["ground_projectiles"],
                dict(definitions=[shot["definition"],shot["definition"]], hit=shot["hit"], retreat=2))
            self.assertEqual(attack, result["actions"]["rows"][1]["execution"]["physical"]["attack"])
            self.assertEqual(snapshot.enemy_artwork(0), dict(model="preview.json"))
            self.assertEqual(set(snapshot.files), {"content.json","battle/enemies/mappings.json",path})
            entry.pop("artwork")
            rules["projectiles"]["seed"]["definition"]["velocity"][2] = 42
            write(path, entry)
            self.assertEqual(snapshot.enemy_rules(0)["actions"]["rows"][0]["execution"]["attack"], attack)
            fresh = Snapshot(root)
            self.assertIsNone(fresh.enemy_artwork(0))
            self.assertEqual(fresh.enemy_rules(0)["actions"]["rows"][0]["execution"]["attack"]["events"][0][1]["projectile"]["definition"]["velocity"][2], 42)
            entry["version"] = 1
            write(path, entry)
            with self.assertRaises(ValueError):
                Snapshot(root).enemy_entry(0)

    def test_models_resolve_named_attachments_from_glb_without_rig_documents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "content.json").write_text(json.dumps(dict(version=102, game="tales-of-symphonia")))
            def mesh(name):
                data = json.dumps(dict(asset=dict(version="2.0"), nodes=[
                    dict(name=name, extras=dict(resonance_bone=1)),
                    dict(name="root", extras=dict(resonance_bone=0), children=[2]),
                    dict(name="editor helper", children=[0]),
                    dict(name="Armature", children=[1])
                ])).encode()
                data += b" " * (-len(data) % 4)
                return struct.pack("<4sIIII", b"glTF", 2, 20+len(data), len(data), 0x4e4f534a) + data
            (root / "body.glb").write_bytes(mesh("hand"))
            definition = dict(attachment_nodes={"0":"hand"}, layers=[
                dict(mesh="body.glb")])
            (root / "model.json").write_text(json.dumps(definition))
            snapshot = Snapshot(root)
            model = snapshot.model("model.json")
            self.assertEqual(model["rig"]["attachments"], {"0":1})
            self.assertEqual(model["rig"]["skeleton"]["bones"], [
                dict(name="root", parent=None), dict(name="hand", parent=0)])
            self.assertEqual(set(snapshot.files), {"content.json", "model.json", "body.glb"})
            (root / "body.glb").write_bytes(mesh("finger"))
            definition["attachment_nodes"]["0"] = "finger"
            (root / "model.json").write_text(json.dumps(definition))
            self.assertEqual(Snapshot(root).model("model.json")["rig"]["attachments"], {"0":1})
            self.assertEqual(snapshot.model("model.json")["rig"]["skeleton"]["bones"][1]["name"], "hand")

    def test_scene_uses_current_references_and_seals_nested_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "library"
            evidence = Path(directory) / "proof"
            def write(path, value):
                path = root / path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value))
            write("content.json", dict(version=102, game="tales-of-symphonia"))
            write("battle/scenes/mappings.json", {"214": "rain"})
            write("battle/scenes/rain/scene.json", dict(duration=120,
                effects="battle/scenes/rain/effects.json"))
            (root / "atlas.ktx2").write_bytes(b"\xabKTX 20\xbb\r\n\x1a\n" + bytes(68))
            write("battle/scenes/rain/effects.json", dict(programs=[[]],
                artwork=dict(textures={"scene":[dict(images=["atlas.ktx2"])]})))
            snapshot = Snapshot(root, evidence)
            path = snapshot.scene_path(214)
            self.assertEqual(path, "battle/scenes/rain/scene.json")
            scene, bank = snapshot.scene(path)
            self.assertEqual(scene["duration"], 120)
            self.assertEqual(scene["projectiles"], [])
            self.assertNotIn("battle/scenes/rain/rules.json", snapshot.files)
            self.assertEqual(bank["programs"], [[]])
            self.assertEqual(scene["textures"][0]["images"], ["atlas.ktx2"])
            self.assertNotIn("source_sha256", scene)
            self.assertNotIn("files", scene)
            original = b"original package bytes"
            proof = snapshot.evidence(path, original)
            self.assertEqual(Path(proof["source"]["path"]).read_bytes(), original)
            self.assertEqual(proof["source"]["sha256"], hashlib.sha256(original).hexdigest())
            self.assertIn("battle/scenes/rain/effects.json", proof["files"])
            write("battle/scenes/rain/effects.json", {})
            self.assertEqual(snapshot.effects(scene["effects"])["programs"], [[]])
            fresh = Snapshot(root, evidence)
            fresh.scene_path(214)
            fresh.scene(path)
            self.assertFalse(same_evidence(proof, fresh.evidence(path, original)))
            self.assertFalse((root / "sources.json").exists())
            self.assertFalse((root / "evidence").exists())

    def test_compact_effect_defaults_and_fixed_values_keep_explicit_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "content.json").write_text(json.dumps(dict(version=102, game="tales-of-symphonia")))
            value = dict(programs=[[dict(at=1, kind="spawn", particle="spark",
                birth=dict(velocity=[None,0,None], lifetime=30, random_draws=1,
                    size=[dict(min=1,max=5,step=2,draw=0,reverse=True),None,None])) ]],
                templates=dict(spark=dict(kind="particle",template=dict(state=dict(
                    geometry=dict(Size=dict(value=[1,2,3])),colors=[[0,0,0,0],[128,128,128,128]])),
                    visual=dict(geometry=dict(kind="quad"),orientation="world",texture=dict(kind="untextured"),blend="additive"))))
            (root / "effects.json").write_text(json.dumps(value))
            snapshot=Snapshot(root)
            result=snapshot.effects("effects.json")
            template=result["actors"][0]["template"]
            self.assertEqual(template["state"]["velocity"],[0,0,0])
            self.assertEqual(template["state"]["colors"][0],[0,0,0,0])
            self.assertIsNone(template["lifetime"])
            self.assertTrue(result["actors"][0]["visual"]["depth_test"])
            birth=result["programs"][0][0]["operation"]["birth"]
            self.assertEqual(birth["velocity"],[None,dict(min=0,max=0),None])
            self.assertEqual(birth["lifetime"],dict(min=30,max=30))
            self.assertEqual(birth["size"][0],dict(min=1,max=5,step=2,draw=0,reverse=True))
            self.assertEqual(birth["random_draws"],1)
            value["templates"]["spark"]["visual"]["depth_test"]=False
            value["templates"]["spark"]["template"]["state"]["velocity"]=[0,7,0]
            (root / "effects.json").write_text(json.dumps(value))
            current=Snapshot(root).effects("effects.json")
            self.assertFalse(current["actors"][0]["visual"]["depth_test"])
            self.assertEqual(current["actors"][0]["template"]["state"]["velocity"],[0,7,0])
            self.assertEqual(snapshot.effects("effects.json"),result)

    def test_old_documents_and_escaping_paths_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "library"
            root.mkdir()
            (root / "content.json").write_text('{"version":102,"game":"tales-of-symphonia"}')
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
