#!/usr/bin/env python3
"""Build editable united-world terrain over an imported Resonance library.

Native settlement architecture and painted terrain art come from the local cook.
Coordinates below were sketched against Alex M.
Lazara's Phantasia map: western Euclid, northern islands, eastern Midgard,
southern Alvanista/Ymir and southeastern Freyland. Units are percent of the map.
"""
from copy import deepcopy
import argparse
import hashlib
import json
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
# Broad silhouettes, with intentionally unsettled straits and young land bridges.
CONTINENTS = [
 # Euclid: a long pilgrimage peninsula opening into inhabited southern country.
 [(9,12),(14,13),(17,22),(16,30),(20,34),(19,42),(23,47),(29,49),(35,55),(37,61),(34,65),(30,67),(27,71),(21,71),(17,68),(15,63),(14,59),(15,55),(13,51),(11,45),(10,40),(8,34),(12,28),(10,22)],
 # The snow crown encloses Flanoir between two mountain walls.
 [(34,4),(43,2),(53,8),(57,17),(53,25),(45,29),(34,25),(30,17)],
 # Eastern Midgard. The bridge crosses an actual strait to the western island.
 [(72,12),(79,8),(85,12),(92,4),(90,24),(95,31),(92,40),(97,49),(91,54),(84,55),(80,53),(77,52),(76,49),(73,47),(71,47.5),(69,45),(66,44),(64,42),(62,39),(61,33),(65,25),(68,26)],
 # Southern woodland, two connected cultural basins and a wooded promontory.
 [(36,69),(43,68),(49,70),(53,76),(50,82),(46,81),(42,80),(37,80),(31,75)],
 # Freyland's ports face north; the oasis and ruin road occupy its sheltered bowl.
 [(78,59),(87,59),(93,67),(96,73),(92,78),(90,85),(84,90),(75,86),(72,81),(66,80),(66,74),(71,70),(73,66)],
 # Southern shards and the Katz sanctuary.
 [(18,85),(27,84),(33,89),(43,92),(45,97),(31,98),(24,95),(17,97),(11,92)],
 # The Sylvarant axis stands alone in the inner sea.
 [(38,48),(43,47),(44,52),(41,55),(38,52)],
 # A garden island shelters Latheon; its inlet points back toward the capital.
 [(52,81.5),(55,80.8),(58,82.6),(59,85.4),(57.7,89.7),(54.8,90),(51.6,87.7),(50.6,84.8)],
 # Western Midgard: Hima's crag, the capital, and the foot of the great bridge.
 [(52,44),(55,43),(58,45),(61,49),(63,54),(64,59),(63,63),(67,65),(68,71),(66,76),(62,79),(57,77),(54,74),(51,68),(49,62),(47,58),(47,52),(49,47)],
]

PLACES = {}

def inside(x,y,poly):
    hit=False
    for (a,b),(c,d) in zip(poly,poly[1:]+poly[:1]):
        if (b>y)!=(d>y) and x < (c-a)*(y-b)/(d-b)+a:
            hit=not hit
    return hit

def land(x,y):
    return any(inside(x,y,p) for p in CONTINENTS)

def write_json(root,path,obj):
    target=root/path
    target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps(obj,indent=2)+'\n')

def glb(root,path,groups,collision=False):
    """Write standard GLB, with mesh extras for gameplay collision."""
    buf=bytearray();views=[];access=[];meshes=[];nodes=[]
    def attr(values,n,kind=5126):
        while len(buf)%4:buf.append(0)
        offset=len(buf)
        flat=[v for row in values for v in row]
        buf.extend(struct.pack('<'+('f' if kind==5126 else 'I')*len(flat),*flat))
        view=len(views);views.append({'buffer':0,'byteOffset':offset,'byteLength':len(buf)-offset})
        a={'bufferView':view,'componentType':kind,'count':len(values),'type':{1:'SCALAR',2:'VEC2',3:'VEC3',4:'VEC4'}[n]}
        if n==3:a.update(min=[min(v[i] for v in values) for i in range(3)],max=[max(v[i] for v in values) for i in range(3)])
        access.append(a);return len(access)-1
    for order,(surface,positions,colors) in enumerate(groups):
        # Collision GLBs use standard Y-up. The existing display renderer
        # retains native Z-up local coordinates for meshes and animations.
        display_positions=positions if collision else [(x,-z,h) for x,h,z in positions]
        attributes={'POSITION':attr(display_positions,3)}
        if not collision:
            attributes['COLOR_0']=attr(colors,4)
            attributes['TEXCOORD_0']=attr([(0.,0.)]*len(positions),2)
            attributes['NORMAL']=attr([(0.,0.,1.)]*len(positions),3)
        indices=attr([(i,) for i in range(len(positions))],1,5125)
        meshes.append({'name':surface,'primitives':[{'attributes':attributes,'indices':indices,'mode':4}]})
        node={'name':surface,'mesh':order,'extras':{'resonance_bone':order}}
        if collision:node['extras']=({'role':'ground','order':order} if collision=='field' else {'surface':surface,'order':order})
        nodes.append(node)
    doc={'asset':{'version':'2.0','generator':'Resonance United World'},'scene':0,'scenes':[{'nodes':list(range(len(nodes)))}],'nodes':nodes,'meshes':meshes,'buffers':[{'byteLength':len(buf)}],'bufferViews':views,'accessors':access}
    data=json.dumps(doc,separators=(',',':')).encode()
    data+=b' '*((-len(data))%4);buf+=b'\0'*((-len(buf))%4)
    binary=struct.pack('<III',0x46546c67,2,12+8+len(data)+8+len(buf))+struct.pack('<II',len(data),0x4e4f534a)+data+struct.pack('<II',len(buf),0x004e4942)+buf
    target=root/path;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(binary)

def build(assets,output):
    original=json.loads((assets/'world/landmarks.json').read_text())
    from design import placements, DISTRICTS, DISCOVERIES, source_origin
    from native_regions import contains
    import numpy as np
    global PLACES
    PLACES=placements(original)
    native={e['id']:e for world in original.values() for e in world}
    contextual={source:district for district in DISTRICTS for source in district.members}
    entries=[];discovery=0;transforms={}
    for source in original['sylvarant']+original['tethe_alla']:
        e=deepcopy(source);key=e['id'];district=contextual.get(key)
        if district is None:
            candidates=[d for d in DISTRICTS if (d.anchor<256)==(key<256)
                        and contains(np.array(source['position'])-source_origin(d,native),np.array(d.outline))]
            if candidates:district=min(candidates,key=lambda d:np.linalg.norm(np.array(source['position'])-source_origin(d,native)))
        if key in PLACES:x,y=PLACES[key]
        elif district is not None:
            offset=np.array(district.target)*[768,576]-source_origin(district,native)
            x,y=(np.array(source['position'])+offset)/[768,576]
        elif e.get('marker')=='field_point' or e.get('guidepost'):
            x,y=DISCOVERIES[discovery%len(DISCOVERIES)];discovery+=1
        else:x,y=50,50
        position=[round(x*768,4),round(y*576,4)]
        e.update(id=1024+key,source_location=key,position=position,world='united')
        if e.get('marker')!='field_point':e['automatic']=False
        if 'visit_alias' in e:e['visit_alias']+=1024
        if key in PLACES:
            e['interaction']='active';e['map_listed']=bool(source.get('map_listed')) or key in [11,277]
        elif e.get('marker')!='field_point' and not e.get('guidepost'):
            e['interaction']='disabled';e['marker']='none'
        if key==264:
            e['interaction']='disabled';e['map_listed']=False # Internal bridge handoff, not a third entrance.
        if isinstance(e.get('marker'),dict):
            if key>=256:e['marker']={'model':32+e['marker']['model']}
            # The restored city, rather than the campaign's default ruins.
            if key==7:
                e['marker']={'model':10}
                e['interaction']='disabled' # The actual Luin gates are 43 and 44.
            if key==61:e['marker']={'model':13}
        elif e.get('marker')=='unmodeled':
            e['marker']='none' # It is now part of the complete district scenery.
        if district is not None:
            offset=np.array(position)-source['position']
            transforms[key]=offset
            for rule in e.get('entry_overrides',[]):
                if rule['placement']['kind']=='fixed':
                    p=rule['placement']['position'];rule['placement']['position']=[p[0]+offset[0],p[1]+offset[1],p[2]]
        else:
            e.pop('entry_overrides',None);e.pop('flight_floor',None);e.pop('ship_camera_clearance',None)
        entries.append(e)
    write_json(output,'world/landmarks.json',dict(original,united=entries))
    from landscape import build as build_landscape
    landscape=build_landscape(assets,output,CONTINENTS,entries,PLACES,write_json,glb,original)
    composition=json.loads((output/'world/united/composition.json').read_text())
    composition['entrances']=[1024+key for key in PLACES if key in native]+[1400]
    write_json(output,'world/united/composition.json',composition)
    # Only the small loose camps are grounded separately. All contextual town
    # roots retain the exact native datum, including explicit zero elevations.
    for entry in entries:
        key=entry['source_location']
        if key in transforms or not isinstance(entry.get('marker'),dict):continue
        ground=float(landscape.sample(landscape.height,*entry['position']))
        source=native[key]
        if 'height' in source:
            from settlements import sample_ground
            old_floor=float(sample_ground(assets,'sylvarant' if key<256 else 'tethe-alla',np.array([source['position'][0]]),np.array([source['position'][1]]))[0])
            if not np.isfinite(old_floor):old_floor=source['height']
            entry['height']=ground+source['height']-old_floor
        else:entry['height']=ground
    movement=json.loads((assets/'world/movement.json').read_text())
    movement['flight_zones']['united']=[]
    for zone in movement['flight_zones'].get('tethe_alla',[]):
        point=np.array(zone['position'])
        for district in DISTRICTS:
            if district.anchor<256:continue
            origin=source_origin(district,native)
            if contains(point-origin,np.array(district.outline)):
                moved=deepcopy(zone);moved['position']=(point-origin+np.array(district.target)*[768,576]).tolist()
                movement['flight_zones']['united'].append(moved);break
    write_json(output,'world/movement.json',movement)
    write_json(output,'world/mappings.json',{'2':'united'})
    encounters=json.loads((assets/'world/encounters.json').read_text())
    source=encounters['sylvarant']
    native_regions=sorted(source['regions'].items(),key=lambda item:item[1]['id'])
    names=['Euclid Marches','Northern Crown','Midgard Reach','Ymir Belt','Freyland Sands','Southern Shards','Inner Sea']
    regions={}
    for i,name in enumerate(names):
        groups=native_regions[i%len(native_regions)][1]['groups']
        if isinstance(groups,str):groups=source['regions'][groups]['groups']
        regions[name]={'id':i,'groups':groups}
    def region_at(x,y):
        return next((region for region,p in zip([0,1,2,3,4,5,6,3,2],CONTINENTS) if inside(x,y,p)),6)
    grid=[[region_at((col+.5)*100/24,(row+.5)*100/18) for col in range(24)] for row in range(18)]
    encounters['united']={'arenas':source['arenas'],'grid':grid,'regions':regions}
    encounters['united_docks']=[]
    for dock in encounters['docks']:
        for district in DISTRICTS:
            if district.anchor<256:continue
            origin=source_origin(district,native)
            if not all(contains(np.array(dock[side]['position'])-origin,np.array(district.outline)) for side in ['land','sea']):continue
            offset=np.array(district.target)*[768,576]-origin
            moved=deepcopy(dock)
            for side in ['land','sea']:moved[side]['position']=np.rint(np.array(moved[side]['position'])+offset).astype(int).tolist()
            encounters['united_docks'].append(moved);break
    if len(encounters['united_docks'])!=len(encounters['docks']):
        raise ValueError('Every sailing landing needs a complete authored coastal context')
    for entry in entries:
        if entry.get('guidepost'):
            x,y=entry['position']
            flag=922+grid[min(17,int(y/3200))][min(23,int(x/3200))]
            entry['guidepost']['discovery_flag']=flag
            entry['guidepost']['unlock_flags']=[flag]
    write_json(output,'world/landmarks.json',dict(original,united=entries))
    write_json(output,'world/encounters.json',encounters)
    markers={str(512+i):path for i,path in [(int(k),v) for k,v in json.loads((assets/'world/markers/mappings.json').read_text()).items()] if i<256}
    markers.update({str(512+32+int(k)%256):v for k,v in json.loads((assets/'world/markers/mappings.json').read_text()).items() if 256<int(k)<512})
    write_json(output,'world/markers/mappings.json',markers)
    write_json(output,'world/landmarks.json',dict(original,united=entries))
    # Preserve the original map artwork; the third slot gets its own image.
    menu=json.loads((assets/'ui/menu.json').read_text());menu['images']['united_map']='ui/menu/united-map.ktx2'
    write_json(output,'ui/menu.json',menu)
    source=(HERE/'united.sym').read_text()
    (output/'scripts/world').mkdir(parents=True,exist_ok=True)
    (output/'scripts/world/united.sym').write_text(source)
    build_dungeon(assets,output)
    # An editable vector atlas doubles as a design review artifact.
    svg=['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 750"><rect width="1000" height="750" fill="#123b56"/>']
    svg.append('<image href="relief-map.png" width="1000" height="750"/>')
    for e in entries:
        if e.get('map_listed'):
            x,y=e['position'][0]/76.8,e['position'][1]/76.8
            name=e['name'].replace('&','&amp;').replace('<','&lt;')
            svg.append(f'<circle cx="{x}" cy="{y}" r="3" fill="#fff3b5"><title>{e["id"]}: {name}</title></circle>')
            svg.append(f'<text x="{x+5}" y="{y}" font-size="9" fill="white">{name}</text>')
    svg.append('</svg>');(output/'atlas.svg').write_text('\n'.join(svg))
    print(f'Built 108 terrain tiles and {len(entries)+1} location identities in {output}')

def native_code(calls):
    words=[]
    for opcode,args in calls:
        for a in args:words.extend([0x0200,a&65535,(a>>16)&65535,0x3000,0x4000])
        words.append(0x2000|opcode)
    return words+[0x20ff]

def scenario(main,events):
    """Small original VM script: explicit native calls and registry records."""
    code=list(main);records=[]
    for kind,key,event in events:
        records.extend([0,kind,(key>>16)&65535,key&65535,0,len(code)])
        code.extend(event)
    words=[4+len(records),0,0,len(events)]+records+code
    return struct.pack('>'+'H'*len(words),*words)

def build_dungeon(assets,output):
    owner='fields/confluence-hollow'
    # A narrow entry opens onto a split courtyard and a raised-looking sanctuary.
    # Geometry is freshly authored; enemy and party bodies use the shared catalogue.
    triangles=[];colors=[]
    def quad(x0,y0,x1,y1,h,color):
        for x,y in [(x0,y0),(x1,y0),(x0,y1),(x1,y0),(x1,y1),(x0,y1)]:
            triangles.append([x,h,-y]);colors.append(color)
    quad(-210,-1000,210,-450,0,(.48,.45,.39,1))
    quad(-650,-450,650,700,0,(.37,.41,.37,1))
    quad(-300,700,300,1100,0,(.48,.47,.40,1))
    floor=list(triangles)
    # Twin-colored fusion seams and shattered paving, kept clear of the main route.
    for i in range(11):
        y=-350+i*120
        quad(-540+i*35,y,-480+i*35,y+30,2,(.23,.73,.75,1))
        quad(390-i*30,y,450-i*30,y+30,2,(.64,.42,.78,1))
    # Six jagged buttresses enclose the room, with a low obelisk at its far end.
    def rock(x,y,w,h,color):
        points=[(x-w,y-w),(x+w,y-w),(x+w,y+w),(x-w,y+w)]
        for a,b in zip(points,points[1:]+points[:1]):
            for px,py,z in [(a[0],a[1],0),(b[0],b[1],0),(x,y,h)]:
                triangles.append([px,z,-py]);colors.append(color)
    for x,y in [(-590,-330),(590,-330),(-590,240),(590,240),(-520,620),(520,620)]:rock(x,y,110,300,(.28,.31,.34,1))
    rock(0,970,90,230,(.42,.70,.72,1))
    glb(output,owner+'/scene.glb',[('confluence-stone',triangles,colors)])
    glb(output,owner+'/collision.glb',[('ground',floor,[])],collision='field')
    write_json(output,owner+'/scene.json',{'parts':[{'mesh':owner+'/scene.glb','materials':[{'meshes':[0],'cull':'none'}]}],'settings':'fields/settings.json'})
    write_json(output,owner+'/actors.json',{'models':[{'group':'party'},'props/encounter-beast']})
    write_json(output,owner+'/field.json',{'script':owner+'/scenario.ssb','scene':owner+'/scene.json','collision':owner+'/collision.glb','actors':owner+'/actors.json','audio':{'music':[7],'effects':[1,2,3,4,6,11,12,13,14,15,16,17,18,19,20,21,22,23,27]}})
    write_json(output,'fields/mappings.json',{'2900':'confluence-hollow'})
    directory=json.loads((assets/'game/menu/world-map.json').read_text())
    directory['field_locations']['confluence-hollow']=1400
    write_json(output,'game/menu/world-map.json',directory)
    # Calls use the ordinary field ABI. Entrance is south; exit circle is behind spawn.
    setup=[(0x10,[1,0,-740,0,0,1,0,0]),(0x22,[1]),(0xa6,[1]),(0xa4,[322,0,0,1000]),(0xa1,[2,4500]),(0x74,[1000,0,-960,0,120,150])]
    for actor,x,y in [(201,-330,80),(202,330,420)]:
        setup.append((0x1f,[actor,0,1,x,y,0,180,1,3,actor,163,0,1,1,280,0]))
    setup.append((0x5d,[0]))
    events=[(2,1000,native_code([(0x40,[3000,0,0,0,0])]))]
    for actor in [201,202]:events.append((0,actor,native_code([(0x0f,[actor,0]),(0x5d,[0])])))
    target=output/owner/'scenario.ssb';target.write_bytes(scenario(native_code(setup),events))
    # Extend the registry with independent united-world keys. Original event
    # code remains byte-identical and reachable by its original keys.
    original=(assets/'world/scripts/scenario.ssb').read_bytes()
    base,default,aux,count=struct.unpack_from('>4H',original)
    assert base==4+6*count and aux*2==len(original),'unexpected world scenario layout'
    lookup={(kind,key):pc for kind,key,pc in (struct.unpack_from('>III',original,8+i*12) for i in range(count))}
    code=bytearray(original[base*2:]);registry=bytearray(original[8:base*2])
    added=0
    def direct(key,map_id,position,heading=0):
        nonlocal added
        event=native_code([(0x40,[map_id,*position,heading])])
        registry.extend(struct.pack('>III',1,key,len(code)//2));added+=1
        code.extend(struct.pack('>'+'H'*len(event),*event))
    for source in PLACES:
        if source==1400:continue
        key=1024+source
        if source in [2,11,25,264,265,277]:
            # These retail markers have no live world event. The fused world
            # restores the ranches and opens the bridge/elf settlement directly.
            map_id,position={2:(330,[0,0,0]),11:(193,[0,0,0]),25:(222,[0,0,0]),264:(75,[200,-220,-136]),265:(282,[0,0,0]),277:(2900,[0,-740,0])}[source]
            direct(key,map_id,position)
        else:
            pc=lookup[(1,43 if source==7 else source)]
            registry.extend(struct.pack('>III',1,key,pc));added+=1
    # Discoveries and guideposts retain their native event behavior too.
    locations=json.loads((output/'world/landmarks.json').read_text())['united']
    for entry in locations:
        source=entry['source_location']
        if source not in PLACES and (1,source) in lookup:
            registry.extend(struct.pack('>III',1,entry['id'],lookup[(1,source)]));added+=1
    direct(1400,2900,[0,-740,0])
    payload=registry+code
    header=struct.pack('>4H',base+6*added,default,(len(payload)+8)//2,count+added)
    target=output/'world/scripts/scenario.ssb';target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(header+payload)
    landmarks=json.loads((output/'world/landmarks.json').read_text())
    landmarks['united'].append({'id':1400,'world':'united','source_location':1400,'name':'Confluence Hollow','position':[PLACES[1400][0]*768,PLACES[1400][1]*576],'radius':240,'marker':{'model':64},'height':99.96643829345703,'map_listed':True})
    write_json(output,'world/landmarks.json',landmarks)
    build_confluence_entrance(assets,output)


def build_confluence_entrance(assets,output):
    """A grounded, painted ruin at the end of the fusion-scar footpath."""
    import numpy as np
    from landscape import Mesh,write_glb
    from native_regions import NativeLibrary
    library=NativeLibrary(assets);source=np.array([7000,21000])
    meshes=[];materials=[];textures=[]
    for _,(parts,_) in library.bounds('sylvarant',source-500,source+500):
        for part,primitives in parts:
            for primitive in primitives:
                if primitive['name']!='hol_iseki':continue
                data=primitive['data'].reshape(-1,primitive['data'].shape[-1]);p=data[:,:3].copy()
                center=(p[:,:2].min(0)+p[:,:2].max(0))/2
                p[:,:2]-=center;p[:,1]*=-1;p[:,2]-=p[:,2].min();p*=1.4
                if not np.isfinite(p).all() or np.max(np.ptp(p,axis=0))>600:
                    raise ValueError('Confluence stonework exceeds its doorway footprint')
                mesh=Mesh();mesh.triangles(p,data[:,3:5],data[:,5:9],data[:,9:11]);index=len(meshes)
                meshes.append(('weathered Martel stonework',mesh))
                material=deepcopy(primitive['material']);material['meshes']=[index];material.pop('draw_order',None)
                for binding in ['color','multiply']:
                    if binding not in material:continue
                    path=part['textures'][material[binding]['texture']]
                    if path not in textures:textures.append(path)
                    material[binding]['texture']=textures.index(path)
                materials.append(material)
    rocks=Mesh()
    # Five deliberately placed fragments frame the doorway without obscuring it.
    for x,y,w,h in [(-125,25,45,65),(118,35,42,82),(-87,-42,31,20),(102,-71,28,16),(-18,-105,52,9)]:
        base=np.array([[x-w,y-w,0],[x+w,y-w,0],[x+w,y+w,0],[x-w,y+w,0]])
        cap=np.array([[x-w*.8,y-w*.8,h],[x+w*.6,y-w*.7,h*.8],[x+w*.7,y+w*.7,h*.9],[x-w*.6,y+w*.8,h*1.1]])
        for i in range(4):rocks.quad([base[i],base[(i+1)%4],cap[(i+1)%4],cap[i]],[[0,1],[1,1],[1,0],[0,0]],[.94,.92,.84,1])
        rocks.quad(cap,[[0,0],[1,0],[1,1],[0,1]],[.92,.94,.85,1])
    rock_path='world/tethe-alla/terrain/column-10-row-02/part-0-texture-4.ktx2'
    textures.append(rock_path);materials.append({'meshes':[len(meshes)],'color':{'texture':len(textures)-1},'cull':'none'})
    meshes.append(('exposed fault stones',rocks))
    owner='world/markers/confluence-mouth'
    write_glb(output/(owner+'/model.glb'),meshes)
    write_json(output,owner+'/model.json',{'parts':[{'mesh':owner+'/model.glb','textures':textures,'materials':materials}]})
    mapping=json.loads((output/'world/markers/mappings.json').read_text());mapping['576']='confluence-mouth'
    write_json(output,'world/markers/mappings.json',mapping)


def fingerprint(assets):
    digest=hashlib.sha256()
    digest.update(str(assets.resolve()).encode())
    for path in sorted([*HERE.glob('*.py'),*HERE.glob('*.sym')]):
        digest.update(path.name.encode());digest.update(path.read_bytes())
    # Recooking any source geometry or paint invalidates the launcher cache.
    inputs=[*assets.joinpath('world').rglob('*'),assets/'content.json',assets/'ui/menu.json',assets/'game/menu/world-map.json']
    for path in sorted(inputs):
        if not path.is_file():continue
        stat=path.stat()
        digest.update(f'{path.relative_to(assets)}:{stat.st_size}:{stat.st_mtime_ns}'.encode())
    return digest.hexdigest()


def output_inventory(output):
    return {str(path.relative_to(output)):[path.stat().st_size,path.stat().st_mtime_ns]
            for path in sorted(output.rglob('*')) if path.is_file() and path.name!='.build-stamp.json'}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets',type=Path,default=Path('local/all-assets'))
    parser.add_argument('--output',type=Path,default=HERE/'generated')
    parser.add_argument('--if-needed',action='store_true',help='reuse unchanged generated assets for a fast launch')
    args=parser.parse_args()
    stamp=args.output/'.build-stamp.json';key=fingerprint(args.assets)
    try:
        cached=json.loads(stamp.read_text())
    except (OSError,ValueError):
        cached={}
    if args.if_needed and cached.get('input')==key and cached.get('files')==output_inventory(args.output):
        print('United World assets are current.')
    else:
        stamp.unlink(missing_ok=True)
        build(args.assets,args.output)
        write_json(args.output,stamp.name,{'input':key,'files':output_inventory(args.output)})
