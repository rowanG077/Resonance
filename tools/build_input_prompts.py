#!/usr/bin/env python3
"""Compose the final SVG atlas and optionally rebuild its lossless KTX2.

Full texture regeneration requires ImageMagick with librsvg and Rust at authoring
time. SVG-only generation uses Python's standard library. No PNG is used.
"""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / 'resources/input-prompts/source'
OUTPUT = ROOT / 'resources/input-prompts'
TILE = 256
COLUMNS = 16

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--svg-only', action='store_true', help='compose only the SVG; leave the runtime layout and texture unchanged')
    args = parser.parse_args(argv)
    def order(source):
        name = source.stem
        if name.startswith('key_') and len(name) == 5 and name[-1].isalpha():
            return (0, ord(name[-1]))
        if name.startswith('key_') and len(name) == 5 and name[-1].isdigit():
            return (1, int(name[-1]))
        if name.startswith('key_f') and name[5:].isdigit():
            return (2, int(name[5:]))
        if name.startswith('key_'): return (3, name)
        if name in ['pad_a','pad_b','pad_x','pad_y']: return (4, ['pad_a','pad_b','pad_x','pad_y'].index(name))
        if name.startswith('pad_'): return (5, name)
        return (6, name)
    sources = sorted(SOURCES.glob('*.svg'), key=order)
    size = (COLUMNS*TILE, ((len(sources)*2+COLUMNS-1)//COLUMNS)*TILE)
    namespace = 'http://www.w3.org/2000/svg'
    ET.register_namespace('', namespace)
    svg = ET.Element(f'{{{namespace}}}svg', width=str(size[0]), height=str(size[1]), viewBox=f'0 0 {size[0]} {size[1]}')
    ET.SubElement(svg, f'{{{namespace}}}title').text = 'Resonance input prompts: normal and pressed states'
    ET.SubElement(svg, f'{{{namespace}}}metadata').text = (OUTPUT/'License.txt').read_text()
    manifest = {}
    pressed = {}
    for index, source in enumerate(sources):
        for is_pressed, rectangles in [(False, manifest), (True, pressed)]:
            state_source = SOURCES/'pressed'/source.name if is_pressed else source
            tile_index = index + (len(sources) if is_pressed else 0)
            x, y = tile_index%COLUMNS*TILE, tile_index//COLUMNS*TILE
            rectangles[source.stem] = [x,y,TILE,TILE]
            tile = ET.parse(state_source).getroot()
            if 'viewBox' not in tile.attrib:
                tile.set('viewBox', f"0 0 {tile.attrib['width']} {tile.attrib['height']}")
            prefix = ('pressed-' if is_pressed else 'normal-') + source.stem
            ids = {element.attrib['id']: prefix + '-' + element.attrib['id'] for element in tile.iter() if 'id' in element.attrib}
            for element in tile.iter():
                for attribute, value in list(element.attrib.items()):
                    if attribute == 'id':
                        value = ids[value]
                    else:
                        value = re.sub(r'url\(#([^)]*)\)', lambda match: f'url(#{ids[match[1]]})', value)
                        if value.startswith('#') and value[1:] in ids:
                            value = '#' + ids[value[1:]]
                    element.set(attribute, value)
            tile.set('id', prefix)
            for attribute, value in [('x', x), ('y', y), ('width', TILE), ('height', TILE)]:
                tile.set(attribute, str(value))
            svg.append(tile)
    ET.indent(svg, space='  ')
    # Conversion happens entirely in staging. A rasterizer or encoder failure
    # must leave the existing runtime layout and texture together.
    with tempfile.TemporaryDirectory(prefix='.input-icons-', dir=OUTPUT) as tmp:
        staged = Path(tmp)
        ET.ElementTree(svg).write(staged/'atlas.svg', encoding='utf-8', xml_declaration=True)
        outputs = ['atlas.svg']
        env = dict(os.environ, XDG_CACHE_HOME=tmp, INKSCAPE_PROFILE_DIR=tmp)
        if not args.svg_only:
            raw = staged/'atlas.rgba'
            subprocess.run([
                'magick', '-background', 'none', f'RSVG:{staged/"atlas.svg"}',
                '-depth', '8', f'rgba:{raw}',
            ], env=env, check=True)
            subprocess.run([
                'cargo', 'run', '--quiet', '--offline', '-p', 'resonance-asset-writer',
                '--example', 'rgba_to_ktx2', '--', str(size[0]), str(size[1]),
                str(raw), str(staged/'atlas.ktx2'),
            ], cwd=ROOT, check=True)
            (staged/'atlas.json').write_text(json.dumps({'size':size,'icons':manifest,'pressed':pressed},indent=2)+'\n')
            outputs = ['atlas.ktx2', 'atlas.json', 'atlas.svg']
        for name in outputs:
            os.replace(staged/name, OUTPUT/name)
    print(f'Built {len(sources)*2} tiles at {size}' + (' (SVG only)' if args.svg_only else ' (SVG, KTX2 and layout)'))

if __name__ == '__main__': main()
