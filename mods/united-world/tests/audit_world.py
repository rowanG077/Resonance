"""Render every tile and orbit every district through the production renderer."""
import argparse
from concurrent.futures import ThreadPoolExecutor,as_completed
import html
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import time


def write_gallery(output,results):
    from PIL import Image,ImageDraw
    sheets=output/'sheets';sheets.mkdir(exist_ok=True)
    for category,prefix in [('tiles','tile-'),('sides',None)]:
        views=[r for r in results if r.get('image') and (r['name'].startswith('tile-') if prefix else not r['name'].startswith('tile-'))]
        for start in range(0,len(views),12):
            sheet=Image.new('RGB',(960,1040),'#151a20');draw=ImageDraw.Draw(sheet)
            for i,result in enumerate(views[start:start+12]):
                preview=Image.open(output/result['image']).convert('RGB');preview.thumbnail((320,232))
                x=i%3*320;y=i//3*260;sheet.paste(preview,(x,y));draw.text((x+4,y+234),result['name'],fill='white')
            sheet.save(sheets/f'{category}-{start//12:02}.png')
    findings_path=output/'visual-findings.json'
    findings=json.loads(findings_path.read_text()) if findings_path.exists() else None
    review='<p>Visual review pending. Successful rendering does not establish visual correctness.</p>'
    if findings:
        review='<h2>Visual review</h2><p>'+html.escape(findings['summary'])+'</p><ul>'
        for item in findings['findings']:
            links=' '.join(f'<a href="{html.escape(name)}">{html.escape(Path(name).stem)}</a>' for name in item.get('images',[]))
            review+=f'<li><strong>{html.escape(item["status"])} — {html.escape(item["title"])}</strong>: {html.escape(item["detail"])} {links}</li>'
        review+='</ul>'
    cards=''.join(f'<figure><a href="{html.escape(r.get("image",""))}"><img loading="lazy" src="{html.escape(r.get("image",""))}" alt="{html.escape(r["label"])}"></a><figcaption>{html.escape(r["label"])} {"" if r["ok"] else "FAILED"}</figcaption></figure>' for r in results)
    links=' '.join(f'<a href="sheets/{p.name}">{p.stem}</a>' for p in sorted(sheets.glob('*.png')))
    evidence=' '.join(f'<a href="{name}">{name}</a>' for name in ['audit-results.json','geometry-tests.log','grounding-and-entrance-tests.log','water-order-test.log','asset-comparison.json','original-world-check.json','manual-review.json'] if (output/name).exists())
    (output/'index.html').write_text('<!doctype html><meta charset="utf-8"><title>United World render audit</title><style>body{background:#151a20;color:#ddd;font:16px sans-serif;line-height:1.5;margin:24px}a{color:#9bd8ff}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr))}figure{margin:8px}img{width:100%}figcaption{padding:6px}li{margin-bottom:12px}</style><h1>United World render audit</h1><p>Overhead tile coverage, four directions per district, caravan sites and wave animation phases. '+str(len(results))+' views; '+str(sum(not r['ok'] for r in results))+' render/resource failures.</p>'+review+'<h2>Validation evidence</h2><p>'+evidence+'</p><h2>Contact sheets</h2><p>'+links+'</p><h2>Full-size views</h2><main>'+cards+'</main>')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mod',type=Path,default=Path('mods/united-world/generated'))
    parser.add_argument('--output',type=Path,default=Path('local/united-world-review/full-audit'))
    parser.add_argument('--workers',type=int,default=3)
    parser.add_argument('--filter',default='')
    parser.add_argument('--resume',action='store_true')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    # Resume only the exact same cooked map and renderer. Reusing an existing
    # screenshot after rebuilding the mod would turn an audit into stale proof.
    digest=hashlib.sha256()
    for path in sorted(args.mod.rglob('*')):
        if path.is_file() and path.name!='.build-stamp.json':
            digest.update(str(path.relative_to(args.mod)).encode());digest.update(path.read_bytes())
    renderer=Path('target/release-fast/examples/overworld_capture')
    digest.update(renderer.read_bytes())
    for path in sorted(Path('local/all-assets/world').rglob('*')):
        if path.is_file():
            stat=path.stat();digest.update(f'{path}:{stat.st_size}:{stat.st_mtime_ns}'.encode())
    audit_input={'fingerprint':digest.hexdigest(),'mod':str(args.mod.resolve())}
    manifest=args.output/'audit-input.json'
    previous=json.loads(manifest.read_text()) if manifest.exists() else None
    if args.resume and previous!=audit_input:
        print('Audit inputs changed; rendering fresh captures.',flush=True)
        args.resume=False
    manifest.write_text(json.dumps(audit_input,indent=2))
    probes=args.output/'probes';probes.mkdir(exist_ok=True)
    composition=json.loads((args.mod/'world/united/composition.json').read_text())
    base={'mods':[str(args.mod)],'story':14000000,'camera_ticks':300,'formation':[1],
          'event_flags':[22,922,923,924,925,926,927,928]}
    jobs=[]
    def add(name,label,position,yaw=0,survey=False,ticks=0):
        probe=dict(base,survey=survey,simulation_ticks=ticks,state={
            'world':'united','position':[*position,0],'heading':yaw,'camera_yaw':yaw,
            'alternate_perspective':False,'map_display':'hidden','mount':'rheairds','altitude':1100})
        jobs.append({'name':name,'label':label,'probe':probe})
    for row in range(9):
        for col in range(12):
            add(f'tile-{row:02}-{col:02}',f'Tile {col}, {row}',[col*6400+3200,row*6400+3200],survey=True)
    for i,district in enumerate(composition['districts']):
        x,y=district['target']
        for direction,yaw,dx,dy in [('north',0,0,1100),('east',math.pi/2,-1100,0),
                                    ('south',math.pi,0,-1100),('west',math.pi*1.5,1100,0)]:
            add(f'district-{i:02}-{direction}',f'{district["name"]}: {direction}',[x+dx,y+dy],yaw)
    landmarks=json.loads((args.mod/'world/landmarks.json').read_text())['united']
    for source in [53,57,60]:
        point=next(e['position'] for e in landmarks if e.get('source_location')==source)
        for direction,yaw,dx,dy in [('north',0,0,700),('east',math.pi/2,-700,0),
                                    ('south',math.pi,0,-700),('west',math.pi*1.5,700,0)]:
            add(f'caravan-{source}-{direction}',f'Caravan {source}: {direction}',[point[0]+dx,point[1]+dy],yaw)
    for tick in [0,40,100]:add(f'wave-phase-{tick:03}',f'Shore animation at tick {tick}',[27494.4,35260],math.pi/2,ticks=tick)
    if args.filter:jobs=[job for job in jobs if args.filter in job['name']]
    def render(job):
        name=job['name'];png=args.output/f'{name}.png';state=png.with_suffix('.json');path=probes/f'{name}.json'
        previous_probe=json.loads(path.read_text()) if path.exists() else None
        path.write_text(json.dumps(job['probe']))
        if not (args.resume and previous_probe==job['probe'] and png.exists() and state.exists()):
            with (args.output/f'{name}.log').open('w') as log:
                result=subprocess.run(['target/release-fast/examples/overworld_capture','local/all-assets',str(path),str(png)],
                                      env=dict(os.environ,WGPU_BACKEND='gl',LP_NUM_THREADS='4'),stdout=log,stderr=subprocess.STDOUT,timeout=180)
            if result.returncode:return dict(name=name,label=job['label'],ok=False,error=f'render exit {result.returncode}')
        snapshot=json.loads(state.read_text())
        return dict(name=name,label=job['label'],ok=not snapshot['transition_failure'] and snapshot['late_asset_reads']==0,
                    late_asset_reads=snapshot['late_asset_reads'],image=png.name)
    started=time.monotonic();results=[]
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures={pool.submit(render,job):job for job in jobs}
        for future in as_completed(futures):
            job=futures[future]
            try:result=future.result()
            except Exception as error:result=dict(name=job['name'],label=job['label'],ok=False,error=str(error))
            results.append(result)
            if len(results)%10==0 or not result['ok'] or len(results)==len(jobs):
                print(f'{len(results)}/{len(jobs)} views; {sum(not r["ok"] for r in results)} failed; {time.monotonic()-started:.0f}s',flush=True)
    results.sort(key=lambda r:r['name'])
    (args.output/'audit-results.json').write_text(json.dumps({'views':results,'seconds':time.monotonic()-started},indent=2))
    write_gallery(args.output,results)
    return int(any(not result['ok'] for result in results))


if __name__=='__main__':raise SystemExit(main())
