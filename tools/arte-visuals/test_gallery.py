import unittest
from gallery import evidence, review_status


class VisualEvidence(unittest.TestCase):
    def setUp(self):
        self.case = {'id': 'steam:party:1:1', 'counterpart': {'candidates': ['gamecube:party:1:1']}}
        self.captures = {key: {'status': 'captured', 'admitted': True,
                              'source_kind': 'resonance',
                              'images': [{'age': 20, 'sha256': key}]}
                         for key in [self.case['id'], 'gamecube:party:1:1']}

    def test_mechanical_capture_is_not_visual_acceptance(self):
        self.assertEqual(review_status(self.case, self.captures, {})[0], 'pending')
        self.captures[self.case['id']]['images'] = []
        self.assertEqual(review_status(self.case, self.captures, {})[0], 'capture_incomplete')

    def test_changed_pixels_invalidate_review_and_reference_is_required(self):
        key, _ = evidence(self.case, self.captures)
        review = {self.case['id']: {'status': 'minor_texture_difference',
                  'reference': 'gamecube:party:1:1', 'evidence_key': key}}
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'minor_texture_difference')
        self.captures['gamecube:party:1:1']['images'][0]['sha256'] = 'changed pixels'
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'stale_review')
        key, _ = evidence(self.case, self.captures, 'missing')
        review[self.case['id']].update(evidence_key=key, reference='missing')
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'reference_missing')
        self.captures['gamecube:party:1:1']['admitted'] = False
        key, _ = evidence(self.case, self.captures, 'gamecube:party:1:1')
        review[self.case['id']].update(evidence_key=key, reference='gamecube:party:1:1')
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'reference_missing')

    def test_unchanged_review_pair_survives_new_alternative_captures(self):
        reference = 'original:gamecube:party:1:1'
        self.captures[reference] = {'status': 'captured', 'admitted': True,
            'source_kind': 'dolphin', 'images': [{'age': 20, 'sha256': 'native pixels'}]}
        key, _ = evidence(self.case, self.captures, reference)
        review = {self.case['id']: {'status': 'match', 'reference': reference, 'evidence_key': key}}
        self.captures['gamecube:party:1:1']['images'][0]['sha256'] = 'new alternative render'
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'match')
        self.captures[reference]['images'][0]['sha256'] = 'changed reference'
        self.assertEqual(review_status(self.case, self.captures, review)[0], 'stale_review')


if __name__ == '__main__':
    unittest.main()
