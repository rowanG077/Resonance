#!/usr/bin/env python3
"""Make labelled phase sheets from real PNG captures, without review decisions."""
import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
from gallery import capture_reports, read


def phase_images(record):
    images = sorted(record['images'], key=lambda image: image['age'])
    if not images:
        return []
    def tagged(*tags):
        return next((image for image in images
                     if image.get('moment', image.get('phase')) in tags), None)
    release = tagged('scene_active') or tagged('release')
    start = record.get('active_scene_age', record.get('release_age'))
    if start is None:
        start = release['age'] if release else 0
    cleanup = tagged('cleanup') or images[-1]
    closest = lambda age: min(images, key=lambda image: abs(image['age'] - age))
    return [release or closest(start), closest(start + 12), closest(start + 24),
            closest((start + cleanup['age']) / 2),
            tagged('impact') or closest(start + 40), cleanup]


def sheet(case, records, output, width=400):
    height = width * 3 // 4
    rows = 6
    canvas = Image.new('RGB', (width * len(records), 70 + rows * (height + 27)), '#181b20')
    draw = ImageDraw.Draw(canvas)
    font = ImageFont.load_default(size=15)
    draw.text((10, 5), f"{case['display_name']} — {case['id']}", fill='white', font=font)
    draw.text((10, 28), 'Actual capture ages shown; fixture/camera alignment still requires review.',
              fill='#bbbbbb', font=font)
    for column, (identifier, record) in enumerate(records):
        x = column * width
        draw.text((x + 8, 49), f"{identifier} [{record['source_kind']}]", fill='#a9d8ff', font=font)
        for row, image in enumerate(phase_images(record)):
            y = 70 + row * (height + 27)
            pixels = Image.open(output / image['path']).convert('RGB')
            pixels.thumbnail((width, height), Image.Resampling.LANCZOS)
            canvas.paste(pixels, (x + (width - pixels.width) // 2, y))
            label = image.get('moment', image.get('phase', 'sample'))
            draw.text((x + 8, y + height + 3), f"age {image['age']} · {label}", fill='white', font=font)
    name = case['id'].replace(':', '-') + '.png'
    canvas.save(output / name)
    return name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--catalogue', required=True, type=Path)
    parser.add_argument('--captures', required=True, action='append', type=Path)
    parser.add_argument('--case', action='append', default=[])
    parser.add_argument('--character', type=int)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'images').mkdir(exist_ok=True)
    selected = [case for case in read(args.catalogue)['cases']
                if case['edition'] == 'steam'
                and (not args.case or case['id'] in args.case)
                and (args.character is None or case['binding'].get('character') == args.character)]
    identifiers = {identifier for case in selected
                   for identifier in [case['id'], *case.get('counterpart', {}).get('candidates', [])]}
    identifiers.update('original:' + identifier for identifier in list(identifiers)
                       if identifier.startswith('gamecube:'))
    captures = capture_reports(args.captures, args.output, identifiers)
    generated = []
    for case in selected:
        primary = captures.get(case['id'])
        if not primary or not primary['images']:
            continue
        candidates = case.get('counterpart', {}).get('candidates', [])
        keys = [case['id'], *candidates]
        keys += ['original:' + key for key in candidates if key.startswith('gamecube:')]
        records = [(key, captures[key]) for key in keys
                   if key in captures and captures[key]['images']]
        generated.append({'id': case['id'], 'path': sheet(case, records, args.output),
                          'capture_status': primary['status'], 'review_status': 'pending',
                          'counterpart_candidates': candidates})
    (args.output / 'sheets.json').write_text(json.dumps(generated, indent=2) + '\n')
    print(f'Generated {len(generated)} phase sheets; no visual verdicts assigned.')


if __name__ == '__main__':
    main()
