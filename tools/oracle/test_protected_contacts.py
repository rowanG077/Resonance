"""Attribution checks for protected contacts and observed offensive outcomes."""
import copy
import json
import unittest
from pathlib import Path
import arte_equivalence as bridge


class ObservedOutcomeTests(unittest.TestCase):
    def test_only_owned_opponent_afflictions_reclassify_presentation(self):
        setup = dict(owner=1, actors=[dict(side=0), dict(side=1), dict(side=1)])
        started = dict(kind='started', owner=1, action=4)
        event = dict(kind='condition_changed', owner=1, target=0, action=4,
                     before=[], after=['attack_down', 'poison_severe'])
        self.assertEqual(bridge.observed_outcome('presentation', setup, [started, event]), 'attack')
        for change in [dict(owner=2), dict(target=2), dict(action=5), dict(target=-1),
                       dict(before=['poison_severe'], after=['poison_severe']),
                       dict(after=['attack_up'])]:
            with self.subTest(change=change):
                self.assertEqual(bridge.observed_outcome('presentation', setup,
                    [started, dict(event, **change)]), 'presentation')
        self.assertEqual(bridge.observed_outcome('presentation', setup, [event]), 'presentation')
        self.assertEqual(bridge.observed_outcome('support', setup, [started, event]), 'support')


class ProtectedProjectileTests(unittest.TestCase):
    def setUp(self):
        fixture = json.loads((Path(__file__).with_name('fixtures') /
                              'protected-projectile-contact.json').read_text())
        self.setup = fixture['setup']
        self.before, self.contact, self.after = fixture['rows']
        nodes, complete = bridge.tracked_list(self.contact, 'projectile',
            bridge.observation_counts(self.contact)['projectile'])
        self.assertTrue(complete)
        self.assertEqual(len(nodes), 1)
        self.prefix = f'projectile_{nodes[0][0]}'
        self.identities = {nodes[0][1]: fixture['projectile_id']}

    def observe(self, current=None, before=None, identities=None):
        return bridge.protected_projectile_contact(
            self.contact if current is None else current,
            self.before if before is None else before, self.setup, 1, 0,
            self.identities if identities is None else identities)

    def test_closed_down_window_observes_one_source_projectile_contact(self):
        event = self.observe()
        self.assertEqual(event['projectile'], 28)
        self.assertEqual((event['owner'], event['target'], event['hp_change']), (1, 0, 0))
        self.assertEqual(event['protection'], 'down_window_closed')
        self.assertEqual(event['damage_branches'], dict(ordinary=[0, 0], critical=None))
        self.assertIsNone(self.observe(self.after, self.contact))

    def test_hp_change_open_window_and_stale_recipient_are_not_zero_damage_hits(self):
        for change in [dict(battle_actor_0_hp=self.contact['battle_actor_0_hp'] - 1),
                       dict(actor_0_protection_window_word=0x01010000),
                       dict(actor_0_raw_280=0), dict(actor_0_fresh_contact_word=0x60),
                       dict(contact_queue_counts=0),
                       dict(battle_combat_tick_word=self.contact['battle_combat_tick_word'] + 1),
                       dict(actor_4_contact_target=0)]:
            with self.subTest(change=change):
                self.assertIsNone(self.observe(dict(self.contact, **change)))

    def test_live_identity_and_recipient_cooldown_are_required(self):
        with self.assertRaisesRegex(bridge.damage_envelope.UnsupportedDamage, 'identity'):
            self.observe(identities={})
        for field, value in [('last_target', 0), ('cooldown_0', 9 << 24), ('280', 0),
                             ('19c', 0), ('278', self.contact[self.prefix + '_278'] + 0x10000)]:
            with self.subTest(field=field):
                with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
                    self.observe(dict(self.contact, **{self.prefix + '_' + field: value}))
        stale = dict(self.before, **{self.prefix + '_cooldown_0': 5 << 24})
        with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
            self.observe(before=stale)
        missing = copy.deepcopy(self.before)
        missing['object_lists']['projectile'] = []
        with self.assertRaises(bridge.damage_envelope.UnsupportedDamage):
            self.observe(before=missing)


if __name__ == '__main__':
    unittest.main()
