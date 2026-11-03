#!/usr/bin/env python3
"""Join actual captures for visual review; never infer acceptance from execution."""
import argparse
from collections import Counter
import hashlib
import html
import json
from pathlib import Path
import shutil


def read(path):
    return json.loads(Path(path).read_text())


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def capture_reports(paths, output, identifiers=None):
    captures = {}
    for path in paths:
        path = Path(path).resolve()
        report = read(path)
        for case in report['cases']:
            if case.get('status') == 'pending' and case.get('error') == 'not requested':
                continue
            identifier = case['id']
            if report.get('source_kind') == 'dolphin':
                identifier = 'original:' + identifier
            if identifiers is not None and identifier not in identifiers:
                continue
            images = []
            for image in case.get('images', []):
                source = Path(image['path'])
                if not source.is_absolute():
                    source = path.parent / source
                digest = hashlib.sha256(source.read_bytes()).hexdigest()
                target = output / 'images' / (digest + source.suffix)
                if not target.exists():
                    shutil.copyfile(source, target)
                images.append(dict(image, path=str(target.relative_to(output)), sha256=digest))
            captures[identifier] = dict(case, images=images,
                source_kind=report.get('source_kind', 'unspecified'),
                report_path=str(path), library=report.get('library'))
    return captures


def work_items(catalogue):
    for case in catalogue['cases']:
        yield case
        for variant in case.get('recipe_variants', []):
            yield dict(variant, display_name=case['display_name'] +
                       f" — recipe {variant['index']}", edition=case['edition'],
                       category='unison_recipe', flags=case.get('flags', []))


def evidence(case, captures, reference=None):
    candidates = case.get('counterpart', {}).get('candidates', [])
    selected = [case['id'], *candidates]
    selected += ['original:' + key for key in selected if key.startswith('gamecube:')]
    if reference and reference not in selected:
        selected.append(reference)
    records = {key: captures[key] for key in selected if key in captures}
    # Include image bytes, not just filenames, so replaced PNGs invalidate review.
    # Once chosen, only the reviewed pair determines staleness. New alternative
    # captures must not invalidate an unchanged original-game comparison.
    reviewed = ({key: records[key] for key in [case['id'], reference] if key in records}
                if reference else records)
    key = fingerprint({identifier: {
        'status': value.get('status'), 'admitted': value.get('admitted'),
        'source_kind': value['source_kind'], 'library': value.get('library'),
        'images': [(image.get('age'), image['sha256']) for image in value['images']]
    } for identifier, value in reviewed.items()})
    return key, records


def review_status(case, captures, reviews):
    review = reviews.get(case['id'], {})
    key, records = evidence(case, captures, review.get('reference'))
    if case.get('scope_status') == 'source_unselectable':
        return 'pending', key, records
    current = records.get(case['id'])
    if not current or current.get('status') != 'captured' or not current.get('admitted') or not current['images']:
        return 'capture_incomplete', key, records
    if not review:
        return 'pending', key, records
    if review.get('evidence_key') != key:
        return 'stale_review', key, records
    selected = review.get('reference')
    reference = records.get(selected)
    if (selected == case['id'] or not reference or reference.get('status') != 'captured'
            or not reference.get('admitted') or not reference['images']):
        return 'reference_missing', key, records
    candidates = case.get('counterpart', {}).get('candidates', [])
    direct = candidates + ['original:' + key for key in candidates if key.startswith('gamecube:')]
    if case['id'].startswith('gamecube:'):
        direct.append('original:' + case['id'])
    if selected not in direct and review.get('basis') != 'gc_style':
        return 'reference_choice_missing', key, records
    # A name match or green mechanical test cannot set a visual review result.
    status = review.get('status')
    allowed = {'match', 'minor_texture_difference', 'needs_fix', 'pending'}
    return status if status in allowed else 'pending', key, records


def generate(catalogue, captures, reviews, output):
    esc = lambda value: html.escape(str(value), quote=True)
    sections, coverage = [], []
    for case in work_items(catalogue):
        status, key, records = review_status(case, captures, reviews)
        originals = [identifier for identifier, record in records.items()
                     if record['source_kind'] == 'dolphin' and record.get('status') == 'captured'
                     and record.get('admitted') and record['images']]
        chosen = records.get(reviews.get(case['id'], {}).get('reference'), {})
        original_status = ('reviewed' if chosen.get('source_kind') == 'dolphin'
                           and status in {'match', 'minor_texture_difference'}
                           else 'available' if originals else 'missing')
        coverage.append({'id': case['id'], 'capture_status':
                         captures.get(case['id'], {}).get('status', 'pending'),
                         'review_status': status, 'evidence_key': key,
                         'scope_status': case.get('scope_status', 'original_game_required'),
                         'scope_evidence': case.get('scope_evidence'),
                         'requires_visual_comparison': case.get('scope_status') not in
                             {'source_unimplemented_slot', 'repository_authored', 'source_unselectable'},
                         'original_gc_reference': {'status': original_status, 'candidates': originals}})
        columns = []
        for identifier, record in records.items():
            frames = ''.join(f'<figure><a href="{esc(image["path"])}"><img loading="lazy" src="{esc(image["path"])}"></a>'
                             f'<figcaption>age {esc(image.get("age", "?"))}</figcaption></figure>'
                             for image in record['images'])
            columns.append(f'<div class="capture"><h3>{esc(identifier)}</h3>'
                           f'<p>Source: {esc(record["source_kind"])} · {esc(record.get("status"))}</p>'
                           f'<p>{esc(record.get("error", ""))}</p><div class="frames">{frames}</div></div>')
        if not columns:
            columns.append('<p>No captured frames. This case has not been visually checked.</p>')
        candidates = case.get('counterpart', {}).get('candidates', [])
        sections.append(f'<section data-status="{status}"><h2>{esc(case["display_name"])} '
                        f'<small>{esc(case["id"])}</small></h2><p>Review: <b>{esc(status)}</b>; '
                        f'counterpart candidates: {esc(", ".join(candidates) or "unresolved")}</p>'
                        f'<details><summary>Review evidence key</summary><code>{key}</code></details>'
                        + ''.join(columns) + '</section>')
    counts = dict(Counter(row['review_status'] for row in coverage))
    required_counts = dict(Counter(row['review_status'] for row in coverage if row['requires_visual_comparison']))
    (output / 'coverage.json').write_text(json.dumps({'counts': counts,
        'required_counts': required_counts, 'cases': coverage}, indent=2) + '\n')
    document = '''<!doctype html><meta charset="utf-8"><title>Arte visual review</title>
<style>body{font:15px system-ui;background:#181b20;color:#eee;margin:24px}a{color:#9bd0ff}
section{border-top:1px solid #555;padding:16px 0}small{font-size:12px;color:#aaa}
.frames{display:flex;overflow:auto;gap:10px}figure{margin:0}img{width:320px;height:240px;object-fit:contain;background:#000}
figcaption{color:#aaa}code{overflow-wrap:anywhere}body.pending-only section[data-status=match],
body.pending-only section[data-status=minor_texture_difference]{display:none}</style>
<h1>Arte visual review</h1><p>Captured execution is separate from visual acceptance.
Resonance renders of GC assets are labelled separately from original-game captures.
Minor texture differences may be accepted; missing geometry, effects or incorrect choreography need fixes.</p>
<button onclick="document.body.classList.toggle('pending-only')">Toggle reviewed cases</button>'''
    document += '<p><input placeholder="Filter arte name or case ID" oninput="for(const s of document.querySelectorAll(\'section\')) s.hidden=!s.textContent.toLowerCase().includes(this.value.toLowerCase())"></p>'
    (output / 'index.html').write_text(document + '<p>' + esc(counts) + '</p>' + ''.join(sections))
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalogue', required=True, type=Path)
    parser.add_argument('--captures', action='append', default=[], type=Path)
    parser.add_argument('--reviews', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'images').mkdir(exist_ok=True)
    captures = capture_reports(args.captures, args.output)
    counts = generate(read(args.catalogue), captures,
                      read(args.reviews) if args.reviews else {}, args.output)
    print(json.dumps(counts, sort_keys=True))


if __name__ == '__main__':
    main()
