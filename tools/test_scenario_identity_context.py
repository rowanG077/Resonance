import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("context", Path(__file__).with_name("scenario_identity_context.py"))
CONTEXT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CONTEXT)


class ContextAudit(unittest.TestCase):
    def test_definition_notes_cannot_invent_or_override_identities(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            std = root / "scripts/std"
            std.mkdir(parents=True)
            (std / "story.sym").write_text('''script library;
// pub enum FakeFlag { MadeUp = 191 }
pub enum StoryFlag {
    Known = 42,
    // Known = 199, } pub enum AnotherFake { MadeUp = 192 }
    /* outer /* Hidden = 193, */ still hidden */
    Second = 43,
}
pub const Note: string = "pub enum QuotedFlag { MadeUp = 194 }";
pub enum Selector { Query = -1 }
''')
            self.assertEqual(CONTEXT.definitions(root), {
                "story::StoryFlag::Known": 42,
                "story::StoryFlag::Second": 43,
                "story::Selector::Query": -1,
            })

    def test_word_and_message_references_stay_distinct_and_text_is_not_executable(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work) / "source"
            assets = Path(work) / "assets"
            std = root / "scripts/std"
            std.mkdir(parents=True)
            (std / "story.sym").write_text("pub enum StoryFlag { Known = 42 }")
            (std / "scenario.sym").write_text("pub enum MessageControl { Number = 5 }")
            relative = "fields/example-0001/scripts/scenario.ssb"
            path = root / Path(relative).with_suffix(".sym")
            path.parent.mkdir(parents=True)
            path.write_text('''script scenario;
pub task entry() {
    native::set_event_bit(bit: story::StoryFlag::Known);
    end;
}
auxiliary:
data { relative32(aux_0000, auxiliary); }
aux_0000:
data {
    sjis("native::set_event_bit(42); { fake } // comment");
    /* outer /* native::clear_event_bit(42); */ still comment */
    message_control(scenario::MessageControl::Number, native::test_event_bit(bit: story::StoryFlag::Known));
}
''')
            message_path = assets / "fields/example-0001/messages.json"
            message_path.parent.mkdir(parents=True)
            message_path.write_text(json.dumps(["Flag status"]))
            (root / "identity-uses.tsv").write_text(
                "input\tpc\tdomain\tnative\tparameter\tvalue\tstatus\n"
                f"{relative}\t0x8\tstory::StoryFlag\tset_event_bit\tbit\t42\tsymbolic\n")
            message_report = root / "message-identity-uses.tsv"
            header = "input\tbyte_offset\tcontrol\tdomain\tnative\tparameter\tvalue\tstatus\n"
            message_report.write_text(header + f"{relative}\t0x101\t5\tstory::StoryFlag\ttest_event_bit\tbit\t42\tsymbolic\n")
            uses = CONTEXT.recover(root, assets)[42]
            self.assertEqual([(use["address_space"], use["pc"]) for use in uses], [("word_pc", "0x8"), ("byte", "0x101")])
            self.assertEqual(uses[1]["task"], "aux_0000")
            self.assertEqual(uses[1]["messages"], [(0, 0, "Flag status")])
            self.assertEqual([(use["domain"], use["status"]) for use in uses],
                             [("story::StoryFlag", "symbolic")] * 2)
            original = path.read_text()
            (std / "story.sym").write_text((std / "story.sym").read_text()
                                            + "\npub enum Field0001Flag { OtherMeaning = 42 }\n")
            path.write_text(original.replace(
                "native::test_event_bit(bit: story::StoryFlag::Known)",
                "native::test_event_bit(bit: story::Field0001Flag::OtherMeaning)"))
            with self.assertRaisesRegex(ValueError, "namespace does not match"):
                CONTEXT.recover(root, assets)
            path.write_text(original)
            message_report.write_text(header)
            with self.assertRaisesRegex(ValueError, "no original-byte reference"):
                CONTEXT.recover(root, assets)
            message_report.write_text(header + f"{relative}\t0x101\t5\tstory::StoryFlag\ttest_event_bit\tbit\t41\tsymbolic\n")
            path.write_text(path.read_text().replace("native::test_event_bit(bit: story::StoryFlag::Known)", "7"))
            with self.assertRaisesRegex(ValueError, "unparsed original message flag occurrences"):
                CONTEXT.recover(root, assets)

    def test_equal_flag_numbers_keep_their_domains_and_bank_reset_slots(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work) / "source"
            std = root / "scripts/std"
            std.mkdir(parents=True)
            (std / "story.sym").write_text('''
pub enum StoryFlag { Known = 42 }
pub enum Field0002Flag { Local = 42 }
pub enum StoryFlagBase { WorldResetSceneBank150 = 150 }
''')
            header = "input\tpc\tdomain\tnative\tparameter\tvalue\tstatus\n"
            rows = []
            for field, expression, domain, native, parameter, value, status in [
                ("global-0001", "story::StoryFlag::Known", "story::StoryFlag", "set_event_bit", "bit", 42, "symbolic"),
                ("local-0002", "story::Field0002Flag::Local", "story::Field0002Flag", "set_event_bit", "bit", 42, "symbolic"),
                ("unknown-0003", "152", "story::StoryFlag", "clear_event_bit", "bit", 152, "unresolved"),
                ("reset-0004", "flag_slot(story::StoryFlagBase::WorldResetSceneBank150, 2)", "story::StoryFlagBase", "clear_event_bit", "bit[2]", 150, "symbolic"),
            ]:
                relative = f"fields/{field}/scripts/scenario.ssb"
                path = root / Path(relative).with_suffix(".sym")
                path.parent.mkdir(parents=True)
                path.write_text(f"pub task entry() {{ native::{native}(bit: {expression}); end; }}")
                reported_native = "bank_reset::" + native if domain == "story::StoryFlagBase" else native
                rows.append(f"{relative}\t0x8\t{domain}\t{reported_native}\t{parameter}\t{value}\t{status}\n")
            (root / "identity-uses.tsv").write_text(header + "".join(rows))
            uses = CONTEXT.recover(root, Path(work) / "assets")
            self.assertEqual({use["domain"] for use in uses[42]},
                             {"story::StoryFlag", "story::Field0002Flag"})
            scoped = CONTEXT.select(uses, domains=["story::Field0002Flag"])
            self.assertEqual([use["field"] for use in scoped[42]], ["local-0002"])
            unknown = CONTEXT.select(uses, unresolved=True)
            self.assertEqual(list(unknown), [152])
            self.assertEqual([use["field"] for use in unknown[152]], ["unknown-0003"])
            reset = next(use for use in uses[152] if use["domain"] == "story::StoryFlagBase")
            self.assertEqual((reset["identity_value"], reset["bank_slot"]), (150, 2))
            self.assertEqual(CONTEXT.select(uses, fields=["local-0002"], unresolved=True), {})
            self.assertEqual(list(CONTEXT.select(uses, flag=42)), [42])
            self.assertEqual(CONTEXT.select(uses, flag=0), {})
            with self.assertRaisesRegex(ValueError, "unknown domain"):
                CONTEXT.select(uses, domains=["story::MisspelledFlag"])
            with self.assertRaisesRegex(ValueError, "unknown field"):
                CONTEXT.select(uses, fields=["absent-0005"])
            # A same-numbered enum from a different field must not validate
            # merely because it compiles to the same original integer.
            path = root / "fields/local-0002/scripts/scenario.sym"
            path.write_text(path.read_text().replace("story::Field0002Flag::Local", "story::StoryFlag::Known"))
            with self.assertRaisesRegex(ValueError, "namespace does not match"):
                CONTEXT.recover(root, Path(work) / "assets")


if __name__ == "__main__":
    unittest.main()
