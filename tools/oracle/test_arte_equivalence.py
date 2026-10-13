"""Focused offline assertions for the source bridge; never launches Dolphin."""
import copy
import unittest

import arte_equivalence as bridge


class ArteTraceTests(unittest.TestCase):
    def fixture(self):
        owner, target = 0x81000000, 0x81002000
        setup = dict(owner=0, target=1, actors=[dict(actor=0), dict(actor=1)],
            source_actor_map=[dict(actor=0, source_slot=0), dict(actor=1, source_slot=4)],
            input={}, initial_battle=dict(combat_tick=0, actors=[dict(slot=0, address=owner), dict(slot=4, address=target)]))
        row = {name: 0 for name in bridge.locations().values()}
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

    def test_unattributed_hp_loss_and_wrong_action_never_become_arte_hit(self):
        setup, row, _ = self.fixture()
        after = copy.deepcopy(row)
        after["battle_actor_4_hp"] = 80
        result = self.trace(setup, [row, after])
        self.assertFalse(any(e["kind"] == "hit" for e in result["events"]))
        self.assertEqual(result["events"][-1]["kind"], "unattributed_hp_loss")
        row["actor_0_native"] = after["actor_0_native"] = 36
        self.assertFalse(any(e["kind"] == "started" for e in self.trace(setup, [row, after])["events"]))

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
        setup, row, _ = self.fixture()
        row["battle_menu_flags_word"] = 0x600000fd
        result = self.trace(setup, [row])
        self.assertFalse(any("menu_or_pause" in value for value in result["incomplete_observations"]))

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

    def test_observer_fits_existing_datagram_receiver(self):
        for slots in ([0, 4], [0,1,2,3,4], list(range(12))):
            _, budget = bridge.observer_plan(slots)
            self.assertLess(budget["maximum_bytes"], 60000)

    def test_pending_clip_never_inherits_outgoing_controller_frame(self):
        row = dict(battle_actor_0_body_clip_word=43 << 8,
                   actor_0_animation_0_layer_word=6, actor_0_animation_0_resource=100,
                   actor_0_animation_0_frame_bits=0x41500000,
                   actor_0_animation_0_child=200, actor_0_animation_0_child_resource=300,
                   actor_0_animation_0_child_frame_bits=0)
        self.assertEqual(bridge.observed_body(row,0,{"43":300}), (43,0.0))
        row["actor_0_animation_0_child_resource"] = 301
        self.assertEqual(bridge.observed_body(row,0,{"43":300}), (43,None))

    def test_one_canonical_relative_event_stream(self):
        setup, row, _ = self.fixture()
        row["battle_combat_tick_word"], row["battle_presentation_tick_word"] = 20, 50
        result = self.trace(setup, [row])
        self.assertNotIn("semantic", result["assessment"])
        self.assertEqual(result["assessment"]["outcome"], "attack")
        event = next(e for e in result["events"] if e["kind"] == "started")
        self.assertEqual((event["clock"], event["presentation_clock"]), (0,0))
        self.assertNotIn("combat_tick", event)

    def test_pending_selection_is_not_committed_admission(self):
        setup, bound, _ = self.fixture()
        pending = copy.deepcopy(bound)
        pending["battle_actor_0_action"] = 5 << 24
        bound["battle_combat_tick_word"] = bound["battle_presentation_tick_word"] = 2
        result = self.trace(setup, [pending, bound])
        self.assertEqual(result["frames"][0]["combat_tick"], 2)
        self.assertEqual(result["setup"]["input"]["request_after_updates"], 1)
        self.assertEqual(result["setup"]["input"]["bound_admission_after_updates"], 2)

    def test_circular_sentinel_is_not_a_live_particle(self):
        _, row, _ = self.fixture()
        self.assertEqual(bridge.tracked_list(row, "particle", bridge.PARTICLES), ([], True))
        row["particle_0_address"] = 0x81200000
        row["particle_0_next"] = row["battle_pool"] + 0x763d8
        self.assertEqual(bridge.tracked_list(row, "particle", bridge.PARTICLES), ([(0, 0x81200000)], True))

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

    def test_execution_without_intended_hit_is_incomplete(self):
        setup,row,_=self.fixture()
        self.assertIn("intended attack outcome was not attributed to the requested action",
                      self.trace(setup,[row])["incomplete_observations"])

    def test_current_contact_queue_accepts_equal_rolls_but_not_stale_contexts(self):
        setup,row,target=self.fixture()
        row.update(contact_queue_counts=1<<16,actor_4_fresh_contact_word=1<<24,
                   actor_0_contact_target=target,contact_queue_0_0_owner=0x81000000,
                   contact_queue_0_0_rule=0x81080000,contact_queue_0_0_template=0x81080100,
                   contact_queue_0_0_nominal=20)
        for _ in range(2):
            self.assertEqual(bridge.attached_contact(row,setup,0,1),"contact_queue_0_0")
        row["contact_queue_counts"]=0
        self.assertIsNone(bridge.attached_contact(row,setup,0,1))
        row.update(contact_queue_counts=2<<16,contact_queue_0_1_owner=0x81000000,
                   contact_queue_0_1_rule=0x81080200,contact_queue_0_1_template=0x81080300,
                   contact_queue_0_1_nominal=20)
        self.assertIsNone(bridge.attached_contact(row,setup,0,1))

    def test_pool_compaction_retains_dynamic_suffixes(self):
        ordinary=bridge.locations([0,4],4,4,4)
        compact=bridge.locations([0,4],4,4,4,pool_base=0x80900000)
        self.assertEqual(compact["809763dc 4 170"], ordinary[bridge.POOL + " 763dc 4 170"])
        self.assertLess(bridge.packet_budget(compact)["maximum_bytes"],bridge.packet_budget(ordinary)["maximum_bytes"])

    def test_authored_birth_uv_override_requires_exact_supported_bytes(self):
        raw=bytes(24)
        bank=dict(declarations=[raw],roots=[100],metadata=dict(programs=[[
            dict(operation=dict(kind="spawn",particle=0,birth=dict(uv=[dict(min=288,max=288),None,None,None],geometry_count=dict(min=4,max=4))))]]))
        live=bytearray(raw);live[8:10]=(288).to_bytes(2,"big");live[18]=4
        self.assertEqual(bridge.declaration_matches(bank,live,100),[0])
        live[18]=5
        self.assertEqual(bridge.declaration_matches(bank,live,100),[])

    def test_identical_prefix_is_disambiguated_only_by_observed_parent_program(self):
        raw = bytes(24)
        bank = dict(declarations=[raw,raw], roots=[100], metadata=dict(programs=[[
            dict(operation=dict(kind="spawn",particle=1))]]))
        self.assertEqual(bridge.declaration_matches(bank,raw,100),[1])
        self.assertEqual(bridge.declaration_matches(bank,raw,999),[0,1])

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
        trace = self.trace(setup,[bound,live])
        self.assertTrue(any("retirement" in gap for gap in trace["incomplete_observations"]))

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
        self.assertEqual([e["clock"] for e in births],[0])
        self.assertEqual([f["particles"][0]["id"] for f in trace["frames"]],[1,1])
        self.assertIsNone(trace["frames"][0]["particles"][0]["origin"])
        self.assertEqual(trace["frames"][1]["particles"][0]["origin"],[0,0,0])


if __name__ == "__main__":
    unittest.main()
