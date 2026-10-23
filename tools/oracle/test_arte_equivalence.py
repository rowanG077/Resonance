"""Focused offline assertions for the source bridge; never launches Dolphin."""
import copy
import struct
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import arte_equivalence as bridge
import enemy_artes
import single_action_fixture


class ArteTraceTests(unittest.TestCase):
    def test_completed_ready_condition_birth_requires_exact_continuation(self):
        copied = bytes(range(24))
        setup = dict(initial_battle=dict(presentation_tick=114),
            ready_condition_history=dict(observations=[dict(particle=0x81000000,
                parent=0x81000100, age=1, copied=copied.hex(), signature=[1, 2, 3])]))
        arguments = [0, 115, 0x81000000, 0x81000100, copied, 2]
        self.assertEqual(bridge.effect_entry.completed_condition_birth(setup, *arguments), (1, 2, 3))
        for index, value in enumerate([1, 116, 0x81000004, 0x81000200, bytes(24), 3]):
            changed = arguments.copy()
            changed[index] = value
            self.assertIsNone(bridge.effect_entry.completed_condition_birth(setup, *changed))

    def test_attached_pending_entry_requires_observed_deployment(self):
        setup = dict(owner=1, source_actor_map=[dict(source_slot=0), dict(source_slot=5)],
            initial_battle=dict(combat_tick=0, actors=[dict(slot=4, hp=66, max_hp=100)]),
            defense_deployed=False, body_resources={"4": {}},
            assistance=dict(preparation=dict(prerequisites=dict(
                defense_deployment=dict(core=4, hp_percent=66, initial_phase=0)))))
        rows = [dict(battle_actor_5_action=action, actor_5_selected=42,
                     battle_combat_tick_word=tick)
                for action, tick in [(13 << 24, 0), (3 << 24, 79),
                                     (4 << 24, 80), ((5 << 24) | (1 << 16), 95)]]
        with patch.object(bridge, "observed_body", return_value=(32, 15)):
            self.assertEqual(bridge.enemy_pending_entry(rows, setup, dict(selected=42), 3), 80)
            setup["initial_battle"]["actors"][0]["hp"] = 67
            with self.assertRaisesRegex(ValueError, "deployment prelude"):
                bridge.enemy_pending_entry(rows, setup, dict(selected=42), 3)
        setup["initial_battle"]["actors"][0]["hp"] = 66
        with patch.object(bridge, "observed_body", return_value=(0, 1)):
            with self.assertRaisesRegex(ValueError, "deployment prelude"):
                bridge.enemy_pending_entry(rows, setup, dict(selected=42), 3)

    def test_ground_release_prepares_distinct_party_recipients(self):
        ground = dict(events=[[29, dict(ground_projectiles=dict(definitions=[{}, {}]))]])
        self.assertEqual(enemy_artes.distinct_ground_targets([
            dict(execution=None), dict(execution="leave_arena"),
            dict(execution=dict(attack=dict(events=[])))]), 1)
        for execution in [dict(attack=ground), dict(physical=dict(attack=ground))]:
            self.assertEqual(enemy_artes.distinct_ground_targets([dict(execution=execution)]), 2)

    def test_enemy_approach_replays_observed_selection_instead_of_initial_target(self):
        setup = dict(owner=2, target=1, requested_target=0,
            actors=[dict(side=0), dict(side=0), dict(side=1)],
            source_actor_map=[dict(source_slot=0), dict(source_slot=1), dict(source_slot=4)],
            initial_battle=dict(combat_tick=0, actors=[dict(address=100), dict(address=200), dict(address=300)]))
        rows = [dict(battle_combat_tick_word=tick, battle_actor_4_target=target)
                for tick, target in [(32,100),(33,200),(34,200)]]
        self.assertEqual(bridge.enemy_approach_targets(rows,setup,33,2),
                         [dict(after_updates=33,target=1)])
        rows[1]["battle_actor_4_target"] = 300
        with self.assertRaisesRegex(ValueError,"observed opponent"):
            bridge.enemy_approach_targets(rows,setup,33,2)
        rows[1]["battle_actor_4_target"] = 100
        with self.assertRaisesRegex(ValueError,"bound admission"):
            bridge.enemy_approach_targets(rows,setup,33,2)

    def test_wrong_admission_requires_a_bound_different_enemy_row(self):
        row = dict(battle_actor_4_action=(5 << 24) | (1 << 16), actor_4_selected=42)
        self.assertTrue(single_action_fixture.wrong_admission(row, 4, dict(selected=41)))
        self.assertFalse(single_action_fixture.wrong_admission(row, 4, dict(selected=42)))
        self.assertFalse(single_action_fixture.wrong_admission(row, 4, dict(native=41)))
        for action in (3 << 24, 4 << 24, 5 << 24, (20 << 24) | (1 << 16)):
            self.assertFalse(single_action_fixture.wrong_admission(
                dict(row, battle_actor_4_action=action), 4, dict(selected=41)))

    def test_clear_enemy_anchor_preserves_pair_spacing_and_clears_neutral_bodies(self):
        geometry = {slot: dict(spheres=[dict(root_offset=[0, 0, 0], radius=10)])
                    for slot in (0, 4, 5, 6)}
        positions = {0: [0, 0, 0], 4: [100, 25, 0], 5: [100, 0, 0], 6: [60, 0, 0]}
        chosen, exclusions = bridge.clear_enemy_anchor(geometry, positions, 4, 100, positions[4])
        self.assertEqual(chosen, [39, 25, 0])
        self.assertTrue(all(not lo <= chosen[0] <= hi for lo, hi in exclusions))
        for selected in (chosen[0], chosen[0] - 100):
            self.assertTrue(all(abs(selected - positions[slot][0]) > 20 for slot in (5, 6)))
        self.assertEqual(positions[4], [100, 25, 0])

    def fixture(self):
        owner, target = 0x81000000, 0x81002000
        setup = dict(owner=0, target=1, actors=[dict(actor=0, side=0), dict(actor=1, side=1)],
            source_actor_map=[dict(actor=0, source_slot=0), dict(actor=1, source_slot=4)],
            input={}, initial_battle=dict(combat_tick=0, presentation_tick=0,
                actors=[dict(slot=0, address=owner), dict(slot=4, address=target)]))
        # Retained captures used a head-only observer; preserve that schema here.
        row = {name: 0 for name in bridge.locations(contact_slot=0).values() if not name.endswith("_prev")}
        row["battle_pool"] = 0x80900000
        for label, sentinel in (("projectile", 0x75c40), ("program", 0x75ec8), ("particle", 0x763d8)):
            row[f"{label}_0_address"] = row["battle_pool"] + sentinel
        row["effect_alternate_head"] = row["battle_pool"] + 0x76660
        row["alternate_0_address"] = row["battle_pool"] + 0x76660
        row.update(battle_presentation_tick_word=1, battle_combat_tick_word=1,
                   battle_actor_0_hp=100, battle_actor_4_hp=100, actor_0_native=35)
        row["battle_actor_0_action"] = (5 << 24) | (1 << 16)
        return setup, row, target

    def trace(self, setup, rows):
        return bridge.normalize(dict(id="party:2:35"), rows, setup, {}, dict(native=35), None)

    def test_spell_debuff_requires_owned_scene_and_observed_opponent_change(self):
        setup, row, _ = self.fixture()
        setup["opponent_control"] = dict(mode="manual")
        setup["stored_scenes"] = dict(registrations=[dict(native=265,
            owner=setup["initial_battle"]["actors"][0]["address"], bank_sha256="rain")])
        row["actor_0_native"] = 265
        after = dict(row, actor_4_condition_14=1 << 18,
                     battle_combat_tick_word=2, battle_presentation_tick_word=2)
        frames = [dict(combat_tick=1, presentation_tick=1, programs=[]),
                  dict(combat_tick=2, presentation_tick=2,
                       programs=[dict(id=7, age=1, owner=0, bank="rain", program=1)])]
        events = [dict(kind="started", owner=0, action=1)]
        case = dict(definition=dict(execution=dict(spell=265)))
        observe = bridge.support_outcomes.spell_debuffs
        self.assertEqual(observe(case, [row, after], setup, frames, events),
            [dict(kind="condition_changed", owner=0, target=1, action=1,
                  before=[], after=["defense_down"], clock=1, presentation_clock=1)])
        for change in (dict(battle_actor_4_hp=99), dict(actor_0_native=264),
                       dict(actor_4_condition_14=1 << 17), dict(battle_actor_4_action=5 << 24)):
            with self.assertRaises(ValueError):
                observe(case, [row, dict(after, **change)], setup, frames, events)
        with self.assertRaises(ValueError):
            observe(case, [row, after], setup,
                    [frames[0], dict(frames[1], programs=[])], events)
        released = dict(row, battle_combat_tick_word=2, battle_presentation_tick_word=2)
        lingering = dict(after, actor_0_native=0, battle_actor_0_action=3 << 24,
                         battle_combat_tick_word=3, battle_presentation_tick_word=3)
        later = dict(frames[1], combat_tick=3, presentation_tick=3,
                     programs=[dict(frames[1]["programs"][0], age=2)])
        applied = observe(case, [row, released, lingering], setup, frames + [later], events)
        self.assertEqual((applied[0]["clock"], applied[0]["after"]), (2, ["defense_down"]))

    def test_unattributed_hp_loss_and_wrong_action_never_become_arte_hit(self):
        setup, row, _ = self.fixture()
        after = copy.deepcopy(row)
        after["battle_actor_4_hp"] = 80
        result = self.trace(setup, [row, after])
        self.assertFalse(any(e["kind"] == "hit" for e in result["events"]))
        self.assertEqual(result["events"][-1]["kind"], "unattributed_hp_loss")
        self.assertIn("HP loss could not be attributed to an observed contact",
                      result["incomplete_observations"])

        row["actor_0_native"] = after["actor_0_native"] = 36
        self.assertFalse(any(e["kind"] == "started" for e in self.trace(setup, [row, after])["events"]))

        setup["common_effect_bank"] = "common"
        setup["actors"][1]["max_hp"] = 100
        row["actor_4_condition_14"] = after["actor_4_condition_14"] = 1
        after["battle_actor_4_hp"] = 99
        common = dict(combat_tick=2, presentation_tick=2)
        events = [dict(kind="condition_changed", owner=0, target=1, action=1,
                       before=[], after=["poison_mild"], combat_tick=1, presentation_tick=1),
                  dict(common, kind="program_started", owner=1, bank="common", program=44)]
        pulse = bridge.poison_pulse(after, row, setup, 0, 1, events, common)
        self.assertEqual((pulse["condition"], pulse["amount"]), ("poison_mild", 1))
        self.assertIsNone(bridge.poison_pulse(after, row, setup, 0, 1, events[:1], common))
        self.assertIsNone(bridge.poison_pulse(after, row, setup, 0, 1, events[1:], common))
        after["battle_actor_4_hp"] = 100
        self.assertIsNone(bridge.poison_pulse(after, row, setup, 0, 1, events, common))

    def test_poison_pulse_preserves_attribution_across_other_condition_changes(self):
        setup, row, _ = self.fixture()
        setup["common_effect_bank"] = "common"
        setup["actors"][1]["max_hp"] = 9999
        before = dict(row, actor_4_condition_14=3, battle_actor_4_hp=9000)
        after = dict(before, battle_actor_4_hp=8801)
        common = dict(combat_tick=94, presentation_tick=94)
        applied = dict(kind="condition_changed", owner=0, target=1, action=1,
                       before=[], after=["poison_severe"], combat_tick=33, presentation_tick=33)
        later = dict(applied, before=["poison_severe"], after=["poison_mild", "poison_severe"],
                     combat_tick=77, presentation_tick=77)
        emission = dict(common, kind="program_started", owner=1, bank="common", program=44)
        events = [applied, later, emission]
        pulse = bridge.poison_pulse(after, before, setup, 0, 1, events, common)
        self.assertEqual((pulse["condition"], pulse["amount"]), ("poison_severe", 199))
        for changed in (dict(later, after=["poison_mild"]),
                        dict(later, owner=1, before=["poison_mild"])):
            self.assertIsNone(bridge.poison_pulse(after, before, setup, 0, 1,
                                               [applied, changed, emission], common))

    def test_direct_enemy_ailments_require_the_owned_callback_and_two_groups(self):
        setup, row, _ = self.fixture()
        setup["opponent_control"] = dict(mode="manual")
        row.update(battle_actor_0_cast_clock_word=119, actor_0_target_state_word=3 << 24)
        after = dict(row, battle_combat_tick_word=2, battle_actor_0_cast_clock_word=120,
                     actor_4_condition_14=(1 << 1) | (1 << 17))
        case = dict(enemy=239, action_row=5)
        programs = [dict(kind="program_started", owner=0, program=7, bank="owned", combat_tick=1)]
        common = dict(combat_tick=2, presentation_tick=2)
        observe = bridge.support_outcomes.enemy_debuffs
        expected = dict(common, kind="condition_changed", owner=0, target=1, action=1,
                        before=[], after=["attack_down", "poison_severe"])
        self.assertEqual(observe(case, row, after, setup, programs, common, 1), [expected])
        self.assertEqual(observe(dict(enemy=239, action_row=4), row, after,
                                 setup, programs, common, 1), [])
        for change in (dict(battle_actor_4_hp=99), dict(actor_4_condition_14=2 | 8),
                       dict(actor_4_fresh_contact_word=1 << 24),
                       dict(actor_0_target_state_word=2 << 24),
                       dict(battle_actor_4_action=5 << 24)):
            with self.assertRaises(ValueError):
                observe(case, row, dict(after, **change), setup, programs, common, 1)
        with self.assertRaises(ValueError):
            observe(case, row, after, setup, [dict(programs[0], owner=1)], common, 1)

    def test_contact_target_transition_keeps_canonical_owner_and_recipient(self):
        setup, row, target = self.fixture()
        after = copy.deepcopy(row)
        after.update(actor_0_contact_target=target, battle_actor_4_hp=80)
        result = self.trace(setup, [row, after])
        hit = next(event for event in result["events"] if event["kind"] == "hit")
        self.assertEqual((hit["owner"], hit["target"], hit["action"], hit["hp_change"]), (0, 1, 1, -20))
        later = copy.deepcopy(after)
        later["battle_actor_4_hp"] = 60
        result = self.trace(setup, [row, after, later])
        self.assertEqual(sum(event["kind"] == "hit" for event in result["events"]), 1)
        self.assertIn("HP loss could not be attributed to an observed contact",
                      result["incomplete_observations"])

        setup["knockdown_impacts"] = True
        setup["damage_context"] = [dict(blocked=[]), dict(blocked=[])]
        row.update(actor_4_recoil_word=0, actor_4_last_damage_word=0)
        after.update(actor_4_recoil_word=1, actor_4_last_damage_word=20 << 16,
                     battle_actor_4_action=14 << 24, battle_actor_4_body_clip_word=8 << 8,
                     actor_4_animation_0_layer_word=6, actor_4_animation_0_impact_state_word=0,
                     battle_combat_tick_word=2, battle_presentation_tick_word=2)
        later = dict(after, actor_4_recoil_word=0, actor_4_last_damage_word=5 << 16,
                     battle_actor_4_hp=75, battle_combat_tick_word=3, battle_presentation_tick_word=3)
        result = self.trace(setup, [row, after, later])
        impact = next(event for event in result["events"] if event["kind"] == "knockdown_impact")
        self.assertEqual((impact["target"], impact["amount"], impact["contact_clock"]), (1, 5, 1))
        self.assertEqual(sum(event["kind"] == "hit" for event in result["events"]), 1)
        later["actor_4_recoil_word"] = 1
        self.assertFalse(any(event["kind"] == "knockdown_impact"
                             for event in self.trace(setup, [row, after, later])["events"]))
        later.update(actor_4_recoil_word=0, actor_4_fresh_contact_word=1 << 24,
                     actor_4_last_damage_word=3 << 16, battle_actor_4_hp=72)
        after["actor_0_contact_target"] = 0
        result = self.trace(setup, [row, dict(after, actor_0_contact_target=target), after, later])
        impacts = [event for event in result["events"] if event["kind"] == "knockdown_impact"]
        self.assertEqual([event["amount"] for event in impacts], [5])
        self.assertEqual(result["events"][-1]["hp_change"], -3)
        later["actor_4_last_damage_word"] = 4 << 16
        self.assertFalse(any(event["kind"] == "knockdown_impact"
                             for event in self.trace(setup, [row, after, later])["events"]))

        setup, row, target = self.fixture()
        setup["damage_context"] = [dict(blocked=[], familiarity=100)] * 2
        for actor in setup["actors"]:
            actor.update(conditions=[], stats=dict(slash=200, thrust=200, defense=20,
                intelligence=100, accuracy=100, evasion=100, level=10))
        contact = dict(rule_flags_element=10 << 8, rule_power=50 << 24,
                       rule_conditions=1 << 7, contact_kind=0)
        after = dict(row, actor_0_contact_target=target, actor_4_fresh_contact_word=1 << 24,
                     battle_actor_4_hp=80)
        ordinary = bridge.contact_damage_range(after, row, contact, setup, 0, 1, 100, 100)
        for condition in (1, 2, 3, 8, 1 << 8):
            affected = dict(after, actor_4_condition_14=condition)
            self.assertEqual(bridge.contact_damage_range(affected,
                dict(row, actor_4_condition_14=condition), contact, setup, 0, 1, 100, 100), ordinary)
        for changes in (dict(actor_0_condition_10=(1 << 24) | 2 | 4),
                        dict(actor_4_condition_10=1 << 24)):
            self.assertEqual(bridge.contact_damage_range(dict(after, **changes), dict(row, **changes),
                contact, setup, 0, 1, 100, 100), ordinary)
        for changes in (dict(actor_4_condition_10=2), dict(actor_0_condition_14=1 << 30)):
            with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, "modifier"):
                bridge.contact_damage_range(dict(after, **changes), dict(row, **changes), contact, setup, 0, 1, 100, 100)
        adjusted_contact = dict(contact, rule_power=4 << 16 | 65531, projectile=0)
        before_adjustment = dict(row, actor_0_attack_power_word=100)
        after_adjustment = dict(after, actor_0_attack_power_word=95, contact_queue_counts=1 << 16,
            battle_combat_tick_word=row["battle_combat_tick_word"] + 1,
            contact_queue_0_0_owner=0x81000000, contact_queue_0_0_context=0x81001184,
            actor_0_contact_0_rule=0x81080000, actor_0_contact_0_template=0x81080100,
            actor_0_contact_0_nominal=20)
        after_adjustment.update({"actor_0_contact_0_" + key: value
                                 for key, value in adjusted_contact.items() if key != "projectile"})
        adjusted_contact = bridge.queued_contact(after_adjustment, setup, 0, 1, before_adjustment)
        self.assertEqual(bridge.contact_damage_range(after_adjustment, before_adjustment,
            adjusted_contact, setup, 0, 1, 100, 95), ordinary)
        multiple_volumes = dict(after_adjustment, contact_queue_counts=2 << 16,
            contact_queue_0_1_owner=0x81000000, contact_queue_0_1_context=0x810011bc)
        multiple_volumes.update({key.replace("contact_0_", "contact_1_"): value
            for key, value in after_adjustment.items() if key.startswith("actor_0_contact_0_")})
        bound = bridge.queued_contact(multiple_volumes, setup, 0, 1, before_adjustment)
        self.assertEqual(bridge.contact_damage_range(multiple_volumes, before_adjustment,
            bound, setup, 0, 1, 100, 95), ordinary)
        for changed in (dict(after_adjustment, actor_0_attack_power_word=94),
                        dict(after_adjustment, contact_queue_counts=2),
                        dict(after_adjustment, actor_0_fresh_contact_word=1 << 24),
                        dict(after_adjustment, battle_combat_tick_word=row["battle_combat_tick_word"])):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_damage_range(changed, before_adjustment, adjusted_contact,
                                            setup, 0, 1, 100, 95)
        for changed in (dict(adjusted_contact, projectile=123),
                        dict(adjusted_contact, contact_kind=2 << 16)):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_damage_range(after_adjustment, before_adjustment, changed,
                                            setup, 0, 1, 100, 95)
        node = 0x81090000
        projectile_hit = dict(after_adjustment, actor_0_attack_power_word=75,
            contact_queue_0_0_projectile=node + 0x28, contact_queue_0_0_context=node + 0x168,
            projectile_0_address=node, projectile_0_next=row["battle_pool"] + 0x75c40,
            projectile_0_14=1 << 24, projectile_0_28=0x81000000,
            projectile_0_16c=0x81080000, projectile_0_170=0x81080100,
            projectile_0_144=100 << 16, projectile_0_19c=20,
            projectile_0_rule_flags_element=contact["rule_flags_element"],
            projectile_0_rule_power=4 << 16 | 65531,
            projectile_0_rule_conditions=contact["rule_conditions"], projectile_0_contact_kind=0)
        before_projectile = dict(row, actor_0_attack_power_word=80)
        self.assertEqual(bridge.projectile_damage_range(projectile_hit, before_projectile,
            "projectile_0", setup, 0, 1, 100), ordinary)
        with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, "unique live projectile"):
            bridge.projectile_damage_range(dict(projectile_hit, contact_queue_0_0_projectile=node + 0x30),
                before_projectile, "projectile_0", setup, 0, 1, 100)
        charged = dict(after, actor_0_raw_1aa8=0x20)
        self.assertEqual(bridge.contact_damage_range(charged, row, contact, setup, 0, 1, 100, 100), ordinary)
        for flag in (0x40, 0x01000000, 0x02000000):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_damage_range(dict(after, actor_0_raw_1aa8=flag), row, contact, setup, 0, 1, 100, 100)
        blocked = copy.deepcopy(setup)
        blocked["damage_context"][0]["blocked"] = ["equipped EX damage modifiers"]
        with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
            bridge.contact_damage_range(charged, row, contact, blocked, 0, 1, 100, 100)
        after["actor_4_condition_14"] = 1 << 7
        applied = bridge.contact_condition(after, row, contact, setup, 0, 1)
        self.assertEqual(applied, dict(kind="condition_changed", owner=0, target=1,
                                      action=1, before=[], after=["curse"]))
        self.assertEqual(bridge.contact_damage_range(after, row, contact, setup, 0, 1, 100, 100), ordinary)
        for prior, current in ((dict(row, actor_4_condition_14=1 << 7), after),
                               (row, dict(after, actor_4_condition_14=(1 << 7) | (1 << 4))),
                               (row, dict(after, actor_4_condition_38=1))):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_damage_range(current, prior, contact, setup, 0, 1, 100, 100)

    def test_contact_ailment_preserves_existing_poison(self):
        setup, row, _ = self.fixture()
        contact = dict(rule_conditions=1, rule_power=(50 << 24) | (1 << 16) | 100)
        before = dict(row, actor_4_condition_14=2)
        after = dict(before, actor_4_condition_14=3, actor_4_fresh_contact_word=1 << 24,
                     actor_0_contact_target=setup["initial_battle"]["actors"][1]["address"],
                     battle_actor_4_hp=90)
        applied = bridge.contact_condition(after, before, contact, setup, 0, 1)
        self.assertEqual(applied["before"], ["poison_severe"])
        self.assertEqual(applied["after"], ["poison_mild", "poison_severe"])
        for changed in (dict(after, actor_4_condition_14=1),
                        dict(after, actor_4_condition_14=3 | (1 << 7)),
                        dict(after, actor_4_fresh_contact_word=0)):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_condition(changed, before, contact, setup, 0, 1)

    def test_knockdown_damage_requires_observed_active_window(self):
        setup, row, _ = self.fixture()
        contact = dict(rule_flags_element=0x01080014)
        previous = dict(row, actor_4_raw_280=0x21000000,
                        actor_4_protection_window_word=0x3a010000)
        after = dict(previous, actor_4_protection_window_word=0x39010000)
        self.assertEqual(bridge.contact_protection_shift(after, previous, contact, setup, 1), 3)
        bypass = dict(contact, rule_flags_element=contact["rule_flags_element"] | (0x40 << 16))
        self.assertEqual(bridge.contact_protection_shift(after, previous, bypass, setup, 1), 0)
        for observed in (dict(after, actor_4_protection_window_word=0x00010000),
                         dict(after, actor_4_raw_280=0x41000000),
                         {key: value for key, value in after.items()
                          if key != "actor_4_protection_window_word"}):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_protection_shift(observed, previous, contact, setup, 1)
        self.assertEqual(bridge.contact_protection_shift(row, row, contact, setup, 1), 0)
        launched = dict(row, actor_4_raw_280=0x21000000, battle_actor_4_action=14 << 24,
                        battle_actor_4_hp=90)
        self.assertEqual(bridge.contact_protection_shift(launched, row, contact, setup, 1), 0)
        with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
            bridge.contact_protection_shift(dict(launched, battle_actor_4_hp=100), row, contact, setup, 1)

    def test_knockdown_landing_precedes_the_next_contact(self):
        setup, row, _ = self.fixture()
        contact = dict(rule_flags_element=0x00040023)
        before = dict(row, battle_actor_4_action=9 << 24,
                      actor_4_target_state_word=0x0d000300, actor_4_recoil_word=2,
                      actor_4_protection_window_word=0x00001800)
        after = dict(before, battle_combat_tick_word=before["battle_combat_tick_word"] + 1,
                     battle_actor_4_action=14 << 24, battle_actor_4_hp=74,
                     actor_4_target_state_word=0x0f000900, actor_4_raw_280=0x21000000,
                     actor_4_protection_window_word=0x3b010800)
        self.assertEqual(bridge.contact_protection_shift(after, before, contact, setup, 1), 3)
        bypass = dict(contact, rule_flags_element=contact["rule_flags_element"] | (0x40 << 16))
        self.assertEqual(bridge.contact_protection_shift(after, before, bypass, setup, 1), 0)
        for changed in (dict(before, actor_4_recoil_word=0),
                        dict(before, actor_4_protection_window_word=0x00011800),
                        dict(before, battle_actor_4_y_bits=0x3f800000),
                        dict(before, battle_combat_tick_word=after["battle_combat_tick_word"]),
                        dict(before, actor_4_target_state_word=17 << 24)):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.contact_protection_shift(after, changed, contact, setup, 1)

    def test_overflow_and_menu_are_explicit_observation_failures(self):
        setup, row, _ = self.fixture()
        row["battle_menu_flags_word"] = 0x80000000
        for index in range(bridge.PROJECTILES):
            row[f"projectile_{index}_address"] = 0x81200000 + index * 0x300
            row[f"projectile_{index}_next"] = 0x81200000 + (index + 1) * 0x300
        result = self.trace(setup, [row])
        self.assertTrue(any("linked list exceeds" in value for value in result["incomplete_observations"]))
        self.assertTrue(any("menu_or_pause" in value for value in result["incomplete_observations"]))

    def test_inactive_menu_bits_do_not_reject_real_idle_combat(self):
        globals_address, session = 0x81000000, 0x81100000
        entry_words = {0x8035a578: globals_address, globals_address + 0x40: 2500,
                       globals_address + 0x4c: 1, globals_address + 0x50: 9}
        ready_words = {0x8035a768: session, session + 0x19f: 2,
                       session + 0x10d0: 332, session + 0x1e3c: 4,
                       session + 0xc9d + 27 // 8: 1 << (27 % 8)}
        entry = bridge.SimpleNamespace(read=lambda address, fmt="I": entry_words.get(address, 0))
        ready = bridge.SimpleNamespace(read=lambda address, fmt="I": ready_words.get(address, 0))
        context = bridge.preparation_context(ready, entry)
        self.assertEqual((context["story"], context["colette_state"], context["world_music"]), (2500, 1, 9))
        self.assertEqual((context["battle_rank"], context["map"], context["event_flags"]), (2, 332, [27]))
        self.assertTrue(context["overlimit_boost"])
        with self.assertRaisesRegex(ValueError, "preparation globals"):
            bridge.preparation_context(ready, ready)
        setup, row, _ = self.fixture()
        row["battle_menu_flags_word"] = 0x600000fd
        result = self.trace(setup, [row])
        self.assertFalse(any("menu_or_pause" in value for value in result["incomplete_observations"]))
        owner = setup["initial_battle"]["actors"][0]["address"]
        setup["stored_scenes"] = dict(registrations=[dict(slot=0, owner=owner, native=238, resource=900)])
        row.update(battle_pause_flags_word=3 << 16, battle_stored_slots_word=0x01000100,
                   battle_stored_owner_word=owner, battle_stored_0_owner_word=owner,
                   battle_stored_0_flags_technique_word=(2 << 16) | 238,
                   battle_stored_countdown_word=59 << 16, battle_actor_0_stored_slot_word=0)
        self.assertTrue(bridge.owned_stored_pause(row, setup))
        self.assertFalse(any("menu_or_pause" in value for value in self.trace(setup, [row])["incomplete_observations"]))
        row["battle_stored_0_owner_word"] += 4
        self.assertFalse(bridge.owned_stored_pause(row, setup))
        self.assertTrue(any("menu_or_pause" in value for value in self.trace(setup, [row])["incomplete_observations"]))
        setup["stored_scenes"]["registrations"][0].update(kind="skill", native=140)
        row.update(battle_pause_flags_word=65 << 16, battle_stored_0_owner_word=owner,
                   battle_stored_0_flags_technique_word=(1 << 16) | 140,
                   battle_stored_countdown_word=29 << 16)
        self.assertTrue(bridge.owned_stored_pause(row, setup))
        row.update(battle_pause_flags_word=128 << 16, battle_stored_slots_word=0x101,
                   battle_stored_0_resource_word=900)
        self.assertTrue(bridge.owned_stored_pause(row, setup))
        row.update(battle_stored_owner_word=0, battle_stored_countdown_word=0)
        self.assertTrue(bridge.owned_stored_pause(row, setup))
        for changes in (dict(battle_stored_countdown_word=1 << 16),
                        dict(battle_stored_0_resource_word=0),
                        dict(battle_stored_0_owner_word=owner + 4)):
            self.assertFalse(bridge.owned_stored_pause(dict(row, **changes), setup))

    def test_contact_window_is_not_a_hit_and_disarm_uses_its_own_nibble(self):
        raw = {0x278: 4 << 16, 0xac: (3 << 16) | 1, 0xd8: 0}
        self.assertEqual(bridge.projectile_contact(raw.__getitem__), (4, True, False))
        raw[0x278] = 5 << 16
        self.assertEqual(bridge.projectile_contact(raw.__getitem__), (5, False, False))
        raw[0xac], raw[0xd8] = 0, 0x10
        self.assertEqual(bridge.projectile_contact(raw.__getitem__), (5, False, True))

    def test_model_instance_transform_does_not_use_shared_last_draw(self):
        world = bridge.static_model_world([10,20,30], [1,2,3], [0,0,0], [2,3,4], 0, 0)
        self.assertEqual(world, [2,0,0,11, 0,3,0,22, 0,0,4,33])
        self.assertIsNone(bridge.static_model_world([0]*3,[0]*3,[0]*3,[1]*3,0x40000000,0))
        parent = [0., 0., 1., 100., 0., 1., 0., 200., -1., 0., 0., 300.]
        attached = bridge.static_model_world([900., 0., 0.], [1., 2., 3.],
            [0., 0., 0.], [2., 3., 4.], 0x40000000, 0., parent)
        self.assertEqual(attached, [0., 0., 4., 103., 0., 3., 0., 202., -2., 0., 0., 299.])
        self.assertEqual(bridge.static_model_world([900.]*3, [1.]*3,
            [10.]*3, [2.]*3, 0x20000, 0., parent), parent)

    def test_model_joint_requires_the_live_retained_parent_descriptor(self):
        plan = [dict(bank="bank", retained=[0], joints=[dict(model=0, joint=1, bone=73)])]
        binding = dict(attachment=dict(retained=0, joint=1))
        row = dict(child_b4=0x01000000, child_184=0x81001028,
            program_10=0, program_c0=0x81002028, parent_10=0x01000000,
            parent_28=0x03010000, parent_184=0x81001028,
            parent_168=0x81200000, parent_fc=0, model_bank_6=0x81200000,
            model_joint_6_0_73_cache=0x81400000)
        matrix = [0., 0., 1., 100., 0., 1., 0., 200., -1., 0., 0., 300.]
        row.update({f"model_joint_6_0_73_{i}": struct.unpack(">I", struct.pack(">f", value))[0]
                    for i, value in enumerate(matrix)})
        records = [("program", 0x81001000), ("parent", 0x81002000)]
        observe = lambda: bridge.effect_model_joints.observed(
            row, "child", "bank", binding, plan, records, [], bridge.f32)
        self.assertEqual(observe(), matrix)
        row["program_c0"] += 0x288
        self.assertIsNone(observe())
        row["program_c0"] -= 0x288
        row["parent_168"] += 0x7e0
        self.assertIsNone(observe())

    def test_observer_fits_existing_datagram_receiver(self):
        for slots in ([0, 4], [0,1,2,3,4], list(range(12))):
            _, budget = bridge.observer_plan(slots)
            self.assertLess(budget["maximum_bytes"], 60000)
        watch, budget = bridge.observer_plan([0,4], 0x812c1000, (8,4,48,2,5), particle_transforms=False)
        self.assertLess(budget["maximum_bytes"], 60000)
        self.assertNotIn("particle_0_170", watch.values())
        self.assertIn("particle_0_parent_owner", watch.values())
        watch, budget = bridge.observer_plan([0,4,5], 0x812c1000, (4,8,32,4,8), knockdown_impacts=True)
        self.assertLess(budget["maximum_bytes"], 60000)
        self.assertIn("actor_5_last_damage_word", watch.values())
        self.assertIn("actor_5_animation_1_impact_state_word", watch.values())
        for count, capacities in ((76, (4,12,76,2,4)), (48, (1,2,48,12,4))):
            addresses = [0x813339b8 - index * bridge.OBJECT_BYTES for index in range(count)]
            watch, budget = bridge.observer_plan([0,1,2,3,4], 0x812c1020, capacities,
                                                 particle_addresses=addresses, contact_slot=0)
            self.assertLess(budget["maximum_bytes"], 60000)
            self.assertEqual(watch[f"{addresses[0] + 0x170:x}"], "particle_0_170")
            self.assertEqual(watch[f"{addresses[0] + 0xfc:x}"], "particle_0_fc")
            self.assertIn("particle_head", watch.values())
            self.assertEqual(budget["particle_addresses"], addresses)
        addresses = [0x813339b8 - index * bridge.OBJECT_BYTES
                     for index in range(bridge.OBJECT_POOL_CAPACITY)]
        watch, budget = bridge.observer_plan([0,1,2,3,4], 0x812c1020, (8,12,416,24,8),
            particle_addresses=addresses, contact_slot=0)
        self.assertLessEqual(budget["maximum_bytes"], budget["receiver_bytes"])
        for index, address in enumerate(addresses):
            self.assertEqual(watch[f"{address + 0x170:x}"], f"particle_{index}_170")
            self.assertEqual(watch[f"{address + 0x134:x}"], f"particle_{index}_134")
        with self.assertRaisesRegex(ValueError, "exceeds"):
            bridge.packet_budget({f"{0x81000000 + index * 4:x}": f"word_{index}"
                                 for index in range(bridge.MEM1_DATAGRAM_BYTES // bridge.MEM1_WORD.size)})

    def test_combined_observations_retain_verified_source_segments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = [dict(presentation_counter=clock, case_marker=42) for clock in range(4)]
            segments = []
            for name, part in (("admission", rows[:1]), ("tail", rows[1:])):
                output = root / name
                output.mkdir()
                bridge.write(output / "observations.json", part)
                bridge.write(output / "capture.json", dict(complete=True, samples=[dict(
                    observations=len(part), observations_sha256=bridge.digest(output / "observations.json"))]))
                segments.append(str(output / "capture.json"))
            bridge.write(root / "observations.json", rows)
            bridge.write(root / "capture.json", dict(complete=True, segments=segments,
                combined_observations=len(rows),
                combined_observations_sha256=bridge.digest(root / "observations.json")))
            self.assertEqual(bridge.read_capture_observations(root / "capture.json"), rows)
            bridge.write(root / "tail" / "observations.json", rows[2:])
            with self.assertRaisesRegex(ValueError, "receipt differs"):
                bridge.read_capture_observations(root / "capture.json")

    def test_complete_pool_projection_keeps_all_live_nodes_and_rejects_broken_links(self):
        pool = 0x80800000
        addresses = [0x81000000 + index * bridge.OBJECT_BYTES for index in range(bridge.OBJECT_POOL_CAPACITY)]
        watch, budget = bridge.observer_plan([0, 4], pool, object_addresses=addresses, contact_slot=0)
        self.assertLessEqual(budget["maximum_bytes"], bridge.MEM1_DATAGRAM_BYTES)
        self.assertFalse(any(" 184 " in address for address in watch))
        values = {name: 0 for name in watch.values()}
        values["battle_pool"] = pool
        for label, offset in (("projectile", 0x75c40), ("program", 0x75ec8),
                              ("particle", 0x763d8), ("alternate", 0x76660)):
            values[label + "_head"] = values[label + "_tail"] = pool + offset
        # Every pool slot may be a live particle; the list is never a sampled prefix.
        sentinel = pool + 0x763d8
        values["particle_head"], values["particle_tail"] = addresses[0], addresses[-1]
        for index, address in enumerate(addresses):
            values[f"object_{index}_0"] = addresses[index - 1] if index else sentinel
            values[f"object_{index}_4"] = addresses[index + 1] if index + 1 < len(addresses) else sentinel
            values[f"object_{index}_10"] = (1 << 24) | (6 << 16)
            values[f"object_{index}_184"] = 0xdeadbeef  # Untyped parents are retained, never dereferenced.
        project = bridge.object_pool_projection(addresses, pool)
        row = project(values)
        self.assertEqual(bridge.tracked_list(row, "particle", bridge.OBJECT_POOL_CAPACITY),
                         (list(enumerate(addresses)), True))
        self.assertEqual(row["particle_0_184"], 0xdeadbeef)
        self.assertEqual(row["particle_0_parent_bank"], 0)
        self.assertFalse(any(key.startswith("object_") for key in row if key != "object_lists"))
        values["object_12_4"] = addresses[10]
        with self.assertRaisesRegex(ValueError, "cyclic"):
            project(values)

    def test_pending_clip_never_inherits_outgoing_controller_frame(self):
        row = dict(battle_actor_0_body_clip_word=43 << 8,
                   actor_0_animation_0_layer_word=6, actor_0_animation_0_resource=100,
                   actor_0_animation_0_frame_bits=0x41500000,
                   actor_0_animation_0_child=200, actor_0_animation_0_child_resource=300,
                   actor_0_animation_0_child_frame_bits=0)
        self.assertEqual(bridge.observed_body(row,0,{"43":300}), (43,0.0))
        row["actor_0_animation_0_child_resource"] = 301
        self.assertEqual(bridge.observed_body(row,0,{"43":300}), (43,None))
        override = dict(body_clip=43, clip=30, resource=100)
        self.assertEqual(bridge.observed_body(row,0,{"43":300},override), (43,None))
        row["actor_0_animation_0_child"] = 0
        self.assertEqual(bridge.observed_body(row,0,{"43":300},override), (30,13.0))
        row["actor_0_animation_0_resource"] = 101
        self.assertEqual(bridge.observed_body(row,0,{"43":300},override), (43,None))

        row.update(actor_0_animation_0_layer_word=0, actor_0_animation_1_layer_word=1,
                   actor_0_animation_2_layer_word=6, actor_0_animation_2_resource=300,
                   actor_0_animation_2_frame_bits=0x41500000)
        self.assertEqual(bridge.observed_body(row,0,{"43":300}), (43,13.0))

        binding = dict(actor=1, source_slot=4, resources={"36":100, "37":300},
                       observation=dict(record=400))
        row = dict(actor_4_secondary_record=400, actor_4_secondary_visibility=0x10000,
                   actor_4_raw_280=0, actor_4_secondary_0_layer=6,
                   actor_4_secondary_0_resource=100, actor_4_secondary_0_frame=0x41500000,
                   actor_4_secondary_0_child=200, actor_4_secondary_0_child_resource=300,
                   actor_4_secondary_0_child_frame=0)
        self.assertEqual(bridge.secondary_bodies.observe(row, binding),
                         dict(actor=1, clip=37, frame=0., visible=True))
        row["actor_4_secondary_record"] = 404
        with self.assertRaises(ValueError):
            bridge.secondary_bodies.observe(row, binding)

    def test_one_canonical_relative_event_stream(self):
        setup, row, _ = self.fixture()
        row["battle_combat_tick_word"], row["battle_presentation_tick_word"] = 20, 50
        result = self.trace(setup, [row])
        self.assertNotIn("semantic", result["assessment"])
        self.assertEqual(result["assessment"]["outcome"], "attack")
        event = next(e for e in result["events"] if e["kind"] == "started")
        self.assertEqual((event["clock"], event["presentation_clock"]), (0,0))
        self.assertNotIn("combat_tick", event)
        setup["support"] = dict(terminal="returned")
        after = dict(row, actor_0_condition_14=1 << 12,
                     battle_combat_tick_word=21, battle_presentation_tick_word=51)
        result = self.trace(setup, [row, after])
        change = next(event for event in result["events"] if event["kind"] == "condition_changed")
        self.assertEqual((change["owner"], change["target"], change["clock"]), (0,0,1))
        self.assertEqual((change["before"], change["after"]), ([], ["attack_up"]))
        self.assertTrue(bridge.support_outcomes.beneficial(change))
        self.assertFalse(bridge.support_outcomes.beneficial(dict(change, before=["attack_up"], after=[])))
        for condition in ("defense_halved", "recovery_down", "enchanted"):
            self.assertFalse(bridge.support_outcomes.beneficial(dict(change, before=[condition], after=[])))
            self.assertFalse(bridge.support_outcomes.beneficial(dict(change, before=[], after=[condition])))

        owner = setup["initial_battle"]["actors"][0]["address"]
        row["actor_0_support_target"] = owner
        released = dict(after, battle_actor_0_action=8 << 24,
                        actor_0_support_target=owner, actor_0_spell_support_target=owner)
        case = dict(id="party:2:35", capabilities=dict(spell=True))
        result = bridge.normalize(case, [row, released], setup, {}, dict(native=35), None)
        self.assertEqual((result["setup"]["target"], result["setup"]["requested_target"]), (0, 1))
        observation = result["setup"]["support_recipient_observation"]
        self.assertEqual((observation["recipient"], observation["clock"]), (0, 1))
        self.assertEqual((observation["support_pointer"], observation["spell_support_pointer"]), (owner, owner))
        repeated = bridge.normalize(case, [row, released], setup, {}, dict(native=35), None)
        self.assertEqual(repeated["setup"]["requested_target"], 1)
        for field, value in (("actor_0_support_target", owner + 4),
                             ("actor_0_spell_support_target", owner + 4),
                             ("actor_0_native", 36),
                             ("battle_actor_0_action", 5 << 24)):
            with self.assertRaises(ValueError):
                bridge.support_outcomes.cast_recipient([row, dict(released, **{field: value})],
                    setup, dict(native=35), result["frames"], result["events"])
        with self.assertRaises(ValueError):
            bridge.support_outcomes.cast_recipient([row, released], setup, dict(native=35),
                result["frames"], result["events"] + [next(event for event in result["events"]
                    if event["kind"] == "recovery_started")])
        enemy = setup["initial_battle"]["actors"][1]["address"]
        with self.assertRaises(ValueError):
            bridge.support_outcomes.cast_recipient([dict(row, actor_0_support_target=enemy),
                dict(released, actor_0_support_target=enemy, actor_0_spell_support_target=enemy)],
                setup, dict(native=35), result["frames"], result["events"])

        setup["actors"][1]["side"] = setup["actors"][0]["side"]
        selected = bridge.support_outcomes.cast_recipient(
            [dict(row, battle_actor_0_target=enemy),
             dict(released, battle_actor_0_target=enemy, actor_0_spell_support_target=enemy)],
            setup, dict(native=35), result["frames"], result["events"])
        self.assertEqual((selected["recipient"], selected["selected_field"]),
                         (1, "battle_actor_0_target"))

        setup, row, _ = self.fixture()
        after = dict(row, actor_4_condition_14=1 << 19,
                     battle_combat_tick_word=2, battle_presentation_tick_word=2)
        guaranteed = dict(clock=1, presentation_clock=1, kind="condition_changed",
                          owner=0, target=1, action=1, before=[], after=["accuracy_down"])
        with patch.object(bridge.support_outcomes, "seal_branches", return_value=([], [guaranteed])):
            result = self.trace(setup, [row, after])
        self.assertIn(guaranteed, result["events"])
        self.assertNotIn("chance_effects", result["setup"])
        self.assertNotIn("intended attack outcome was not attributed to the requested action",
                         result["incomplete_observations"])

        target = setup["initial_battle"]["actors"][1]["address"]
        row.update(actor_0_native=77, battle_actor_0_cast_clock_word=60,
                   battle_actor_0_target=target)
        after = dict(row, actor_4_condition_10=16, battle_actor_0_cast_clock_word=61,
                     battle_combat_tick_word=2, battle_presentation_tick_word=2)
        programs = [dict(owner=0, bank="seals", program=85,
                         attachment=dict(kind="actor", actor=actor)) for actor in (0,1)]
        frames = [dict(combat_tick=tick, presentation_tick=tick, programs=programs) for tick in (1,2)]
        events = [dict(kind="started", owner=0), dict(kind="payment", owner=0, amount=25)]
        banks = {1:dict(source_sha256="seals", path="/techniques.json")}
        branch, changes = bridge.support_outcomes.seal_branches(
            dict(native=77), [row, after], setup, frames, events, banks)
        self.assertEqual(branch, [])
        self.assertEqual(changes, [dict(kind="condition_changed", owner=0, target=1, action=1,
            before=[], after=["recovery_down"], clock=1, presentation_clock=1)])
        for invalid in (dict(battle_actor_0_target=0), dict(battle_actor_4_action=5 << 24),
                        dict(actor_4_condition_14=1), dict(actor_0_condition_14=1),
                        dict(battle_actor_0_cast_clock_word=62), dict(battle_actor_4_hp=99)):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                bridge.support_outcomes.seal_branches(
                    dict(native=77), [row, dict(after, **invalid)], setup, frames, events, banks)
        with self.assertRaises(ValueError):
            bridge.support_outcomes.seal_branches(dict(native=77), [row, after], setup, frames, [], banks)

    def test_pending_selection_is_not_committed_admission(self):
        setup, bound, _ = self.fixture()
        pending = copy.deepcopy(bound)
        pending["battle_actor_0_action"] = 5 << 24
        unadmitted = self.trace(setup, [pending])
        self.assertEqual(unadmitted["events"], [])
        self.assertIn("requested action admission was not observed", unadmitted["incomplete_observations"])
        bound["battle_combat_tick_word"] = bound["battle_presentation_tick_word"] = 2
        result = self.trace(setup, [pending, bound])
        self.assertEqual(result["frames"][0]["combat_tick"], 2)
        self.assertEqual(result["setup"]["input"]["request_after_updates"], 1)
        self.assertEqual(result["setup"]["input"]["bound_admission_after_updates"], 2)
        substituted = copy.deepcopy(setup)
        substituted["input"]["selection_native"] = 34
        selected = dict(pending, actor_0_native=34)
        result = bridge.normalize(dict(id="party:2:35", character=2),
            [selected, bound], substituted, {}, dict(native=35), None)
        self.assertEqual(result["setup"]["input"]["request_after_updates"], 1)
        self.assertEqual(result["setup"]["input"]["bound_admission_after_updates"], 2)
        self.assertEqual(result["frames"][0]["combat_tick"], 2)
        setup["actors"][0].update(position=[0.0, 0.0, 0.0], heading=0.0)
        pending["actor_0_selected"] = bound["actor_0_selected"] = 123
        pending["battle_actor_0_heading_bits"] = 0x41100000  # Nine degrees.
        bound["battle_actor_0_heading_bits"] = 0x41900000
        self.assertEqual(bridge.enemy_pending_entry([pending, bound], setup, {"selected": 123}, 1), 1)
        pending["battle_actor_0_x_bits"] = 0x3f800000
        with self.assertRaises(ValueError):
            bridge.enemy_pending_entry([pending, bound], setup, {"selected": 123}, 1)
        hovering = [dict(row, battle_actor_0_heading_bits=0,
                         battle_actor_0_x_bits=0, battle_actor_0_y_bits=0x3f800000,
                         actor_0_condition_5c=3, actor_0_raw_280=0x200000)
                    for row in (pending, bound)]
        self.assertEqual(bridge.enemy_pending_entry(hovering, setup, {"selected": 123}, 1), 1)
        hovering[0]["battle_actor_0_x_bits"] = 0x3f800000
        with self.assertRaises(ValueError):
            bridge.enemy_pending_entry(hovering, setup, {"selected": 123}, 1)
        walking = [dict(pending, battle_actor_0_action=4 << 24, battle_actor_0_heading_bits=0),
                   dict(bound, battle_actor_0_x_bits=0x40000000, battle_actor_0_heading_bits=0)]
        self.assertEqual(bridge.enemy_pending_entry(walking, setup, {"selected": 123}, 1), 1)
        moved_before_selection = dict(walking[0], battle_actor_0_action=3 << 24)
        with self.assertRaises(ValueError):
            bridge.enemy_pending_entry([moved_before_selection, *walking], setup, {"selected": 123}, 2)
        entry_heading = dict(walking[0], battle_combat_tick_word=0,
                             battle_actor_0_action=3 << 24, battle_actor_0_heading_bits=0x41100000)
        initialized = dict(entry_heading, battle_combat_tick_word=1)
        selected = dict(walking[0], battle_combat_tick_word=2,
                        battle_actor_0_heading_bits=0x41100000)
        bound_after_entry = dict(walking[1], battle_combat_tick_word=3)
        self.assertEqual(bridge.enemy_pending_entry(
            [entry_heading, initialized, selected, bound_after_entry], setup, {"selected": 123}, 3), 2)
        restored = [entry_heading, dict(initialized, battle_actor_0_heading_bits=0),
                    dict(selected, battle_actor_0_heading_bits=0), bound_after_entry]
        self.assertEqual(bridge.enemy_pending_entry(restored, setup, {"selected": 123}, 3), 2)
        restored.insert(2, dict(initialized, battle_combat_tick_word=2))
        with self.assertRaisesRegex(ValueError, "facing changed"):
            bridge.enemy_pending_entry(restored, setup, {"selected": 123}, 4)
        carried = copy.deepcopy(setup)
        carried["source_actor_map"][0]["source_slot"] = 4
        carried["recipient_input"] = dict(mode="attack", target_slot=0)
        carried["assistance"] = dict(preparation=dict(prerequisites=dict(release_host=0)))
        def carried_row(tick, action, flags, host_hp):
            return dict(battle_combat_tick_word=tick, battle_actor_4_action=action << 24,
                actor_4_selected=123, actor_4_raw_280=flags, battle_actor_0_hp=host_hp,
                battle_actor_0_action=(3 if host_hp == 100 else 9) << 24)
        released = [carried_row(1, 3, 0x80000, 100), carried_row(2, 8, 0, 99),
                    carried_row(3, 2, 0, 99), carried_row(4, 3, 0, 99),
                    carried_row(5, 4, 0, 99), carried_row(6, 10, 0, 99),
                    carried_row(7, 5, 0, 99), carried_row(8, 5, 0, 99)]
        self.assertEqual(bridge.enemy_pending_entry(released, carried, {"selected": 123}, 7), 5)
        released[1]["battle_actor_0_hp"] = 100
        with self.assertRaises(ValueError):
            bridge.enemy_pending_entry(released, carried, {"selected": 123}, 7)
        setup["target"] = 0
        bound["battle_actor_0_target"] = setup["initial_battle"]["actors"][1]["address"]
        result = bridge.normalize(dict(id="enemy:209:1"), [bound], setup, {}, {"selected": 123}, None)
        self.assertEqual((result["setup"]["target"], result["setup"]["requested_target"]), (1, 0))
        self.assertEqual(result["setup"]["action_target_observation"]["combat_tick"], 2)
        with self.assertRaisesRegex(ValueError, "target is not an observed actor"):
            bridge.bind_action_target(dict(bound, battle_actor_0_target=1), setup)
        item_setup = copy.deepcopy(setup)
        item_setup.update(target=1, requested_target=1)
        item_row = dict(bound, actor_0_support_target=setup["initial_battle"]["actors"][0]["address"])
        item_events = [[28, dict(use_item=dict(item=4, party=False))]]
        self.assertEqual(bridge.bind_action_target(item_row, item_setup, item_events), 0)
        self.assertEqual(item_setup["requested_target"], 1)
        self.assertEqual(item_row["battle_actor_0_target"], bound["battle_actor_0_target"])
        with self.assertRaisesRegex(ValueError, "not an allied actor"):
            bridge.bind_action_target(dict(item_row, actor_0_support_target=bound["battle_actor_0_target"]),
                                      item_setup, item_events)
        self.assertEqual(bridge.bind_action_target(item_row, item_setup,
            [[28, dict(use_item=dict(item=39, party=True))]]), 1)
        self.assertEqual(bridge.bind_action_target(item_row, item_setup, allied_spell=True), 0)
        with self.assertRaisesRegex(ValueError, "not an allied actor"):
            bridge.bind_action_target(dict(item_row, actor_0_support_target=bound["battle_actor_0_target"]),
                                      item_setup, allied_spell=True)
        earlier = dict(pending, actor_0_selected=122, battle_actor_0_action=(5 << 24) | (1 << 16))
        result = bridge.normalize(dict(id="enemy:209:1"), [earlier, bound], setup, {}, {"selected": 123}, None)
        self.assertIn("owner admitted another action before the requested row; single-action fixture is incomplete",
                      result["incomplete_observations"])

    def test_circular_sentinel_is_not_a_live_particle(self):
        _, row, _ = self.fixture()
        self.assertEqual(bridge.tracked_list(row, "particle", bridge.PARTICLES), ([], True))
        row["particle_0_address"] = 0x81200000
        row["particle_0_next"] = row["battle_pool"] + 0x763d8
        self.assertEqual(bridge.tracked_list(row, "particle", bridge.PARTICLES), ([(0, 0x81200000)], True))
        head = row["battle_pool"] + 0x763d8
        fixed = dict(battle_pool=row["battle_pool"], particle_head=100, particle_tail=200,
                     particle_0_prev=head, particle_0_next=200, particle_0_10=1 << 24,
                     particle_1_prev=100, particle_1_next=head, particle_1_10=1 << 24,
                     particle_2_prev=987, particle_2_next=654, particle_2_10=1 << 24)
        self.assertEqual(bridge.observation_counts(fixed)["particle"], 3)
        self.assertEqual(bridge.tracked_list(fixed, "particle", 3, [100,200,300]),
                         ([(0,100),(1,200)], True))
        self.assertFalse(bridge.tracked_list(fixed, "particle", 3)[1])
        fixed["particle_1_next"] = 400
        self.assertFalse(bridge.tracked_list(fixed, "particle", 3, [100,200,300])[1])
        fixed["particle_1_next"], fixed["particle_1_prev"] = head, 300
        self.assertFalse(bridge.tracked_list(fixed, "particle", 3, [100,200,300])[1])
        head = row["battle_pool"] + 0x763d8
        joined = dict(battle_pool=row["battle_pool"])
        for index, address, previous, following in ((0, 100, head, 200), (1, 200, 100, 300),
                                                     (2, 200, 100, 300), (3, 300, 200, head)):
            joined.update({f"particle_{index}_{key}": value for key, value in
                           (("address", address), ("prev", previous), ("next", following), ("10", 1 << 24))})
        self.assertEqual(bridge.tracked_list(joined, "particle", 4), ([(0,100),(1,200),(3,300)], True))
        joined["particle_2_10"] = 0
        self.assertFalse(bridge.tracked_list(joined, "particle", 4)[1])
        joined["particle_2_10"] = 1 << 24
        joined["particle_1_2c"], joined["particle_2_2c"] = 44, 45
        self.assertFalse(bridge.tracked_list(joined, "particle", 4)[1])
        joined["particle_2_2c"] = 44
        self.assertEqual(bridge.tracked_list(joined, "particle", 4), ([(0,100),(1,200),(3,300)], True))
        joined["particle_3_prev"] = 100
        self.assertFalse(bridge.tracked_list(joined, "particle", 4)[1])

    def test_pending_projectile_payload_is_not_a_birth(self):
        setup, pending, _ = self.fixture()
        pending.update(projectile_0_address=0x81200000,
                       projectile_0_next=pending["battle_pool"] + 0x75c40,
                       projectile_0_278=13 << 16)
        ready = copy.deepcopy(pending)
        ready.update(projectile_0_14=1 << 24, projectile_0_278=1 << 16,
                     projectile_0_28=0x81000000,projectile_0_2c=0x81002000,
                     battle_combat_tick_word=2,battle_presentation_tick_word=2)
        trace = self.trace(setup,[pending,ready])
        self.assertEqual(trace["frames"][0]["projectiles"],[])
        births=[e for e in trace["events"] if e["kind"]=="projectile_started"]
        self.assertEqual([(e["clock"],e["projectile"]) for e in births],[(1,1)])
        self.assertEqual(trace["frames"][1]["projectiles"][0]["id"],1)

    def test_numeric_overlay_is_not_an_effect_program(self):
        setup,row,_=self.fixture()
        row.update(program_0_address=0x81200000,program_0_next=row["battle_pool"]+0x75ec8,
                   program_0_10=0x06041805)
        trace=self.trace(setup,[row])
        self.assertFalse(any("program" in gap for gap in trace["incomplete_observations"]))

    def test_alternate_program_uses_the_same_exact_bank_binding(self):
        setup,row,_=self.fixture()
        row.update(alternate_0_address=0x81200000,alternate_0_next=row["battle_pool"]+0x76660,
                   alternate_0_60=0x81080000,alternate_0_6c=0x81080100,
                   alternate_0_2c=0x81000000)
        bank=dict(source_sha256="pinned",roots=[0x100],metadata=dict(programs=[[]]))
        trace=bridge.normalize(dict(id="party:2:35"),[row],setup,{0x81080000:bank},dict(native=35),None)
        program=next(e for e in trace["events"] if e["kind"]=="program_started")
        self.assertEqual((program["owner"],program["bank"],program["program"]),(0,"pinned",0))
        raw = bytearray(b"ef1\0" + bytes(24))
        raw[4] = 1
        struct.pack_into(">6H", raw, 8, 20, 22, 20, 20, 20, 20)
        for first, expected in (([0,0,254,0,0,0], [0]),
                                ([0,1,254,0,0,0], []), ([0,0,0,0,0,0], [])):
            raw[22:28] = bytes(first)
            metadata = dict(source_sha256=bridge.hashlib.sha256(raw).hexdigest(), programs=[[]])
            with patch.object(bridge, "read", return_value=metadata):
                authored = bridge.effect_bank(raw, "bank.json")
            self.assertEqual(authored["immediate_end_programs"], expected)
        for attachment, origin, generations in ((0, 0, 1), (0, 0x3f800000, 2),
                                                 (0x81000000, 0x3f800000, 1),
                                                 (None, 0x3f800000, 1)):
            before = dict(row, alternate_0_44=attachment)
            after = dict(before, alternate_0_48=origin, battle_combat_tick_word=2,
                         battle_presentation_tick_word=2)
            trace=bridge.normalize(dict(id="party:2:35"),[before,after],setup,
                                   {0x81080000:bank},dict(native=35),None)
            starts=[e for e in trace["events"] if e["kind"]=="program_started"]
            self.assertEqual(len(starts), generations)
            self.assertEqual(trace["frames"][-1]["programs"][0]["id"], generations)
        for lifecycle, generations in ((1, 1), (2, 2)):
            before = dict(row, alternate_0_14=lifecycle << 24, alternate_0_5c=1)
            after = dict(before, alternate_0_14=1 << 24, battle_combat_tick_word=2,
                         battle_presentation_tick_word=2)
            trace=bridge.normalize(dict(id="party:2:35"),[before,after],setup,
                                   {0x81080000:bank},dict(native=35),None)
            self.assertEqual(sum(e["kind"] == "program_started" for e in trace["events"]), generations)

    def test_execution_without_intended_hit_is_incomplete(self):
        setup,row,_=self.fixture()
        self.assertIn("intended attack outcome was not attributed to the requested action",
                      self.trace(setup,[row])["incomplete_observations"])

    def test_current_contact_queue_accepts_equal_rolls_but_not_stale_contexts(self):
        setup, row, target = self.fixture()
        owner, context = 0x81000000, 0x81001184
        row.update(contact_queue_counts=1 << 16, actor_4_fresh_contact_word=1 << 24,
                   actor_0_contact_target=target, contact_queue_0_0_owner=owner,
                   contact_queue_0_0_context=context, actor_0_contact_0_rule=0x81080000,
                   actor_0_contact_0_template=0x81080100, actor_0_contact_0_nominal=20)
        for _ in range(2):
            self.assertEqual(bridge.queued_contact(row, setup, 0, 1)["context"], context)
        row["actor_4_fresh_contact_word"] = 0
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1))
        row["actor_4_fresh_contact_word"] = 1 << 24
        row.update(contact_queue_counts=0, contact_queue_0_0_context=0xffffffff)
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1))
        row["contact_queue_counts"] = 1 << 16
        with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, "bound actor context"):
            bridge.queued_contact(row, setup, 0, 1)

        # A queue's raw pointer must join a currently observed initialized
        # projectile. Its owner's result remains valid on equal rolls.
        node, sentinel = 0x81090000, row["battle_pool"] + 0x75c40
        row.update(projectile_0_address=node, projectile_0_next=sentinel,
                   projectile_0_14=1 << 24, projectile_0_28=owner,
                   projectile_0_16c=0x81080000, projectile_0_170=0x81080100,
                   projectile_0_19c=20, contact_queue_0_0_projectile=node + 0x28,
                   contact_queue_0_0_context=node + 0x168)
        for _ in range(2):
            self.assertEqual(bridge.queued_contact(row, setup, 0, 1)["projectile"], node + 0x28)
        row["projectile_0_14"] = 0
        with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, "live projectile"):
            bridge.queued_contact(row, setup, 0, 1)
        row["projectile_0_14"] = 1 << 24
        second = node + 0x300
        row.update(projectile_0_next=second, projectile_1_address=second, projectile_1_next=sentinel,
                   projectile_1_14=1 << 24, projectile_1_28=owner,
                   projectile_1_16c=0x81080200, projectile_1_170=0x81080300,
                   projectile_1_19c=20, contact_queue_counts=2 << 16,
                   contact_queue_0_1_owner=owner, contact_queue_0_1_context=second + 0x168,
                   contact_queue_0_1_projectile=second + 0x28)
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1))
        previous = row.copy()
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, previous))
        # Equal repeated rolls remain attributable when all other queued
        # projectiles have an observed cleared collision result.
        row["projectile_0_280"] = previous["projectile_0_280"] = 1
        self.assertEqual(bridge.queued_contact(row, setup, 0, 1, previous)["projectile"], node + 0x28)
        row["projectile_1_280"] = 1
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, previous))
        repeated = dict(row, battle_combat_tick_word=2,
                        projectile_0_278=8 << 16, projectile_1_278=14 << 16,
                        projectile_0_last_target=target, projectile_1_last_target=target,
                        projectile_0_rule_timing=5 << 24, projectile_1_rule_timing=5 << 24,
                        projectile_0_cooldown_0=5 << 24, projectile_1_cooldown_0=2 << 24)
        before_repeat = dict(repeated, battle_combat_tick_word=1,
                             projectile_0_278=7 << 16, projectile_1_278=13 << 16,
                             projectile_0_cooldown_0=1 << 24, projectile_1_cooldown_0=3 << 24)
        self.assertEqual(bridge.queued_contact(repeated, setup, 0, 1, before_repeat)["projectile"],
                         node + 0x28)
        for current in (dict(repeated, projectile_0_last_target=owner),
                        dict(repeated, projectile_0_cooldown_0=0),
                        dict(repeated, battle_combat_tick_word=3)):
            self.assertIsNone(bridge.queued_contact(current, setup, 0, 1, before_repeat))
        before_repeat["projectile_1_cooldown_0"] = 1 << 24
        repeated["projectile_1_cooldown_0"] = 5 << 24
        self.assertIsNone(bridge.queued_contact(repeated, setup, 0, 1, before_repeat))
        row["projectile_0_280"] = row["projectile_1_280"] = previous["projectile_0_280"] = 0
        row["projectile_1_19c"] = 21
        self.assertEqual(bridge.queued_contact(row, setup, 0, 1, previous)["projectile"], second + 0x28)
        row["projectile_0_19c"] = 22
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, previous))
        row["projectile_0_19c"] = 20
        previous = dict(row, contact_queue_counts=1 << 16, battle_combat_tick_word=1,
                        projectile_1_19c=0, projectile_1_278=1 << 16)
        row.update(battle_combat_tick_word=2, projectile_1_278=2 << 16)
        self.assertEqual(bridge.queued_contact(row, setup, 0, 1, previous)["projectile"],
                         second + 0x28)
        for field, value in (("projectile_1_16c", 0x81080400),
                             ("projectile_1_170", 0x81080500),
                             ("projectile_1_14", 2 << 24),
                             ("projectile_1_278", 3 << 16),
                             ("battle_combat_tick_word", 0)):
            self.assertIsNone(bridge.queued_contact(row, setup, 0, 1,
                                                   dict(previous, **{field: value})))
        row["contact_queue_0_1_context"] += 4
        with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, "live projectile"):
            bridge.queued_contact(row, setup, 0, 1, previous)

        # A new pair of identical-rule body contacts must not discard the
        # changed first slot beside an unchanged persistent projectile.
        row.update(contact_queue_counts=1 << 16, projectile_0_next=sentinel,
                   actor_0_contact_0_rule=0, actor_0_contact_0_template=0,
                   actor_0_contact_0_nominal=0)
        previous = row.copy()
        row.update(contact_queue_counts=3 << 16,
                   battle_combat_tick_word=previous["battle_combat_tick_word"] + 1)
        for index in range(2):
            row.update({f"contact_queue_0_{index + 1}_owner": owner,
                        f"contact_queue_0_{index + 1}_context": context + index * 0x38,
                        f"contact_queue_0_{index + 1}_projectile": 0,
                        f"actor_0_contact_{index}_rule": 0x81080400,
                        f"actor_0_contact_{index}_template": 0x81080500,
                        f"actor_0_contact_{index}_nominal": 30 if index == 0 else 0})
        self.assertEqual(bridge.queued_contact(row, setup, 0, 1, previous)["context"], context)
        row["actor_0_contact_1_nominal"] = 30
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, previous))
        row["actor_0_contact_1_nominal"] = 0
        previous.update(actor_0_contact_0_rule=0x81080400,
                        actor_0_contact_0_template=0x81080600,
                        actor_0_contact_0_nominal=45)
        self.assertEqual(bridge.queued_contact(row, setup, 0, 1, previous)["context"], context)
        for field, value in (("actor_0_contact_0_nominal", None),
                             ("actor_0_contact_0_nominal", 30),
                             ("battle_combat_tick_word", row["battle_combat_tick_word"] - 2)):
            self.assertIsNone(bridge.queued_contact(row, setup, 0, 1,
                                                   dict(previous, **{field: value})))

        # Body-center contacts use their own live pool node, with a copied
        # context and an exact binding to the immutable authored template.
        setup, row, target = self.fixture()
        binding = dict(rule=0x81080000, template=0x81080100,
                       rule_flags_element=0x200023, rule_power=0x10032, contact_kind=1)
        setup["detached_contacts"] = dict(records=[binding])
        row.update(contact_queue_counts=1 << 16, actor_4_fresh_contact_word=1 << 24,
                   actor_0_contact_target=target, contact_queue_0_0_owner=owner,
                   contact_queue_0_0_context=node + 0x28, program_0_address=node,
                   program_0_next=row["battle_pool"] + 0x75ec8, program_0_10=5 << 24,
                   program_0_14=1 << 24, program_0_18=owner, program_0_2c=binding["rule"],
                   program_0_30=binding["template"], program_0_5c=20, program_0_278=(1 << 16) | 2)
        for _ in range(2):
            contact = bridge.queued_contact(row, setup, 0, 1)
            self.assertEqual((contact["context"], contact["nominal"]), (node + 0x28, 20))
            self.assertEqual(contact["rule_power"], binding["rule_power"])
        for field, value in (("program_0_18", target), ("program_0_30", binding["template"] + 4),
                             ("program_0_14", 0), ("program_0_278", (4 << 16) | 2),
                             ("contact_queue_0_0_context", node + 0x2c)):
            with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                bridge.queued_contact(dict(row, **{field: value}), setup, 0, 1)
        self.assertFalse(any("standalone contact" in reason or "unclassified node" in reason
                             for reason in self.trace(setup, [row])["incomplete_observations"]))

        # A new detached contact can share a queue with an unchanged retiring
        # contact. Its initialized result, not its unspecified nominal baseline,
        # proves the hit; repeated registrations still identify one context.
        row.update(battle_combat_tick_word=10, program_0_278=(2 << 16) | 2,
                   program_0_5c=0xb33bbd2e, program_0_280=0)
        previous = row.copy()
        second_binding = dict(binding, template=binding["template"] + 32)
        setup["detached_contacts"]["records"].append(second_binding)
        fresh = node + 0x300
        row.update(battle_combat_tick_word=11, contact_queue_counts=3 << 16,
                   program_0_address=fresh, program_0_next=node,
                   program_0_30=second_binding["template"], program_0_5c=31,
                   program_0_280=1 << 8)
        for key, value in previous.items():
            if key.startswith("program_0_"):
                row[key.replace("program_0_", "program_1_", 1)] = value
        row.update(program_1_14=2 << 24, program_1_278=(3 << 16) | 2)
        for index in (1, 2):
            row.update({f"contact_queue_0_{index}_owner": owner,
                        f"contact_queue_0_{index}_context": fresh + 0x28,
                        f"contact_queue_0_{index}_projectile": 0})
        contact = bridge.queued_contact(row, setup, 0, 1, previous)
        self.assertEqual((contact["context"], contact["template"], contact["nominal"]),
                         (fresh + 0x28, second_binding["template"], 31))
        for result in (0, 2 << 8, None):
            changed = dict(row, program_0_280=result)
            self.assertIsNone(bridge.queued_contact(changed, setup, 0, 1, previous))
        self.assertIsNotNone(bridge.queued_contact(dict(row, program_0_280=17 << 8),
                                                  setup, 0, 1, previous))
        for field, value in (("battle_combat_tick_word", 12),
                             ("actor_4_fresh_contact_word", 0),
                             ("actor_0_contact_target", owner)):
            self.assertIsNone(bridge.queued_contact(dict(row, **{field: value}),
                                                   setup, 0, 1, previous))
        with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
            bridge.queued_contact(row, setup, 0, 1, dict(previous, program_0_next=0))

        prior_live = previous.copy()
        prior_live["program_0_next"] = fresh
        for key, value in row.items():
            if key.startswith("program_0_"):
                prior_live[key.replace("program_0_", "program_1_", 1)] = value
        prior_live["program_1_next"] = previous["program_0_next"]
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, prior_live))

        third = fresh + 0x300
        third_binding = dict(binding, template=binding["template"] + 64)
        setup["detached_contacts"]["records"].append(third_binding)
        for key, value in list(row.items()):
            if key.startswith("program_0_"):
                row[key.replace("program_0_", "program_2_", 1)] = value
        row.update(program_1_next=third, program_2_address=third,
                   program_2_next=previous["program_0_next"],
                   program_2_30=third_binding["template"],
                   contact_queue_0_2_context=third + 0x28)
        self.assertIsNone(bridge.queued_contact(row, setup, 0, 1, previous))

    def test_pool_compaction_retains_dynamic_suffixes(self):
        ordinary=bridge.locations([0,4],4,4,4)
        compact=bridge.locations([0,4],4,4,4,pool_base=0x80900000)
        self.assertEqual(compact["809763dc 4 170"], ordinary[bridge.POOL + " 763dc 4 170"])
        self.assertCountEqual(compact.values(), ordinary.values())

    def test_authored_birth_uv_override_requires_exact_supported_bytes(self):
        raw=bytes(24)
        bank=dict(declarations=[raw],roots=[100],metadata=dict(programs=[[
            dict(operation=dict(kind="spawn",particle=0,birth=dict(uv=[dict(min=288,max=288),None,None,None],geometry_count=dict(min=4,max=4))))]]))
        live=bytearray(raw);live[8:10]=(288).to_bytes(2,"big");live[18]=4
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])

        live[18]=5
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth = bank["metadata"]["programs"][0][0]["operation"]["birth"]
        live[18] = 4
        live[20] = 0x10
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth["cull_back"] = True
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[20] = 0
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth.pop("cull_back")
        live[23] = 0x20
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth["ribbon_depth"] = True
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[23] = 0
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth.pop("ribbon_depth")
        birth["uv"][0] = dict(min=1,max=43,step=6)
        live[18] = 4
        for value in (1,7,31,37,43):
            live[8:10] = value.to_bytes(2,"big")
            self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        for value in (0,2,42,44):
            live[8:10] = value.to_bytes(2,"big")
            self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        birth["uv"][0] = dict(min=1,max=44,step=6,reverse=True)
        live[8:10] = (38).to_bytes(2,"big")
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[8:10] = (37).to_bytes(2,"big")
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        live[8:10] = (38).to_bytes(2,"big")
        birth["phase"] = dict(min=22,max=22)
        live[19] = 22
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[19] = 23
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        live[19] = 22
        bank["metadata"]["programs"][0][0]["operation"]["blend"] = 1
        live[1] = 1
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[1] = 0
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        live[1] = 1
        birth["lifetime"] = dict(min=51,max=65,step=1,draw=9)
        birth["geometry_count"] = dict(min=8,max=22,step=1,draw=9)
        live[16:18], live[18] = (62).to_bytes(2,"big"), 20
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[16:18] = (63).to_bytes(2,"big")
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        live[18] = 21
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        palette = bytearray(352)
        palette[0x32] = 255
        bank = dict(declarations=[palette], roots=[100], metadata=dict(programs=[[
            dict(operation=dict(kind="spawn", particle=0, birth=dict(
                uv=[dict(min=1,max=129,step=64,draw=1)],
                palette=dict(min=1,max=5,step=2,draw=1))))]]))
        live = bytearray(palette[:24])
        live[8:10] = (65).to_bytes(2,"big")
        for value, expected in ((3,[0]), (1,[]), (2,[]), (5,[]), (7,[])):
            live[3] = value
            self.assertEqual(bridge.declaration_matches(bank,live,100),expected)
        model = bytearray(352)
        model[0], model[212] = 3, 3
        bank = dict(declarations=[model], roots=[100], metadata=dict(programs=[[
            dict(operation=dict(kind="spawn", particle=0, birth=dict(model=dict(min=3,max=4,step=1))))]]))
        for variant in (3,4):
            self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
                model_binding=variant << 24,initialized=True,age=1),[0])
        for variant in (2,5):
            self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
                model_binding=variant << 24,initialized=True,age=1),[])
        bank["metadata"]["programs"][0][0]["operation"]["birth"]["model"] = dict(min=3,max=5,step=2)
        self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
            model_binding=4 << 24,initialized=True,age=1),[])
        birth = bank["metadata"]["programs"][0][0]["operation"]["birth"]
        birth["animation"] = dict(model=3, clip=2, blend_ticks=0, rate=0.4, repeat=True)
        self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
            model_binding=(3 << 24) | 0x100,initialized=True,age=1),[0])
        for value in (3 << 24, (3 << 24) | 0x200, (5 << 24) | 0x100):
            self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
                model_binding=value,initialized=True,age=1),[])
        spawn = bank["metadata"]["programs"][0][0]
        spawn["at"] = 20
        animation = birth.pop("animation")
        motion = dict(at=20, operation=dict(kind="set_motion", spawn=0, animation=animation))
        bank["metadata"]["programs"][0].append(motion)
        self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
            model_binding=(3 << 24) | 0x100,initialized=True,age=1),[0])
        motion["at"] = 21
        self.assertEqual(bridge.declaration_matches(bank,model[:24],100,
            model_binding=(3 << 24) | 0x100,initialized=True,age=1),[])
        descriptor = 0x81330000
        words = {0xfc: 0x100, 0x168: descriptor,
                 0x100: descriptor + 0x640 + 2 * 20, 0x108: 0x40000000,
                 0x134: descriptor + 0x640, 0x13c: 0x40e00000}
        binding = dict(animation=None, program_animation=True)
        self.assertEqual(bridge.observed_effect_animation(words.__getitem__, binding), (2, 2.))
        words[0xfc], words[0x100] = 0, 0
        self.assertEqual(bridge.observed_effect_animation(words.__getitem__, binding), (0, 7.))
        words[0x134] += 4
        with self.assertRaises(ValueError):
            bridge.observed_effect_animation(words.__getitem__, binding)
        words[0x134] -= 4
        words[0x13c] = 0x7fc00000
        with self.assertRaises(ValueError):
            bridge.observed_effect_animation(words.__getitem__, binding)
        words[0xfc] = 0x200
        with self.assertRaises(ValueError):
            bridge.observed_effect_animation(words.__getitem__, binding)
        words[0x100], words[0x108] = descriptor + 0x640, 0x3f000000
        self.assertEqual(bridge.observed_effect_animation(words.__getitem__, dict(animation=0)), (0, 0.5))
        with self.assertRaises(ValueError):
            bridge.observed_effect_animation(words.__getitem__, dict(animation=1))
        words[0xfc], words[0x168] = 2 << 24, descriptor + 0x7e0
        self.assertEqual(bridge.observed_effect_animation(words.__getitem__,
            dict(slot=1, animation=0)), (0, 0.5))
        with self.assertRaises(ValueError):
            bridge.observed_effect_animation(words.__getitem__, dict(slot=2, animation=0))
        words[0xfc], words[0x134] = 0, 0
        self.assertEqual(bridge.observed_effect_animation(words.__getitem__, binding), (None, None))

    def test_authored_uv_sums_preserve_ranges_and_shared_draws(self):
        terms = [dict(min=1, max=97, step=32, draw=1),
                 dict(min=0, max=96, step=32, draw=2)]
        birth = dict(sums=[dict(field=dict(kind="uv", axis=0), terms=terms)])
        bank = dict(declarations=[bytes(24)], roots=[100], metadata=dict(programs=[[
            dict(operation=dict(kind="spawn", particle=0, birth=birth))]]))
        live = bytearray(24)
        for value in (1, 33, 97, 129, 193):
            live[8:10] = value.to_bytes(2, "big")
            self.assertEqual(bridge.declaration_matches(bank, live, 100), [0])
        for value in (0, 2, 192, 225):
            live[8:10] = value.to_bytes(2, "big")
            self.assertEqual(bridge.declaration_matches(bank, live, 100), [])
        terms[1]["draw"] = 1
        for value, expected in ((33, []), (65, [0]), (97, []), (193, [0])):
            live[8:10] = value.to_bytes(2, "big")
            self.assertEqual(bridge.declaration_matches(bank, live, 100), expected)
        raw = bytearray(352)
        raw[0] = 5
        rectangle = struct.pack(">4H", 1, 161, 30, 30)
        bank["declarations"] = [raw]
        bank["uv_descriptors"] = bytes([4, 0]) + rectangle
        birth["uv"] = [dict(min=97, max=97)]
        live = raw[:24]
        live[8:16] = rectangle
        self.assertEqual(bridge.declaration_matches(bank, live, 100, initialized=True, age=1), [0])
        live[10:12] = (129).to_bytes(2, "big")
        self.assertEqual(bridge.declaration_matches(bank, live, 100, initialized=True, age=1), [])

    def test_identical_prefix_is_disambiguated_only_by_observed_parent_program(self):
        raw = bytes(24)
        bank = dict(declarations=[raw,raw], roots=[100], metadata=dict(programs=[[
            dict(operation=dict(kind="spawn",particle=1))]]))
        self.assertEqual(bridge.declaration_matches(bank,raw,100),[1])
        self.assertEqual(bridge.declaration_matches(bank,raw,999),[0,1])
        boned = copy.deepcopy(bank)
        boned["metadata"]["actors"] = [dict(template=dict(owner_bone=bone)) for bone in (76, 98)]
        for bone, expected in ((None, [0, 1]), (76, [0]), (98, [1]), (111, [])):
            self.assertEqual(bridge.declaration_matches(boned, raw, 999, owner_bone=bone), expected)
        boned["metadata"]["programs"][0][0]["operation"]["birth"] = dict(attachment_bone=11)
        self.assertEqual(bridge.declaration_matches(boned, raw, 100, owner_bone=11), [1])
        self.assertEqual(bridge.declaration_matches(boned, raw, 100, owner_bone=98), [])
        timed = dict(bank, metadata=dict(programs=[[
            dict(at=5, operation=dict(kind="spawn", particle=0)),
            dict(at=8, operation=dict(kind="spawn", particle=1))]]))
        for tick, expected in ((None, [0,1]), (5, [0]), (8, [1]), (6, [0,1])):
            self.assertEqual(bridge.declaration_matches(timed,raw,100,birth_tick=tick), expected)
        timed["metadata"]["programs"][0][1]["at"] = 5
        self.assertEqual(bridge.declaration_matches(timed,raw,100,birth_tick=5), [0,1])
        self.assertEqual(bridge.declaration_matches(timed,raw,999,birth_tick=5), [0,1])
        attached = copy.deepcopy(timed)
        attached["metadata"]["actors"] = [dict(binding=dict(attachment=dict(retained=0, joint=joint)))
                                            for joint in (2, 3)]
        for attachment, expected in (((2, 0), [0]), ((3, 0), [1]), ((2, 1), [])):
            self.assertEqual(bridge.declaration_matches(attached, raw, 100,
                model_attachment=attachment), expected)
        mirrored = copy.deepcopy(timed)
        mirrored["metadata"]["actors"] = [dict(template=dict(state=dict(
            offset=[16 * sign, 0, 0], velocity=[sign, 0, 0]))) for sign in (-1, 1)]
        self.assertEqual(bridge.declaration_matches(mirrored, raw, 100, initialized=True, age=1,
            offset=[0, 0, 17], heading=-90), [1])
        self.assertEqual(bridge.declaration_matches(mirrored, raw, 100, initialized=True, age=1,
            offset=[0, 0, -17], heading=-90), [0])
        for command in mirrored["metadata"]["programs"][0]:
            command["operation"]["birth"] = dict(uv=[dict(min=0, max=0)])
        self.assertEqual(bridge.declaration_matches(mirrored, raw, 100, initialized=True, age=1,
            offset=[0, 0, 17], heading=-90), [1])
        for command in mirrored["metadata"]["programs"][0]:
            command["operation"]["birth"] = dict(offset=[dict(min=-30, max=30), None, None])
        self.assertEqual(bridge.declaration_matches(mirrored, raw, 100, initialized=True, age=1,
            offset=[0, 0, 17], heading=-90), [0, 1])
        timed["metadata"]["actors"] = [dict(template=dict(state=dict(geometry=dict(Size=dict(
            value=[0, 0, width], velocity=[10, 0, 0], acceleration=[0, 0, 0])))))
            for width in (128, 224)]
        timed["declarations"] = [raw + bytes(152) + struct.pack(">3f", 0, 0, width)
                                 for width in (128, 224)]
        self.assertEqual(bridge.declaration_matches(timed, raw, 100, birth_tick=5,
            initialized=True, age=1, size=[10, 0, 224]), [1])
        self.assertEqual(bridge.declaration_matches(timed, raw, 100, birth_tick=5,
            initialized=True, age=1, size=[10, 0, 225]), [])
        fractional = copy.deepcopy(timed)
        fractional["metadata"]["actors"][0]["template"]["state"]["geometry"]["Size"]["value"][2] = 0.9
        fractional["declarations"][0] = raw + bytes(152) + struct.pack(">3f", 0, 0, 0.9)
        for width, expected in ((struct.unpack(">f", struct.pack(">f", 0.9))[0], [0]), (0.9, [])):
            self.assertEqual(bridge.declaration_matches(fractional, raw, 100, birth_tick=5,
                initialized=True, age=1, size=[10, 0, width]), expected)
        timed["metadata"]["programs"][0][1]["operation"]["birth"] = dict(size=[None, None,
            dict(min=128, max=224, step=1)])
        self.assertEqual(bridge.declaration_matches(timed, raw, 100, birth_tick=5,
            initialized=True, age=1, size=[10, 0, 128]), [0, 1])
        angled = dict(declarations=[raw, raw], roots=[100], metadata=dict(
            actors=[dict(template=dict(state=dict(angles=[0, yaw, 0],
                angular_velocity=[0, 0, 0]))) for yaw in (0, 90)],
            programs=[[dict(operation=dict(kind="spawn", particle=i)) for i in range(2)]]))
        self.assertEqual(bridge.declaration_matches(angled, raw, 100,
            initialized=True, age=1, angles=[0, 90, 0]), [1])
        for command in angled["metadata"]["programs"][0]:
            command["operation"]["birth"] = dict(yaw_heading=1)
        self.assertEqual(bridge.declaration_matches(angled, raw, 100,
            initialized=True, age=1, angles=[0, 180, 0]), [0, 1])
        for actor in angled["metadata"]["actors"]:
            actor["template"]["fixed_orientation"] = True
        self.assertEqual(bridge.declaration_matches(angled, raw, 100,
            initialized=True, age=1, angles=[45, 180, 90]), [0, 1])
        model = bytes([3]) + raw[1:]
        angled["declarations"] = [model, model]
        for command in angled["metadata"]["programs"][0]:
            command["operation"].pop("birth")
        self.assertEqual(bridge.declaration_matches(angled, model, 100,
            initialized=True, age=1, angles=[0, 180, 0]), [0, 1])
        source_setup = dict(observer=dict(particle_layout="fixed_pool", particle_addresses=[9000]),
                            initial_battle=dict(actors=[dict(slot=4, address=1000)]))
        signature = (2000, 2100, 1000)
        before = dict(battle_combat_tick_word=1, battle_presentation_tick_word=1,
            particle_0_184=1000 + 0x15b4, particle_0_278=1 << 16,
            actor_4_effect_bank=2000, actor_4_effect_program=2100, actor_4_effect_clock_word=5)
        before.update({f"particle_0_{at:x}": 0 for at in range(0x28, 0x40, 4)})
        after = dict(before, battle_combat_tick_word=2, battle_presentation_tick_word=2,
                     actor_4_effect_clock_word=6)
        self.assertEqual(bridge.observed_birth_tick(after,before,"particle_0",signature,
            source_setup,set(),{},{}), 5)
        self.assertIsNone(bridge.observed_birth_tick(after,before,"particle_0",signature,
            source_setup,{9000},{},{}))
        linked = dict(source_setup, observer={})
        linked_after = dict(after, particle_0_address=9000)
        self.assertEqual(bridge.observed_birth_tick(linked_after,before,"particle_0",signature,
            linked,set(),{},{}), 5)
        self.assertIsNone(bridge.observed_birth_tick(linked_after,before,"particle_0",signature,
            linked,{9000},{},{}))
        self.assertIsNone(bridge.observed_birth_tick(after,before,"particle_0",signature,
            linked,set(),{},{}))
        for invalid in (dict(after, actor_4_effect_clock_word=7),
                        dict(after, battle_combat_tick_word=3), dict(after, particle_0_278=2 << 16)):
            self.assertIsNone(bridge.observed_birth_tick(invalid,before,"particle_0",signature,
                source_setup,set(),{},{}))
        self.assertIsNone(bridge.observed_birth_tick(after,dict(before,actor_4_effect_program=2200),
            "particle_0",signature,source_setup,set(),{},{}))
        detached = dict(after, particle_0_184=8000 + 0x28)
        prior_program = dict(id=1, signature=signature, age=5)
        current_program = dict(prior_program, age=6)
        self.assertEqual(bridge.observed_birth_tick(detached,before,"particle_0",signature,
            source_setup,set(),{8000:current_program},{8000:prior_program}), 5)
        current_program["id"] = 2
        self.assertIsNone(bridge.observed_birth_tick(detached,before,"particle_0",signature,
            source_setup,set(),{8000:current_program},{8000:prior_program}))
        live = bytearray(raw)
        live[5] = 1
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        live[20:24] = (0x40000).to_bytes(4,"big")
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        bank["declarations"][1] = bytes(live)
        for slot in (0,1,7,255):
            live[5] = slot
            self.assertEqual(bridge.declaration_matches(bank,live,100),[1])
        for slot in (8,254):
            live[5] = slot
            self.assertEqual(bridge.declaration_matches(bank,live,100),[])
        ring = bytearray(24)
        ring[0] = 4
        ring[20:24] = (0x100000).to_bytes(4,"big")
        ring_bank = dict(declarations=[bytes(ring)], roots=[], metadata={})
        ring[23] = 0x10
        self.assertEqual(bridge.declaration_matches(ring_bank,ring,999),[0])
        ring[23] |= 0x20
        self.assertEqual(bridge.declaration_matches(ring_bank,ring,999),[])
        ring[23] = 0x10
        ring[0] = 3
        self.assertEqual(bridge.declaration_matches(ring_bank,ring,999),[])
        sprite = bytearray(352)
        sprite[0] = 5
        sprite[8:16] = struct.pack(">4h",321,65,62,94)
        sprite_bank = dict(declarations=[bytes(sprite)],roots=[],metadata={},
            uv_descriptors=b"\x06\0" + struct.pack(">4h",320,64,64,96))
        live = bytearray(sprite[:24])
        live[8:16] = struct.pack(">4h",320,64,64,96)
        self.assertEqual(bridge.declaration_matches(sprite_bank,live,999,
            initialized=True,age=1),[0])
        for age in (0,2):
            self.assertEqual(bridge.declaration_matches(sprite_bank,live,999,
                initialized=True,age=age),[])
        live[9] += 1
        self.assertEqual(bridge.declaration_matches(sprite_bank,live,999,
            initialized=True,age=1),[])
        models = [bytearray(352),bytearray(352)]
        for index, model in enumerate(models):
            model[0] = 3
            model[212] = index + 2
        model_bank = dict(declarations=[bytes(model) for model in models],roots=[100],
            uv_descriptors=bytes.fromhex("80000002000000400040"),
            metadata=dict(programs=[[dict(operation=dict(kind="spawn",particle=i)) for i in range(2)]]))
        live = bytearray(models[0][:24])
        live[8:16] = struct.pack(">4h",2,0,64,64)
        self.assertEqual(bridge.declaration_matches(model_bank,live,100),[])
        for index in range(2):
            self.assertEqual(bridge.declaration_matches(model_bank,live,100,
                model_binding=(index+2)<<24,initialized=True,age=1),[index])
        self.assertEqual(bridge.declaration_matches(model_bank,live,100,
            model_binding=4<<24,initialized=True,age=1),[])
        self.assertEqual(bridge.declaration_matches(model_bank,live,100,
            model_binding=2<<24,initialized=True,age=2),[])
        live[8:10] = (3).to_bytes(2,"big")
        self.assertEqual(bridge.declaration_matches(model_bank,live,100,
            model_binding=2<<24,initialized=True,age=1),[])
        for step in ((-2,2),(1,-2),(0,-15)):
            model_bank["uv_descriptors"] = b"\x80\0" + struct.pack(">4h",*step,64,64)
            live[8:16] = struct.pack(">4h",*step,64,64)
            self.assertEqual(bridge.declaration_matches(model_bank,live,100,
                model_binding=2<<24,initialized=True,age=1),[0])
            live[9] ^= 1
            self.assertEqual(bridge.declaration_matches(model_bank,live,100,
                model_binding=2<<24,initialized=True,age=1),[])
        model_bank["uv_descriptors"] = b"\x80\0" + struct.pack(">4h",-64,0,64,64)
        live[8:16] = struct.pack(">4h",-64,0,64,64)
        self.assertEqual(bridge.declaration_matches(model_bank,live,100,
            model_binding=2<<24,initialized=True,age=1),[])
        declaration = bytes.fromhex("070000110000100000010041001e001e0000040004800000")
        sprite = dict(kind="particle", template=dict(lifetime=None, uv_animation=dict(frames=[
            dict(change=dict(kind="rectangle", rect=[0,64,32,32]))])))
        common = dict(source_sha256="common", declarations=[bytes(24)] * 19 + [declaration],
                      metadata=dict(actors=[{}] * 19 + [sprite]))
        setup = dict(common_effect_bank="common", initial_battle=dict(actors=[dict(slot=4,address=1000)]))
        row = dict(particle_0_184=0, particle_0_278=1 << 16,
                   particle_0_1c=1000 + bridge.enemy_artes.POSITION_X,
                   battle_actor_4_action=bridge.STUN_ACTION << 24, battle_actor_4_cast_clock_word=179)
        pending = dict(row, particle_0_278=9 << 16)
        self.assertEqual(bridge.stun_particle_identity(pending,"particle_0",declaration,setup,{1:common},initialized=False),
                         (common,bridge.STUN_DECLARATION,0))
        copied = bytearray(declaration)
        copied[8:16] = bytes.fromhex("0000004000200020")
        self.assertIsNone(bridge.stun_particle_identity(pending,"particle_0",copied,setup,{1:common},initialized=False))
        self.assertEqual(bridge.stun_particle_identity(row,"particle_0",copied,setup,{1:common}),
                         (common,bridge.STUN_DECLARATION,0))
        row["battle_actor_4_action"] = 3 << 24
        self.assertIsNone(bridge.stun_particle_identity(row,"particle_0",copied,setup,{1:common}))
        row["battle_actor_4_action"] = bridge.STUN_ACTION << 24
        row["particle_0_1c"] += 4
        self.assertIsNone(bridge.stun_particle_identity(row,"particle_0",copied,setup,{1:common}))

    def test_embedded_stale_binding_is_not_a_restart(self):
        previous = dict(actor_0_effect_bank=0,actor_0_effect_program=0,actor_0_effect_clock_word=0)
        bound = dict(actor_0_effect_bank=100,actor_0_effect_program=200,actor_0_effect_clock_word=0)
        self.assertTrue(bridge.embedded_start(bound,previous,0))
        self.assertFalse(bridge.embedded_start(bound,bound,0))
        advanced = dict(bound,actor_0_effect_clock_word=20)
        self.assertFalse(bridge.embedded_start(advanced,bound,0))
        self.assertTrue(bridge.embedded_start(bound,advanced,0))

    def test_capture_stop_requires_admission_return_and_complete_empty_tail(self):
        setup, bound, _ = self.fixture()
        idle = copy.deepcopy(bound)
        idle["battle_actor_0_action"] = 3 << 24
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([idle]))
        live = copy.deepcopy(idle)
        live.update(particle_0_address=0x81200000,particle_0_next=live["battle_pool"]+0x763d8,
                    particle_0_10=1<<24,particle_0_28=4<<24)
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,live]))
        self.assertTrue(bridge.completion_observer(0,dict(native=35))([bound,live,idle]))
        shutdown = dict(idle, program_0_address=0x81201000,
                        program_0_next=idle["battle_pool"] + 0x75ec8,
                        program_0_10=0, program_0_14=0x02000000)
        self.assertTrue(bridge.completion_observer(0,dict(native=35))([bound,shutdown]))
        child = dict(shutdown, **{key: value for key, value in live.items() if key.startswith("particle_")})
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,child]))
        future = dict(shutdown, program_0_14=0x01000000)
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,future]))
        stored = dict(idle, battle_stored_slots_word=0x100)
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,stored]))
        self.assertTrue(bridge.completion_observer(0,dict(native=35))([bound,stored,idle]))
        stopped = dict(idle, battle_time_stop_word=1 << 16)
        self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,stopped]))
        self.assertTrue(bridge.completion_observer(0,dict(native=35))([bound,stopped,idle]))
        for slot in ("primary", "secondary"):
            field = f"battle_actor_0_{slot}_phase_age_word"
            pending = dict(idle, **{field: 0x01400047})
            self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,pending]))
            self.assertFalse(self.trace(setup,[bound,pending])["assessment"]["retirement_complete"])
            self.assertTrue(bridge.completion_observer(0,dict(native=35))([bound,pending,idle]))
            del pending[field]
            self.assertFalse(bridge.completion_observer(0,dict(native=35))([bound,pending]))
        inspected = []
        stop = bridge.completion_observer(0, dict(native=35), lambda rows: inspected.append(len(rows)))
        self.assertFalse(stop([bound,live]))
        self.assertFalse(stop([bound,live,live]))
        self.assertTrue(stop([bound,live,live,idle]))
        self.assertEqual(inspected, [2])
        trace = self.trace(setup,[bound,live])
        self.assertTrue(any("retirement" in gap for gap in trace["incomplete_observations"]))
        scripted = dict(id="enemy:1:0", context="attack", self_defeat=dict(retain_body=False))
        dead = dict(live, battle_actor_0_action=18 << 24, actor_0_control_word=2 << 16,
                    actor_0_raw_280=0)
        trace = bridge.normalize(scripted, [bound, dead], copy.deepcopy(setup), {}, dict(native=35), None)
        self.assertFalse(trace["assessment"]["terminal_complete"])
        hidden = dict(idle, battle_actor_0_action=18 << 24, actor_0_control_word=2 << 16,
                      actor_0_raw_280=0x04000000)
        trace = bridge.normalize(scripted, [bound, dead, hidden], copy.deepcopy(setup), {}, dict(native=35), None)
        self.assertTrue(trace["assessment"]["terminal_complete"])
        self.assertTrue(trace["assessment"]["retirement_complete"])
        self.assertTrue(trace["frames"][-1]["actors"][0]["defeated"])
        self.assertGreater(trace["frames"][-1]["actors"][0]["hp"], 0)
        self.assertTrue(any(event["kind"]=="self_defeated" for event in trace["events"]))
        self.assertFalse(any(event["kind"]=="self_cost" for event in trace["events"]))
        missing_hit = "intended attack outcome was not attributed to the requested action"
        self.assertIn(missing_hit, trace["incomplete_observations"])
        scripted["definition"] = dict(execution=dict(attack=dict(events=[
            [0, {"pose": {"clip": 30}}], [1, {"defeat": {"rewards": False}}]])))
        trace = bridge.normalize(scripted, [bound, dead, hidden], copy.deepcopy(setup), {}, dict(native=35), None)
        self.assertNotIn(missing_hit, trace["incomplete_observations"])
        scripted["definition"]["execution"]["attack"]["events"].insert(1, [0, {"contact": {}}])
        trace = bridge.normalize(scripted, [bound, dead, hidden], copy.deepcopy(setup), {}, dict(native=35), None)
        self.assertIn(missing_hit, trace["incomplete_observations"])
        reentry = dict(live, battle_actor_0_action=5 << 24)
        trace = self.trace(setup, [bound, live, reentry, idle])
        self.assertIn("owner admitted another action after returning idle; single-action fixture is incomplete",
                      trace["incomplete_observations"])
        approach = dict(live, battle_actor_0_action=4 << 24)
        trace = self.trace(setup, [bound, live, approach, idle])
        self.assertIn("owner began another approach after returning idle; single-action fixture is incomplete",
                      trace["incomplete_observations"])
        self.assertTrue(trace["assessment"]["retirement_complete"])
        bound.update(actor_0_selected=123,
                     battle_actor_0_target=setup["initial_battle"]["actors"][1]["address"])
        guard = dict(live, battle_actor_0_action=25 << 24, actor_0_selected=122)
        trace = bridge.normalize(dict(id="enemy:53:2"), [bound, live, guard, idle],
                                 setup, {}, dict(selected=123), None)
        self.assertIn("owner admitted another action after returning idle; single-action fixture is incomplete",
                      trace["incomplete_observations"])
        attack_case = dict(id="enemy:48:2", definition=dict(requirements=dict(predicates=["target_attacking"])))
        attacking = dict(bound, battle_actor_4_action=5 << 24, actor_4_target_state_word=3 << 24)
        trace = bridge.normalize(attack_case, [attacking, idle], setup, {}, dict(selected=123), None)
        gap = "required target attack was not observed at row selection"
        self.assertNotIn(gap, trace["incomplete_observations"])
        self.assertEqual(trace["setup"]["target_attack_admission"]["phase"], 3)
        approach = dict(attacking, battle_actor_0_action=4 << 24)
        ended_attack = dict(attacking, battle_combat_tick_word=2, battle_presentation_tick_word=2,
                            battle_actor_4_action=3 << 24, actor_4_target_state_word=0)
        approached_setup = copy.deepcopy(setup)
        approached_setup["actors"][0].update(heading=0.0, position=[0.0, 0.0, 0.0])
        trace = bridge.normalize(attack_case, [approach, ended_attack, idle],
                                 approached_setup, {}, dict(selected=123), None)
        self.assertNotIn(gap, trace["incomplete_observations"])
        self.assertEqual(trace["setup"]["target_attack_admission"]["phase"], 3)
        attacking["actor_4_target_state_word"] = 0
        trace = bridge.normalize(attack_case, [attacking, idle], setup, {}, dict(selected=123), None)
        self.assertIn(gap, trace["incomplete_observations"])
        events = [dict(kind="program_started", owner=0, bank="bank", program=0, presentation_tick=t)
                  for t in (10,19,28)]
        events.append(dict(kind="declaration_started", owner=0, bank="bank", member=0))
        banks = {1:dict(source_sha256="bank", metadata=dict(
            programs=[[dict(at=0, operation=dict(kind="spawn", particle=0))]],
            actors=[dict(kind="particle", template=dict(lifetime=17))]))}
        baseline = bridge.recurring_effects(events, banks)
        model_banks = copy.deepcopy(banks)
        model_banks[1]["metadata"]["actors"][0]["kind"] = "model_particle"
        self.assertEqual(bridge.recurring_effects(events, model_banks), baseline)
        model_banks[1]["metadata"]["actors"][0]["template"]["lifetime"] = None
        self.assertEqual(bridge.recurring_effects(events, model_banks), [])
        particle = dict(owner=0, bank="bank", member=0, initialized=True)
        live["battle_presentation_tick_word"] = 40
        counts = bridge.observation_counts(live)
        self.assertTrue(bridge.baseline_tail(live, counts, baseline, [], [(particle,13)]))
        self.assertFalse(bridge.baseline_tail(live, counts, baseline, [], [(particle,18)]))
        self.assertFalse(bridge.baseline_tail(live, counts, [], [], [(particle,13)]))
        banks[1]["metadata"]["programs"][0].append(dict(at=0, operation=dict(kind="spawn", particle=1)))
        banks[1]["metadata"]["actors"].append(dict(kind="particle", template=dict(lifetime=9)))
        events.append(dict(kind="declaration_started", owner=0, bank="bank", member=1))
        registered = dict(initial_battle=dict(presentation_tick=0),
                          ready_passive_effects=[dict(actor=0,program=0,interval=9,counter=0)])
        self.assertEqual(len(bridge.recurring_effects(events,banks,registered,11)),2)
        recurrence = [event for event in events if event.get("presentation_tick") != 28]
        self.assertEqual(len(bridge.recurring_effects(recurrence,banks,registered,11)),2)
        self.assertEqual(bridge.recurring_effects(recurrence,banks),[])
        registered["ready_passive_effects"][0]["interval"] = 8
        self.assertEqual(bridge.recurring_effects(recurrence,banks,registered,11),[])
        registered["ready_passive_effects"][0]["interval"] = 9
        self.assertEqual(bridge.recurring_effects(events,banks,registered,10),[])
        pristine = copy.deepcopy(registered)
        pristine["ready_effect_lists"] = [dict(kind=kind, sentinel=i, next=i, previous=i)
            for i,kind in enumerate(("projectile", "program", "particle", "alternate"))]
        paused = [dict(event, presentation_tick=event["presentation_tick"] + (2 if index else 0),
                       frozen_updates=2 if index else 0) if event["kind"] == "program_started" else event
                  for index,event in enumerate(events)]
        phased = bridge.recurring_effects(paused,banks,pristine,9)
        self.assertEqual(phased[0]["observed_starts"], [10,19,28])
        self.assertEqual(phased[0]["presentation_starts"], [10,21,30])
        pristine["ready_effect_lists"].pop()
        self.assertEqual(bridge.recurring_effects(paused,banks,pristine,9),[])
        registered["ready_passive_effects"][0]["counter"] = 1
        self.assertEqual(bridge.recurring_effects(events,banks,registered,11),[])
        registered["ready_passive_effects"][0]["counter"] = 0
        scenery = dict(initial_battle=dict(presentation_tick=0),
                       ready_ambient_effect=dict(bank="bank", program=0, interval=9,
                                                 counter=0, program_end=1))
        scenery_events = [dict(event, observed_age=1) if event["kind"] == "program_started"
                          else event for event in recurrence]
        self.assertEqual(len(bridge.recurring_effects(scenery_events,banks,scenery,20)),2)
        scenery["ready_ambient_effect"]["counter"] = 1
        self.assertEqual(bridge.recurring_effects(scenery_events,banks,scenery,20),[])
        overlapping = copy.deepcopy(scenery)
        overlapping["ready_ambient_effect"].update(counter=0, program_end=18)
        repeated = copy.deepcopy(banks)
        repeated[1]["metadata"]["programs"][0].append(dict(at=0, operation=dict(kind="spawn", particle=0)))
        stage = bridge.recurring_effects(scenery_events, repeated, overlapping, 20)
        self.assertEqual(stage[0]["scenery"], dict(program_end=18, spawn_offsets=[0,0]))
        resumed = scenery_events + [dict(scenery_events[0], presentation_tick=33)]
        self.assertEqual(bridge.recurring_effects(resumed, repeated, overlapping, 20), [])
        self.assertEqual(bridge.recurring_effects(resumed, repeated, overlapping, 20, scenery_pause=22), stage)
        banks[1]["program_ends"] = [8]
        scheduler_events = [dict(event, observed_age=2) if event["kind"] == "program_started"
                            else event for event in recurrence]
        aged = dict(scheduler_events[0], presentation_tick=5, observed_age=6, observed_at_entry=True)
        scheduler_events.insert(0, aged)
        baseline = bridge.recurring_effects(scheduler_events,banks,registered,11)
        self.assertEqual(baseline[0]["observed_starts"], [10,19])
        aged["observed_at_entry"] = False
        self.assertEqual(bridge.recurring_effects(scheduler_events,banks,registered,11), [])
        aged["observed_at_entry"] = True
        running = dict(live, battle_presentation_tick_word=22,
                       program_0_address=0x81201000,
                       program_0_next=live["battle_pool"] + 0x75ec8, program_0_10=0)
        counts = bridge.observation_counts(running)
        program = dict(owner=0, bank="bank", program=0, age=5)
        self.assertTrue(bridge.baseline_tail(running, counts, baseline, [program], [(particle,4)]))
        paused = dict(running, battle_presentation_tick_word=27)
        stage_program = dict(program, age=4)
        self.assertFalse(bridge.baseline_tail(paused, counts, stage, [stage_program], [(particle,3)]))
        self.assertTrue(bridge.baseline_tail(paused, counts, stage, [stage_program], [(particle,3)], scenery_frozen=5))
        self.assertFalse(bridge.baseline_tail(paused, counts, stage, [stage_program], [(particle,3)], scenery_frozen=4))
        self.assertFalse(bridge.baseline_tail(paused, counts, baseline, [program], [(particle,4)]))
        self.assertTrue(bridge.baseline_tail(paused, counts, baseline, [program], [(particle,4)], frozen_updates={0:5}))
        self.assertFalse(bridge.baseline_tail(paused, counts, baseline, [program], [(particle,4)], frozen_updates={0:4}))
        for age in (4,9):
            self.assertFalse(bridge.baseline_tail(running, counts, baseline, [dict(program,age=age)], [(particle,4)]))
        self.assertFalse(bridge.baseline_tail(running, counts, baseline, [program], [(dict(particle,member=2),4)]))
        banks[1]["metadata"]["programs"][0].append(dict(at=2, operation=dict(kind="spawn", particle=0)))
        baseline = bridge.recurring_effects(scheduler_events,banks,registered,11)
        self.assertTrue(bridge.baseline_tail(running, counts, baseline, [program], [(particle,3)]))
        self.assertFalse(bridge.baseline_tail(running, counts, baseline, [program], [(particle,2)]))
        shutdown = dict(running, program_0_14=0x02000000)
        self.assertTrue(bridge.baseline_tail(shutdown, counts, baseline, [dict(program,age=8)], [(particle,3)]))
        self.assertFalse(bridge.baseline_tail(shutdown, counts, baseline, [dict(program,age=8)], [(dict(particle,member=2),3)]))
        # Both one generation and overlapping finite generations can emit at
        # the same recurring phase. Neither allows a third unobserved birth.
        for second_birth in (2, 11):
            banks[1]["program_ends"] = [max(8, second_birth)]
            banks[1]["metadata"]["programs"][0].append(dict(at=second_birth, operation=dict(kind="spawn", particle=0)))
            paired = bridge.recurring_effects(scheduler_events,banks,registered,11)
            self.assertEqual(paired[0]["scheduler"]["spawn_offsets"], [0,1,second_birth-1])
            for total in (2,3):
                simultaneous = dict(running)
                for index in range(total):
                    simultaneous.update({f"particle_{index}_address":0x81200000+index*0x288,
                        f"particle_{index}_next":0x81200000+(index+1)*0x288 if index+1<total else live["battle_pool"]+0x763d8,
                        f"particle_{index}_10":1<<24, f"particle_{index}_28":4<<24})
                self.assertEqual(bridge.baseline_tail(simultaneous, bridge.observation_counts(simultaneous),
                    paired, [program], [(particle,3)]*total), total==2)
                self.assertFalse(bridge.baseline_tail(simultaneous, bridge.observation_counts(simultaneous),
                    paired, [program], [(particle,2)]*total))
            banks[1]["metadata"]["programs"][0].pop()
        banks[1]["metadata"]["programs"][0].pop()
        instant = copy.deepcopy(banks)
        instant[1]["program_ends"] = [0]
        instant[1]["metadata"]["programs"][0].append(dict(at=0, operation=dict(kind="spawn", particle=0)))
        instant_events = [dict(event, observed_age=0) if event["kind"]=="program_started" else event for event in recurrence]
        immediate = bridge.recurring_effects(instant_events, instant, registered, 11)
        self.assertEqual(immediate[0]["scheduler"], dict(program_end=0, observed_start_age=0, spawn_offsets=[0,0]))
        banks[1]["program_ends"] = [None]
        baseline = bridge.recurring_effects(scheduler_events,banks,registered,11)
        self.assertFalse(bridge.baseline_tail(running, counts, baseline, [program], [(particle,4)]))
        banks[1]["metadata"]["actors"][1]["template"]["lifetime"] = None
        self.assertEqual(bridge.recurring_effects(events,banks,registered,11),[])

    def test_learned_repertoire_uses_saved_bit_indices_not_catalogue_id_bits(self):
        member = 0x81000000 + bridge.battle_artes.MEMBERS
        values = {bridge.battle_artes.SETTINGS:0x81000000, member + 0x70:5}
        state = bridge.SimpleNamespace(read=lambda address, fmt="I": values[address])
        techniques, proof = bridge.learned_techniques(state,1,[98,99,107])
        self.assertEqual(techniques,[98,107])
        self.assertEqual(proof["bindings"],[(0,98),(2,107)])
        values[member + 0x70] = 8
        with self.assertRaises(ValueError):
            bridge.learned_techniques(state,1,[98,99,107])

    def test_final_spawn_requires_immediately_observed_exact_parent_generation(self):
        signature = (100,200,300)
        previous = {0x81000000:dict(signature=signature,age=16)}
        self.assertTrue(bridge.retired_parent_birth(0x81000028,signature,1,previous))
        self.assertFalse(bridge.retired_parent_birth(0x81000028,signature,2,previous))
        self.assertFalse(bridge.retired_parent_birth(0x81000028,(100,201,300),1,previous))
        self.assertFalse(bridge.retired_parent_birth(0x81000028,signature,1,{}))
        setup, pending, _ = self.fixture()
        pending.update(program_0_address=0x81201000,program_0_next=pending["battle_pool"]+0x75ec8,
            program_0_60=0x81080000,program_0_6c=0x81080100,program_0_2c=0x81000000,
            particle_0_address=0x81202000,particle_0_next=pending["battle_pool"]+0x763d8,
            particle_0_10=0x01060002,particle_0_28=4<<24,particle_0_278=13<<16,
            particle_0_184=0x81201028,particle_0_parent_bank=0x81080000,
            particle_0_parent_program=0x81080100,particle_0_parent_owner=0x81000000)
        initialized = dict(pending,particle_0_10=0x01060402,particle_0_278=1<<16,
            battle_combat_tick_word=2,battle_presentation_tick_word=2,program_0_5c=1)
        bank = dict(source_sha256="pinned",roots=[0x100],declarations=[bytes([4])+bytes(23)],
            metadata=dict(actors=[dict(kind="ring")],programs=[[
                dict(operation=dict(kind="spawn",particle=0))]]))
        trace=bridge.normalize(dict(id="party:2:35"),[pending,initialized],setup,
            {0x81080000:bank},dict(native=35),None)
        births=[e for e in trace["events"] if e["kind"]=="declaration_started"]
        self.assertEqual([e["clock"] for e in births],[1])
        self.assertEqual([f["particles"][0]["id"] for f in trace["frames"]],[1,1])
        self.assertEqual([f["particles"][0]["initialized"] for f in trace["frames"]],[False,True])
        self.assertIsNone(trace["frames"][0]["particles"][0]["origin"])
        self.assertEqual(trace["frames"][1]["particles"][0]["origin"],[0,0,0])
        unknown = dict(initialized, particle_0_parent_bank=0)
        trace = bridge.normalize(dict(id="party:2:35"), [dict(pending, particle_0_parent_bank=0), unknown], setup,
            {0x81080000:bank}, dict(native=35), None)
        self.assertIn("initialized effect birth lacks exact bank/declaration/owner identity",
                      trace["incomplete_observations"])
        setup["observer"] = dict(particle_transforms=False)
        bank["metadata"]["actors"][0] = dict(kind="model_particle", binding=dict(animation=None))
        trace = bridge.normalize(dict(id="party:2:35"), [pending, initialized], setup,
            {0x81080000:bank}, dict(native=35), None)
        self.assertIn("sprite-only observer encountered an effect model", trace["incomplete_observations"])


if __name__ == "__main__":
    unittest.main()
