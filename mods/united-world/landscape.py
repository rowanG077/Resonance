"""United-world landscape authoring in the original game's painted art style.

The source cook owns all reference artwork. Generated geometry/albedo is local
mod output; no original disc textures are checked into this repository.
"""
from pathlib import Path
from collections import defaultdict
from compression import zstd
import json
import math
import struct

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage as ndi

W, H = 76800, 57600
STEP = 80
MESH_STEP = 160
TILE = 6400
TEX = 514  # 512 interior texels and a shared one-texel gutter on each side.


def smooth(a, b, x):
    t = np.clip((x-a)/(b-a), 0, 1)
    return t*t*(3-2*t)


def snow_region(x,y):
    """The northern crown is cold; eastern grass settlements keep their biome."""
    return smooth(25,30,x)*(1-smooth(57,62,x))*smooth(31,26,y)


def curve(points, closed=False, samples=12):
    points = np.array(points, float)
    result = []
    count = len(points) if closed else len(points)-1
    for i in range(count):
        p0, p1, p2, p3 = [points[j % len(points)] if closed else points[min(max(j,0),len(points)-1)] for j in (i-1,i,i+1,i+2)]
        for t in np.linspace(0,1,samples,endpoint=False):
            result.append(.5*((2*p1)+(-p0+p2)*t+(2*p0-5*p1+4*p2-p3)*t*t+(-p0+3*p1-3*p2+p3)*t*t*t))
    if not closed: result.append(points[-1])
    return np.array(result)


def read_ktx(path):
    blob=Path(path).read_bytes()
    w,h=struct.unpack_from('<II',blob,20)
    scheme=struct.unpack_from('<I',blob,44)[0]
    offset,size,_=struct.unpack_from('<QQQ',blob,80)
    data=blob[offset:offset+size]
    if scheme==2: data=zstd.decompress(data)
    elif scheme: raise ValueError(f'unsupported texture compression: {path}')
    return np.frombuffer(data,np.uint8).reshape(h,w,4).copy()


def write_ktx(path, pixels, template):
    pixels=np.ascontiguousarray(np.clip(pixels,0,255),dtype=np.uint8)
    h,w=pixels.shape[:2]
    reference=Path(template).read_bytes()
    offset,size=struct.unpack_from('<II',reference,48)
    descriptor=reference[offset:offset+size]
    raw=pixels.tobytes(); compressed=zstd.compress(raw,level=7)
    data_at=104+len(descriptor)
    header=struct.pack('<12s13I2Q',reference[:12],37,1,w,h,0,0,1,1,2,104,len(descriptor),0,0,0,0)
    Path(path).parent.mkdir(parents=True,exist_ok=True)
    Path(path).write_bytes(header+struct.pack('<QQQ',data_at,len(compressed),len(raw))+descriptor+compressed)


class Mesh:
    def __init__(self):
        self.p=[];self.uv=[];self.uv_b=[];self.c=[];self.indices=[]
    def triangles(self, p, uv=None, color=None, uv_b=None):
        p=np.asarray(p,float).reshape(-1,3)
        start=len(self.p);self.p.extend(p.tolist())
        self.uv.extend((np.zeros((len(p),2)) if uv is None else np.asarray(uv).reshape(-1,2)).tolist())
        self.uv_b.extend((np.asarray(self.uv[start:]) if uv_b is None else np.asarray(uv_b).reshape(-1,2)).tolist())
        self.c.extend((np.ones((len(p),4)) if color is None else np.broadcast_to(color,(len(p),4))).tolist())
        self.indices.extend(range(start,start+len(p)))
    def quad(self, p, uv=None, color=None):
        p=np.asarray(p,float);u=np.asarray(uv if uv is not None else [[0,0],[1,0],[1,1],[0,1]],float)
        order=[0,1,2,0,2,3]
        self.triangles(p[order],u[order],color)


def write_glb(path, meshes):
    """Display meshes retain the cooked library's Z-up convention."""
    buf=bytearray();views=[];accessors=[];objects=[];nodes=[]
    def accessor(data,components,kind=5126):
        while len(buf)%4:buf.append(0)
        data=np.asarray(data,dtype='<f4' if kind==5126 else '<u4').reshape(-1,components)
        offset=len(buf);buf.extend(data.tobytes())
        view=len(views);views.append({'buffer':0,'byteOffset':offset,'byteLength':data.nbytes})
        a={'bufferView':view,'componentType':kind,'count':len(data),'type':{1:'SCALAR',2:'VEC2',3:'VEC3',4:'VEC4'}[components]}
        if components==3:a.update(min=data.min(axis=0).tolist(),max=data.max(axis=0).tolist())
        accessors.append(a);return len(accessors)-1
    for name,mesh in meshes:
        if not mesh.indices:continue
        data=np.concatenate([np.asarray(mesh.p),np.asarray(mesh.uv),np.asarray(mesh.c),np.asarray(mesh.uv_b)],axis=1).astype(np.float32)
        unique,inverse=np.unique(data,axis=0,return_inverse=True)
        attrs={'POSITION':accessor(unique[:,:3],3),'TEXCOORD_0':accessor(unique[:,3:5],2),'COLOR_0':accessor(unique[:,5:9],4),'TEXCOORD_1':accessor(unique[:,9:11],2)}
        indices=accessor(inverse[np.asarray(mesh.indices)],1,5125)
        i=len(objects);objects.append({'name':name,'primitives':[{'attributes':attrs,'indices':indices,'mode':4}]})
        nodes.append({'name':f'{name}-{i}','mesh':i,'extras':{'resonance_bone':i}})
    doc={'asset':{'version':'2.0','generator':'United World landscape authoring'},'scene':0,'scenes':[{'nodes':list(range(len(nodes)))}],'nodes':nodes,'meshes':objects,'buffers':[{'byteLength':len(buf)}],'bufferViews':views,'accessors':accessors}
    data=json.dumps(doc,separators=(',',':')).encode();data+=b' '*((-len(data))%4);buf+=b'\0'*((-len(buf))%4)
    output=struct.pack('<III',0x46546c67,2,28+len(data)+len(buf))+struct.pack('<II',len(data),0x4e4f534a)+data+struct.pack('<II',len(buf),0x004e4942)+buf
    Path(path).parent.mkdir(parents=True,exist_ok=True);Path(path).write_bytes(output)


class Geography:
    def __init__(self, continents, entries, places, composition):
        from design import ROUTES, RIDGES, FORESTS, LAND_FORMS
        self.step=STEP
        self.shape=(H//STEP+1,W//STEP+1)
        self.y,self.x=np.mgrid[0:self.shape[0],0:self.shape[1]].astype(np.float32)*STEP
        self.entries=entries;self.places=places;self.composition=composition
        canvas=Image.new('L',(self.shape[1],self.shape[0]));draw=ImageDraw.Draw(canvas)
        self.contours=[]
        for poly in continents:
            points=curve(poly,True,18)*[W/100,H/100]
            self.contours.append(points)
            draw.polygon([tuple(p/STEP) for p in points],fill=255)
        land=np.asarray(canvas)>0
        self.sdf=(ndi.distance_transform_edt(land)-ndi.distance_transform_edt(~land))*STEP
        coast=smooth(0,440,self.sdf)
        # Broad, named shelves keep routes readable. Small relief is deliberate,
        # not a noise field stamped beneath settlements.
        self.height=np.full(self.shape,28.,np.float32)
        shelves=[(23,56,7,10,28),(60,65,9,10,44),(69,45,7,9,35),
                 (81,73,12,9,22),(42,15,12,14,55),(42,75,12,6,30)]
        for x,y,rx,ry,rise in shelves:
            r=np.sqrt(((self.x-x*W/100)/(rx*W/100))**2+((self.y-y*H/100)/(ry*H/100))**2)
            self.height+=rise*(1-smooth(.15,1,r))
        mountain=np.zeros(self.shape,np.float32)
        peak_profile=[.45,.88,.57,1.,.62,.84,.35]
        for name,points,width,peak in RIDGES:
            points=np.array(points)*[W/100,H/100]
            ridge=np.zeros(self.shape,np.float32)
            for i,(a,b) in enumerate(zip(points,points[1:])):
                edge=b-a;t=np.clip(((self.x-a[0])*edge[0]+(self.y-a[1])*edge[1])/(edge@edge),0,1)
                d=np.hypot(self.x-a[0]-t*edge[0],self.y-a[1]-t*edge[1])
                summit=peak*(peak_profile[i%len(peak_profile)]*(1-t)+peak_profile[(i+1)%len(peak_profile)]*t)
                ridge=np.maximum(ridge,np.maximum(0,1-d/width)**1.35*summit)
            mountain=np.maximum(mountain,ridge)
        self.height=(self.height+mountain)*coast
        self.height=np.where(land,self.height,-18)
        self.clear=np.zeros(self.shape,np.float32)
        self.snow=snow_region(self.x/(W/100),self.y/(H/100))
        self.desert=smooth(68,75,self.x/(W/100))*smooth(59,68,self.y/(H/100))*(1-self.snow)
        self.forest=np.zeros(self.shape,np.float32)
        self.forest_outlines=[]
        for name,outline in FORESTS:
            polygon=curve(outline,True,12)*[W/100,H/100]
            mask=self.polygon_mask(polygon)
            inside=ndi.distance_transform_edt(mask)*STEP
            self.forest=np.maximum(self.forest,smooth(0,220,inside))
            self.forest_outlines.append((name,polygon))
        # An exposed fusion tarn gives the new dungeon a particular approach.
        ponds=[[(21.7,47.5),(22.6,47.6),(23.1,48.2),(22.7,48.7),(21.9,48.4)]]
        self.lake=np.zeros(self.shape,bool)
        for polygon in ponds:self.lake|=self.polygon_mask(curve(polygon,True,12)*[W/100,H/100])
        self.water=(~land)|self.lake
        bank=ndi.distance_transform_edt(~self.water)*STEP
        self.height*=smooth(0,300,bank)
        self.height=np.where(self.water,-18,self.height)
        self.route_lines=[]
        roads=np.full(self.shape,1e6,np.float32)
        for name,width,points in ROUTES:
            points=curve(points,False,20)*[W/100,H/100]
            distance=self.distance_to_line(points)
            roads=np.minimum(roads,distance-width)
            self.route_lines.append({'name':name,'half_width':width,'points':points.tolist()})
        self.road=roads+80
        # Camps and guide stones use small level patches, not city-sized disks.
        contextual={member for region in composition.regions for member in region.spec.members}
        for entry in entries:
            source=entry.get('source_location')
            if source in contextual or not isinstance(entry.get('marker'),dict):continue
            x,y=entry['position'];d=np.hypot(self.x-x,self.y-y)
            weight=1-smooth(150,350,d)
            target=float(self.sample(self.height,x,y))
            self.height=self.height*(1-weight)+target*weight
            self.clear=np.maximum(self.clear,weight)
        # This is the final height operation. Native districts are never eroded,
        # bank-graded or road-smoothed after their foundations are installed.
        composition.conform(self)
        for name,rise,outline in LAND_FORMS:
            mask=self.polygon_mask(curve(outline,True,16)*[W/100,H/100])
            signed=(ndi.distance_transform_edt(mask)-ndi.distance_transform_edt(~mask))*STEP
            outside=~composition.core
            self.sdf=np.where(outside,np.maximum(self.sdf,signed),self.sdf)
            shelf=rise*smooth(0,380,signed)
            self.height=np.where(outside&mask,np.maximum(self.height,shelf),self.height)
            self.water=np.where(outside&mask,False,self.water)
        for entry in entries:
            if entry.get('source_location') not in [55,57,58,60]:continue
            x,y=entry['position'];d=np.hypot(self.x-x,self.y-y)
            if self.sample(self.water.astype(float),x,y)>.1:raise ValueError(f'{entry["name"]} needs an authored dry campsite')
            if (composition.core&(d<240)).any():raise ValueError('A movable camp must not overlap a fixed native foundation')
            target=float(self.sample(self.height,x,y));weight=(1-smooth(240,440,d))*(~composition.core)*(~self.water)
            self.height=self.height*(1-weight)+target*weight
            self.clear=np.maximum(self.clear,weight)
        # A binary -18 sea floor against a near-zero land vertex pushes the
        # visible shoreline onto grid corners. Carry a continuous signed shore
        # profile through the sea level instead, outside native foundations.
        away=smooth(0,320,ndi.distance_transform_edt(~composition.core)*STEP)
        softened=ndi.gaussian_filter(self.sdf,1)
        self.sdf=self.sdf*(1-away)+softened*away
        shore=(1-smooth(80,360,np.abs(self.sdf)))*away
        self.height=self.height*(1-shore)+np.clip(self.sdf*.08,-18,28)*shore
        self.water=np.where(composition.core,self.water,self.sdf<0)
        self.land=~self.water
        self.bridge=np.zeros(self.shape,bool)
        self.water_distance=ndi.distance_transform_edt(self.water).astype(np.float32)*STEP
        self.water_mask=self.water.astype(np.float32)
        self.bridge_mask=self.bridge.astype(np.float32)
        self.forest*=1-self.clear
        self.display_height=composition.display_height(self)
        self.dy,self.dx=np.gradient(self.display_height,STEP)
        self.slope=np.hypot(self.dx,self.dy)
        # Planned discoveries may move only a short distance to a usable perch.
        # A bad authored position is reported rather than scattered elsewhere.
        valid=(~self.water)&(self.slope<.55)
        for entry in entries:
            if entry.get('source_location') in places:continue
            ix=min(self.shape[1]-1,round(entry['position'][0]/STEP));iy=min(self.shape[0]-1,round(entry['position'][1]/STEP))
            if valid[iy,ix]:continue
            nearest=ndi.distance_transform_edt(~valid,return_distances=False,return_indices=True)
            nx,ny=nearest[1,iy,ix],nearest[0,iy,ix]
            entry['position']=[int(nx*STEP),int(ny*STEP)]

    def polygon_mask(self,polygon):
        image=Image.new('L',(self.shape[1],self.shape[0]));draw=ImageDraw.Draw(image)
        draw.polygon([tuple(p/STEP) for p in polygon],fill=255)
        return np.asarray(image)>0

    def sample(self,field,x,y,order=1):
        x,y=np.broadcast_arrays(x,y)
        return ndi.map_coordinates(field,[y.ravel()/STEP,x.ravel()/STEP],order=order,mode='nearest').reshape(x.shape)

    def distance_to_line(self,points):
        image=Image.new('L',(self.shape[1],self.shape[0]));draw=ImageDraw.Draw(image)
        draw.line([tuple(p/STEP) for p in points],fill=255,width=1)
        return ndi.distance_transform_edt(np.asarray(image)==0).astype(np.float32)*STEP

    def surface(self,x,y):
        if self.sample(self.water_mask,x,y)>.5:return 'water'
        if self.sample(self.road,x,y)<100:return 'road'
        if self.sample(self.slope,x,y)>.8:return 'barrier'
        if self.sample(self.snow,x,y)>.5:return 'snowfield'
        if self.sample(self.desert,x,y)>.5:return 'desert'
        if self.sample(self.forest,x,y)>.42:return 'forest'
        if self.sample(self.sdf,x,y)<220:return 'beach'
        if self.sample(self.height,x,y)>360:return 'wasteland'
        return 'grassland'


class Palette:
    def __init__(self,assets):
        self.assets=assets
        def load(world,tile,index):return read_ktx(assets/f'world/{world}/terrain/{tile}/part-0-texture-{index}.ktx2')[...,:3].astype(float)
        # Extract grass-only atlas cells; cliff strips include a green lip but
        # must never be tiled across the ground.
        grass=load('sylvarant','column-01-row-04',6)[32:64,:32]
        grass=np.concatenate([grass,grass[:,::-1]],axis=1)
        self.grass=np.concatenate([grass,grass[::-1]],axis=0)
        atlas=load('sylvarant','column-01-row-04',6)
        self.grass_variants=[]
        for patch in [atlas[96:128,96:128],atlas[128:160,32:64]]:
            patch=np.concatenate([patch,patch[:,::-1]],axis=1)
            self.grass_variants.append(np.concatenate([patch,patch[::-1]],axis=0))
        self.sand=load('tethe-alla','column-01-row-04',0)
        self.desert=load('sylvarant','column-01-row-06',0)
        self.snow=load('tethe-alla','column-10-row-02',6)[:64,:64]
        self.rock=load('tethe-alla','column-10-row-02',4)
        self.leaves='world/tethe-alla/terrain/column-01-row-04/part-0-texture-2.ktx2'
        self.bark='world/tethe-alla/terrain/column-01-row-04/part-0-texture-3.ktx2'
        self.water='world/tethe-alla/terrain/column-03-row-04/part-1-texture-1.ktx2'
        self.foam='world/sylvarant/terrain/column-01-row-04/part-1-texture-1.ktx2'
        self.template=assets/self.water

    @staticmethod
    def repeat(texture,x,y,scale):
        h,w=texture.shape[:2]
        coordinates=[np.mod(y/scale*h,h),np.mod(x/scale*w,w)]
        return np.stack([ndi.map_coordinates(texture[...,i],coordinates,order=1,mode='grid-wrap') for i in range(3)],axis=-1)

    def paint(self,field,x,y,map_view=False):
        h=field.sample(field.display_height,x,y)
        slope=field.sample(field.slope,x,y)
        snow=field.sample(field.snow,x,y)
        desert=field.sample(field.desert,x,y)
        forest=field.sample(field.forest,x,y)
        shore=field.sample(field.sdf,x,y)
        road=field.sample(field.road,x,y)
        # Blend the original painted grains at unrelated scales and headings;
        # a single mirrored grass tile makes visible squares from the air.
        warp_x=x+105*np.sin(y/940)+65*np.sin((x+y)/1730)
        warp_y=y+90*np.sin(x/1170)-55*np.cos((x-y)/1370)
        grass=self.repeat(self.grass,warp_x,warp_y,910)*.55
        grass+=self.repeat(self.grass_variants[0],warp_x*.798-warp_y*.603,warp_x*.603+warp_y*.798,670)*.3
        grass+=self.repeat(self.grass_variants[1],warp_x*.342+warp_y*.94,-warp_x*.94+warp_y*.342,1510)*.15
        sand=self.repeat(self.sand,x,y,540)
        dune=self.repeat(self.desert,x,y,720)
        ice=self.repeat(self.snow,x,y,950)
        rock=self.repeat(self.rock,x,h*6+y*.08,1150)
        def mix(a,b,t):return a*(1-t[...,None])+b*t[...,None]
        color=mix(grass,grass*[.68,.85,.72],forest*.65)
        meadow=(.5+.5*np.sin(x/670+y/830)*np.sin(y/530-x/1700))*.24
        color=mix(color,sand,meadow*(1-forest))
        color=mix(color,dune,desert)
        color=mix(color,ice,snow)
        stone=smooth(.28,.7,slope)*smooth(80,260,h)
        color=mix(color,rock,stone*(1-snow*.5))
        color=mix(color,sand*(1-snow[...,None]*.2),1-smooth(120,420,shore))
        track=(1-smooth(45,125,road))*(1-snow*.6)
        color=mix(color,sand*[.82,.76,.61],track*.86)
        nx=-field.sample(field.dx,x,y);ny=field.sample(field.dy,x,y)
        light=(.85+(.24*nx+.34*ny)/np.sqrt(1+nx*nx+ny*ny))
        broad=.97+.035*np.sin(x/650+y/730)+.025*np.sin(x/1510-y/1200)
        color*=np.clip(light*broad,.48,1.14)[...,None]
        wet=field.sample(field.water.astype(float),x,y)>.5
        if map_view:
            ocean=np.empty_like(color);depth=np.clip(-shore/1800,0,1)
            ocean[...,0]=31-depth*14;ocean[...,1]=117-depth*51;ocean[...,2]=145-depth*36
            color=np.where(wet[...,None],ocean,color)
        else:color=np.where(wet[...,None],np.array([24,80,106]),color)
        result=np.concatenate([np.clip(color,0,255),np.full((*h.shape,1),255)],axis=-1).astype(np.uint8)
        return field.composition.paint(x,y,result,map_view,wet)


def build_reserves(field,composition):
    from surfaces import forest_meshes
    return forest_meshes(field,composition)


def build(assets,output,continents,entries,places,write_json,collision_glb,original):
    from native_regions import Composition, fan, valid_polygon
    from surfaces import sea_crossing,coast_meshes
    print('Composing authored regions, complete foundations and deliberate travel routes...',flush=True)
    composition=Composition(assets,original)
    field=Geography(continents,entries,places,composition)
    composition.export(output,write_json,field)
    palette=Palette(assets)
    # Native sea meshes rely on neighboring seabed coverage. Moving a district
    # can expose holes beneath their translucent waves. A continuous deep bed
    # supplies that coverage without changing any coast or support collision.
    write_ktx(output/'world/united/ocean-bed.ktx2',np.full((4,4,4),[24,80,106,255],np.uint8),palette.template)
    # The tree silhouettes occupy the upper-right atlas cell, not its full
    # width. A separate repeatable cutout prevents unrelated cells bleeding in.
    fringe=read_ktx(assets/palette.bark)[:16,32:64]
    write_ktx(output/'world/united/forest-fringe.ktx2',fringe,palette.template)
    foam=read_ktx(assets/palette.foam)
    foam[:,:,3]=(foam[:,:,3]*smooth(160,240,foam[:,:,0].astype(float))).astype(np.uint8)
    foam[:,:,:3]=255
    write_ktx(output/'world/united/shore-foam.ktx2',foam,palette.template)
    reserves=build_reserves(field,composition)
    tiles=[];vertex_count=0;coast_segments=list(composition.coast_segments)
    for row in range(9):
        for col in range(12):
            owner=f'world/united/terrain/column-{col:02}-row-{row:02}'
            candidates=composition.nearby(np.array([col*TILE,row*TILE]),np.array([(col+1)*TILE,(row+1)*TILE]))
            x,y=np.meshgrid(np.arange(41)*MESH_STEP+col*TILE,np.arange(41)*MESH_STEP+row*TILE)
            heights=field.sample(field.display_height,x,y)
            points=np.stack([x,y,heights],axis=-1).reshape(-1,3)
            colors=np.ones((len(points),4));uv=(np.stack([(x-col*TILE)/TILE,(y-row*TILE)/TILE],axis=-1)*(TEX-2)+1).reshape(-1,2)/TEX
            data=np.concatenate([points,uv,colors],axis=1)
            terrain_mesh=Mesh();water=Mesh();groups=defaultdict(list)
            def append_display(mesh,polygon,faces=None):
                faces=(fan(polygon) if faces is None else faces).reshape(-1,9);local=faces[:,:3].copy()
                local[:,0]-=col*TILE+TILE/2;local[:,1]=row*TILE+TILE/2-local[:,1]
                mesh.triangles(local,faces[:,3:5],faces[:,5:9])
            for j in range(40):
                for i in range(40):
                    a=j*41+i;b=a+1;c=a+41;d=c+1
                    for triangle in [(a,c,b),(b,c,d)]:
                        tri=data[list(triangle)]
                        polygons=composition.outside(tri,candidates)
                        for polygon in polygons:
                            if not valid_polygon(polygon):continue
                            polygon=composition.stitch(polygon,candidates)
                            center=None
                            if len(polygon)>3:
                                center=polygon.mean(0);center[2]=field.sample(field.display_height,*center[:2])
                            faces=fan(polygon,center)
                            append_display(terrain_mesh,polygon,faces)
                            for face in faces:
                                sea_face=face[:,:3].copy();sea_face[:,2]-=.5
                                crossing=sea_crossing(sea_face)
                                if crossing is not None:coast_segments.append(crossing)
                                center=face[:,:2].mean(0);surface=field.surface(*center)
                                local=face[:,:3].copy();local[:,0]-=col*TILE+TILE/2;local[:,1]-=row*TILE+TILE/2
                                groups[surface].extend(local[:,[0,2,1]].tolist())
            # One continuous sea plane is occluded by the solid terrain. Testing
            # only coarse grid corners leaves water holes at clipped coastlines.
            sea_x,sea_y=np.meshgrid(np.arange(11)*640+col*TILE,np.arange(11)*640+row*TILE)
            sea=np.column_stack([sea_x.ravel(),sea_y.ravel(),np.full(121,.5)])
            distance=-field.sample(field.sdf,*sea[:,:2].T)
            shallows=1-smooth(0,700,np.maximum(0,distance))
            tint=np.column_stack([.76+.13*shallows,.86+.11*shallows,.98-.07*shallows,np.ones(121)])
            wet=np.column_stack([sea,sea[:,:2]/800,tint])
            for j in range(10):
                for i in range(10):
                    a=j*11+i;b=a+1;c=a+11;d=c+1
                    append_display(water,wet[[a,c,b]])
                    append_display(water,wet[[b,c,d]])
            native_groups=[]
            for kind,positions in composition.collision_ordered[(col,row)]:
                p=np.asarray(positions).copy();p[:,0]-=col*TILE+TILE/2;p[:,1]-=row*TILE+TILE/2
                native_groups.append((kind,p[:,[0,2,1]].tolist(),[]))
            px,py=np.meshgrid((np.arange(TEX)-.5)*TILE/(TEX-2)+col*TILE,(np.arange(TEX)-.5)*TILE/(TEX-2)+row*TILE)
            albedo=palette.paint(field,px,py)
            write_ktx(output/(owner+'/albedo.ktx2'),albedo,palette.template)
            parts=[]
            bed=Mesh();bed.quad([[-TILE/2,-TILE/2,-90],[TILE/2,-TILE/2,-90],[TILE/2,TILE/2,-90],[-TILE/2,TILE/2,-90]])
            write_glb(output/(owner+'/ocean-bed.glb'),[('continuous deep seabed',bed)])
            parts.append({'mesh':owner+'/ocean-bed.glb','textures':['world/united/ocean-bed.ktx2'],'materials':[{'meshes':[0],'color':{'texture':0},'vertex_color':False}]})
            if terrain_mesh.indices:
                write_glb(output/(owner+'/land.glb'),[('interstitial terrain',terrain_mesh)])
                parts.append({'mesh':owner+'/land.glb','textures':[owner+'/albedo.ktx2'],'materials':[{'meshes':[0],'color':{'texture':0,'wrap_u':'clamp','wrap_v':'clamp'},'vertex_color':False}]})
            collision_glb(output,owner+'/collision.glb',native_groups+[(surface,p,[]) for surface,p in groups.items() if p],True)
            if water.indices:
                water.uv_b=[[v,-u] for u,v in water.uv]
                write_glb(output/(owner+'/water.glb'),[('sea',water)])
                parts.append({'resource':2,'mesh':owner+'/water.glb','textures':[palette.water],
                              'materials':[{'meshes':[0],'color':{'texture':0},'multiply':{'texture':0},'depth_write':False}],
                              'texture_animations':[{'texture':0,'sequence':{'kind':'scroll','step':[.001,.001],'ticks':1000}}]})
            if (col,row) in reserves:
                leaves,trunks=reserves[(col,row)]
                meshes=[];materials=[]
                for name,mesh,texture,blend in [('drawn reserve canopy',leaves,0,False),('native tree fringe',trunks,1,True)]:
                    if not mesh.indices:continue
                    binding={'texture':texture}
                    if blend:binding['wrap_v']='clamp'
                    materials.append({'meshes':[len(meshes)],'color':binding,'cull':'none','blend':blend,'draw_order':[2000 if not blend else 4000]})
                    meshes.append((name,mesh))
                if meshes:
                    write_glb(output/(owner+'/reserves.glb'),meshes)
                    parts.append({'mesh':owner+'/reserves.glb','textures':[palette.leaves,'world/united/forest-fringe.ktx2'],'materials':materials})
            parts.extend(composition.parts[(col,row)])
            if composition.forest_fringe[(col,row)]:
                fringe_meshes=composition.forest_fringe[(col,row)]
                write_glb(output/(owner+'/forest-closures.glb'),fringe_meshes)
                parts.append({'mesh':owner+'/forest-closures.glb','textures':['world/united/forest-fringe.ktx2'],
                              'materials':[{'meshes':list(range(len(fringe_meshes))),'draw_order':list(range(3800,3800+len(fringe_meshes))),
                                            'color':{'texture':0,'wrap_v':'clamp'},'blend':True,'cull':'none'}]})
            if composition.aprons[(col,row)]:
                joins=composition.aprons[(col,row)]
                write_glb(output/(owner+'/joins.glb'),joins)
                parts.append({'mesh':owner+'/joins.glb','textures':[owner+'/albedo.ktx2'],
                              'materials':[{'meshes':list(range(len(joins))),'draw_order':list(range(3000,3000+len(joins))),
                                            'color':{'texture':0,'wrap_u':'clamp','wrap_v':'clamp'},'blend':True,'depth_write':False}]})
            write_json(output,owner+'/terrain.json',{'collision':owner+'/collision.glb','parts':parts})
            tiles.append({'column':col,'row':row,'base':owner+'/terrain.json'})
            vertex_count+=len(terrain_mesh.p)
        print(f'  transition terrain row {row+1}/9 stitched',flush=True)
    waves=coast_meshes(coast_segments,composition,read_ktx(assets/palette.foam).shape[0])
    for (col,row),mesh in waves.items():
        owner=f'world/united/terrain/column-{col:02}-row-{row:02}'
        write_glb(output/(owner+'/wave-ribbons.glb'),[('shore-following wave ribbon',mesh)])
        definition=json.loads((output/(owner+'/terrain.json')).read_text())
        definition['parts'].append({'resource':2,'mesh':owner+'/wave-ribbons.glb','textures':['world/united/shore-foam.ktx2'],
                                   'materials':[{'meshes':[0],'color':{'texture':0,'wrap_v':'clamp'},'blend':True,'depth_write':False,'draw_order':[4000]}],
                                   'texture_animations':[{'texture':0,'sequence':{'kind':'atlas','grid':[1,8],'interval':20}}]})
        write_json(output,owner+'/terrain.json',definition)
    write_json(output,'world/united/world.json',{'terrain':tiles})
    mx,my=np.meshgrid((np.arange(768)+.5)*W/768,(np.arange(576)+.5)*H/576)
    preview=Image.fromarray(palette.paint(field,mx,my,True),'RGBA')
    chart=preview.resize((384,288),Image.Resampling.LANCZOS)
    write_ktx(output/'ui/menu/united-map.ktx2',np.asarray(chart),palette.template)
    preview.save(output/'relief-map.png')
    write_json(output,'world/united/art-direction.json',{'interstitial_vertices':vertex_count,'native_districts':len(composition.regions),
               'layout':'authored district composition, complete native foundations, explicit itineraries and reserves',
               'routes':field.route_lines,'scattered_groves':0,'automatic_road_graph':False})
    return field
