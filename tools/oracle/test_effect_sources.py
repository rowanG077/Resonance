import struct
import unittest
import effect_pitch
from unittest.mock import patch
import effect_defaults
import effect_sources


class EffectSources(unittest.TestCase):
    def test_binding_reorders_and_shares_only_equal_original_definitions(self):
        first = dict(kind='unsupported', reason='first')
        second = dict(kind='unsupported', reason='second')
        with patch.object(effect_sources, 'original_templates', return_value=[first,second,first]):
            selected, aliases = effect_sources.bind(b'original', [second,first], [b'A',b'B',b'C'])
            self.assertEqual(selected,[b'B',b'A'])
            self.assertEqual(aliases,[[b'B'],[b'A',b'C']])
            with self.assertRaisesRegex(ValueError, 'no equivalent'):
                effect_sources.bind(b'original',[dict(kind='unsupported',reason='edited')],[b'A',b'B',b'C'])

    def test_native_commands_resolve_current_members_and_skip_removed_templates(self):
        raw = b''.join(struct.pack('>hBBH', *record) for record in
            [(0, 5, 0, 0), (1, 3, 0, 0), (2, 254, 0, 0)])
        bank = dict(raw=raw, roots=[0], evidence=dict(effect_templates=[[5]]))
        emissions = list(effect_pitch.authored_emissions(bank, 0))
        self.assertEqual(len(emissions), 1)
        self.assertEqual(emissions[0]['member'], 0)
        self.assertEqual(emissions[0]['command'], 0)

    def test_repeated_named_births_keep_random_sharing_and_previous_targets(self):
        programs = [[dict(at=2,repeat=3,every=5,events=[
            dict(kind='spawn',particle='spark',birth=dict(random_draws=2,
                size=[dict(min=0,max=1,draw=1),None,None])),
            dict(at=2,kind='spawn',particle='ember',birth=dict(
                velocity=[dict(min=0,max=1,draw=-2),None,None],
                model_parent=dict(previous_spawn=1,joint=3))),
            dict(at=3,kind='set_motion',previous_spawn=2,velocity=[None,2,None])])]]
        expanded=effect_defaults.programs(programs,['ember','spark'])[0]
        self.assertEqual(len(expanded),9)
        for index in range(3):
            first,second,motion=expanded[index*3:index*3+3]
            self.assertEqual([first['at'],second['at'],motion['at']],[2+5*index,4+5*index,5+5*index])
            self.assertEqual(first['operation']['particle'],1)
            self.assertEqual(second['operation']['particle'],0)
            self.assertEqual(first['operation']['birth']['size'][0]['draw'],index*2+1)
            self.assertEqual(second['operation']['birth']['velocity'][0]['draw'],index*2)
            self.assertEqual(second['operation']['birth']['model_parent']['spawn'],index*2)
            self.assertEqual(motion['operation']['spawn'],index*2)
        with self.assertRaises(ValueError):
            effect_defaults.programs([[dict(kind='spawn',particle='missing')]],['spark'])


if __name__ == '__main__':
    unittest.main()
