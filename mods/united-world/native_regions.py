"""Rigid landscape assembly and exact polygon seams for the authored atlas.

A district has one transform for scenery, collision, shoreline and landmarks.
Native UVs, paint, tree silhouettes, terraces and material recipes stay intact.
Only the distant transition ring is resculpted. No town gets fitted to a raster
heightmap and no building is independently raised by an entrance's floor.
"""
from collections import defaultdict
from copy import deepcopy
from pathlib import Path
import json
import math
import numpy as np
from scipy import ndimage as ndi
from landscape import Mesh, write_glb, read_ktx, smooth, TEX
from settlements import GLB
from design import DISTRICTS, source_origin

TILE=6400
FEATHER=2200
SURFACES=['barrier','grassland','road','wasteland','snowfield','desert','forest','deep_forest','beach','water','bridge','pier']
WALKABLE=set(SURFACES)-{'barrier','water'}


def cross2(a,b):
    return a[...,0]*b[...,1]-a[...,1]*b[...,0]


def area(poly):
    return cross2(poly,np.roll(poly,-1,axis=0)).sum()/2


def clip_half(poly,a,b,inside=True):
    if len(poly)<3:return np.empty((0,poly.shape[1]))
    d=cross2(b-a,poly[:,:2]-a)
    if not inside:d=-d
    kept=d>=-1e-7
    if kept.all():return poly
    if not kept.any():return np.empty((0,poly.shape[1]))
    result=[]
    for i in range(len(poly)):
        j=(i+1)%len(poly)
        if kept[i]:result.append(poly[i])
        if kept[i]!=kept[j]:
            t=d[i]/(d[i]-d[j]);result.append(poly[i]+t*(poly[j]-poly[i]))
    return np.asarray(result)


def clip_polygon(poly,boundary):
    for a,b in zip(boundary,np.roll(boundary,-1,axis=0)):
        poly=clip_half(poly,a,b)
        if len(poly)<3:break
    return poly


def subtract_polygon(poly,boundary):
    result=[]
    for a,b in zip(boundary,np.roll(boundary,-1,axis=0)):
        outside=clip_half(poly,a,b,False)
        if len(outside)>=3:result.append(outside)
        poly=clip_half(poly,a,b)
        if len(poly)<3:break
    return result


def fan(poly,center=None):
    if len(poly)<3:return np.empty((0,3,poly.shape[-1]))
    if center is not None:return np.asarray([[center,a,b] for a,b in zip(poly,np.roll(poly,-1,axis=0))])
    return np.asarray([[poly[0],poly[i],poly[i+1]] for i in range(1,len(poly)-1)])


def contains(points,polygon):
    result=np.ones(points.shape[:-1],bool)
    for a,b in zip(polygon,np.roll(polygon,-1,axis=0)):
        result&=cross2(b-a,points-a)>=-1e-5
    return result


def distance(points,polygon):
    result=np.full(points.shape[:-1],np.inf)
    for a,b in zip(polygon,np.roll(polygon,-1,axis=0)):
        edge=b-a;t=np.clip(((points-a)*edge).sum(axis=-1)/(edge@edge),0,1)
        result=np.minimum(result,np.linalg.norm(points-a-t[...,None]*edge,axis=-1))
    return np.where(contains(points,polygon),-result,result)


def rectangle(x0,y0,x1,y1):
    return np.array([[x0,y0],[x1,y0],[x1,y1],[x0,y1]])


def inset_polygon(polygon,amount):
    result=polygon.copy()
    for a,b in zip(polygon,np.roll(polygon,-1,axis=0)):
        edge=b-a;normal=np.array([-edge[1],edge[0]])/np.linalg.norm(edge)*amount
        result=clip_half(result,a+normal,b+normal)
        if len(result)<3:break
    return result


def outset_polygon(polygon,amount):
    result=rectangle(*(polygon.min(0)-amount*2),*(polygon.max(0)+amount*2))
    for a,b in zip(polygon,np.roll(polygon,-1,axis=0)):
        edge=b-a;normal=np.array([-edge[1],edge[0]])/np.linalg.norm(edge)*amount
        result=clip_half(result,a-normal,b-normal)
    return result


def is_forest(name):
    return 'forest' in name.lower() or 'fore_' in name.lower()


def is_mountain(name):
    return 'mount' in name.lower() or 'yama' in name.lower()


def is_ground(primitive):
    """Only landscape surfaces may supply terrain heights and transition paint.

    Roofs and tree crowns can be opaque and close to walking collision too.
    Sampling them as ground raises the connecting terrain into the canopy and
    projects architectural atlas cells onto distant hills.
    """
    if primitive['forest']:return False
    name=primitive['name'].lower()
    return is_mountain(name) or any(word in name for word in
        ['tairiku','ground','jimen','field','gake','sougen','sabaku','kotou',
         'under_sea','undersea','sea_bottom','sea_deep_edge']) or name.split('/')[0] in ['him_base','thu_base']


def is_ocean(primitive):
    name=primitive['name'].lower()
    return ('ocean' in name or 'fsea' in name) and primitive['material'].get('multiply') is not None


def texture_sample(texture,uv,binding):
    """Match the renderer's texel centres, linear filtering and wrap modes."""
    h,w=texture.shape[:2];xy=np.nan_to_num(uv,nan=0,posinf=0,neginf=0)*[w,h]-.5
    lower=np.floor(xy).astype(int);fraction=xy-lower
    def address(p,n,mode):
        if mode=='clamp':return np.clip(p,0,n-1)
        if mode=='mirror':
            p=p%(n*2);return np.minimum(p,n*2-1-p)
        return p%n
    x0=address(lower[...,0],w,binding.get('wrap_u','repeat'));x1=address(lower[...,0]+1,w,binding.get('wrap_u','repeat'))
    y0=address(lower[...,1],h,binding.get('wrap_v','repeat'));y1=address(lower[...,1]+1,h,binding.get('wrap_v','repeat'))
    fx=fraction[...,0,None];fy=fraction[...,1,None]
    if binding.get('nearest_mag'):return texture[np.where(fraction[...,1]<.5,y0,y1),np.where(fraction[...,0]<.5,x0,x1)]
    return (texture[y0,x0]*(1-fx)+texture[y0,x1]*fx)*(1-fy)+(texture[y1,x0]*(1-fx)+texture[y1,x1]*fx)*fy


def valid_polygon(poly):
    if len(poly)<3:return False
    return any(np.linalg.norm(np.cross(poly[i,:3]-poly[0,:3],poly[i+1,:3]-poly[0,:3]))>1e-5 for i in range(1,len(poly)-1))


def rooted_forest_faces(faces,polygon):
    """Retain cut foliage components which actually continue the core forest.

    Clipping a nearby forest can leave isolated trunk arcs or leaf scraps. They
    are not a continuation of the district, even if their source mesh also
    contains trees inside it. Join across material and tile boundaries first.
    """
    parent=list(range(len(faces)));vertices={}
    def root(i):
        while parent[i]!=i:
            parent[i]=parent[parent[i]];i=parent[i]
        return i
    for i,face in enumerate(faces):
        for vertex in face:
            key=tuple(np.round(vertex[:3],3))
            other=vertices.setdefault(key,i)
            a,b=root(i),root(other)
            if a!=b:parent[a]=b
    touching=np.any(np.abs(distance(faces[:,:,:2],polygon))<.02,axis=1)
    rooted={root(i) for i in np.flatnonzero(touching)}
    return np.array([root(i) in rooted for i in range(len(faces))])


def split_tiles(poly):
    # A vertical wall can have zero extent on an exact tile boundary. Shrinking
    # both bounds makes its tile interval empty and silently removes the wall.
    lo=np.clip(np.floor(poly[:,:2].min(axis=0)/TILE).astype(int),[0,0],[11,8])
    hi=np.clip(np.floor(poly[:,:2].max(axis=0)/TILE).astype(int),[0,0],[11,8])
    for row in range(max(0,lo[1]),min(8,hi[1])+1):
        for col in range(max(0,lo[0]),min(11,hi[0])+1):
            clipped=clip_polygon(poly,rectangle(col*TILE,row*TILE,(col+1)*TILE,(row+1)*TILE))
            if valid_polygon(clipped):yield (col,row),clipped


class SurfaceSampler:
    """Spatially indexed, ordered triangle probes for native support surfaces."""
    def __init__(self,faces):
        self.triangles=np.array([face for _,face in faces],float)
        self.kinds=np.array([SURFACES.index(kind) for kind,_ in faces],np.int16)
        self.priority=np.array([0 if kind in WALKABLE else (1 if kind=='barrier' else 2) for kind,_ in faces])
        self.bins=defaultdict(list)
        if not len(self.triangles):return
        t=self.triangles
        self.den=cross2(t[:,1,:2]-t[:,0,:2],t[:,2,:2]-t[:,0,:2])
        for i,face in enumerate(t):
            if self.den[i]>=-1e-8:continue
            lo=np.floor((face[:,:2].min(0)-.01)/512).astype(int)
            hi=np.floor((face[:,:2].max(0)+.01)/512).astype(int)
            for row in range(lo[1],hi[1]+1):
                for col in range(lo[0],hi[0]+1):self.bins[(col,row)].append(i)
        self.bins={key:np.array(value,dtype=int) for key,value in self.bins.items()}

    def sample(self,x,y,highest=False):
        x,y=np.broadcast_arrays(x,y);shape=x.shape;x=x.ravel();y=y.ravel()
        height=np.full(len(x),np.nan);kind=np.full(len(x),-1,np.int16)
        keys=np.floor(np.stack([x,y],axis=-1)/512).astype(int)
        unique,inverse=np.unique(keys,axis=0,return_inverse=True)
        for i,key in enumerate(unique):
            candidate=self.bins.get(tuple(key))
            if candidate is None:continue
            points=np.where(inverse==i)[0]
            a,b,c=np.moveaxis(self.triangles[candidate],1,0)
            dx=x[points][None,:]-a[:,0,None];dy=y[points][None,:]-a[:,1,None]
            den=self.den[candidate,None]
            u=(dx*(c[:,1,None]-a[:,1,None])-dy*(c[:,0,None]-a[:,0,None]))/den
            v=((b[:,0,None]-a[:,0,None])*dy-(b[:,1,None]-a[:,1,None])*dx)/den
            inside=(u>=-1e-5)&(v>=-1e-5)&(u+v<=1.00001)
            h=a[:,2,None]+u*(b[:,2,None]-a[:,2,None])+v*(c[:,2,None]-a[:,2,None])
            score=np.where(inside,-h if highest else self.priority[candidate,None]*(len(self.triangles)+1)+candidate[:,None],10**12)
            chosen=score.argmin(axis=0);valid=inside[chosen,np.arange(len(points))]
            height[points[valid]]=h[chosen[valid],np.arange(len(points))[valid]]
            kind[points[valid]]=self.kinds[candidate[chosen[valid]]]
        return height.reshape(shape),kind.reshape(shape)


class NativeLibrary:
    def __init__(self,assets):
        self.assets=Path(assets);self.tiles={};self.textures={}

    def texture(self,path):
        if path not in self.textures:self.textures[path]=read_ktx(self.assets/path)/255
        return self.textures[path]

    def tile(self,world,col,row):
        key=(world,col,row)
        if key in self.tiles:return self.tiles[key]
        owner=f'world/{world}/terrain/column-{col:02}-row-{row:02}'
        definition=json.loads((self.assets/owner/'terrain.json').read_text())
        parts=[]
        for part_i,part in enumerate(definition['parts']):
            g=GLB(self.assets/part['mesh']);primitives=[]
            materials={mesh:(material,i) for i,material in enumerate(part['materials']) for mesh in material['meshes']}
            for node_i,node in enumerate(g.doc['nodes']):
                if 'mesh' not in node:continue
                transform=g.transform(node_i)
                for primitive in g.doc['meshes'][node['mesh']]['primitives']:
                    if primitive.get('mode',4)!=4:continue
                    attrs=primitive['attributes'];p=g.array(attrs['POSITION']).astype(float)
                    p=p@transform[:3,:3].T+transform[:3,3]
                    p=np.stack([p[:,0]+col*TILE+TILE/2,row*TILE+TILE/2-p[:,1],p[:,2]],axis=-1)
                    uv=g.array(attrs['TEXCOORD_0']) if 'TEXCOORD_0' in attrs else np.zeros((len(p),2))
                    uv_b=g.array(attrs['TEXCOORD_1']) if 'TEXCOORD_1' in attrs else uv
                    color=g.array(attrs['COLOR_0']) if 'COLOR_0' in attrs else np.ones((len(p),4))
                    material,material_i=materials.get(node['mesh'],({},-1))
                    order=material.get('draw_order',material.get('meshes',[]))
                    draw_order=order[material['meshes'].index(node['mesh'])] if order else node['mesh']
                    index=g.array(primitive['indices']).reshape(-1,3).astype(int)
                    data=np.concatenate([p,uv,color,uv_b],axis=1)[index]
                    primitives.append({'data':data,'mesh':node['mesh'],'name':node.get('name',''),
                                       'material':material,'material_i':material_i,'draw_order':draw_order})
            parts.append((part,primitives))
        forest_textures={part['textures'][p['material']['color']['texture']] for part,ps in parts for p in ps
                         if is_forest(p['name']) and p['material'].get('color')}
        mountain_textures={part['textures'][p['material']['color']['texture']] for part,ps in parts for p in ps
                           if is_mountain(p['name']) and p['material'].get('color')}
        for part,ps in parts:
            for p in ps:
                binding=p['material'].get('color')
                p['forest']=is_forest(p['name']) or bool(binding and part['textures'][binding['texture']] in forest_textures-mountain_textures)
        g=GLB(self.assets/definition['collision']);collision=[]
        for node in sorted(g.doc['nodes'],key=lambda n:n.get('extras',{}).get('order',0)):
            if 'mesh' not in node:continue
            kind=node['extras']['surface']
            for prim in g.doc['meshes'][node['mesh']]['primitives']:
                p=g.array(prim['attributes']['POSITION']).astype(float)
                p=np.stack([p[:,0]+col*TILE+TILE/2,p[:,2]+row*TILE+TILE/2,p[:,1]],axis=-1)
                faces=p[g.array(prim['indices']).reshape(-1,3).astype(int)]
                # Locomotion accepts only upward faces (map Y runs south).
                collision.extend((kind,face) for face in faces)
        result=(parts,collision);self.tiles[key]=result;return result

    def bounds(self,world,lo,hi):
        for row in range(max(0,int(lo[1]//TILE)),min(8,int(hi[1]//TILE))+1):
            for col in range(max(0,int(lo[0]//TILE)),min(11,int(hi[0]//TILE))+1):
                yield (col,row),self.tile(world,col,row)


class Region:
    def __init__(self,spec,native,library):
        self.spec=spec;self.library=library
        self.world='sylvarant' if spec.anchor<256 else 'tethe-alla'
        self.anchor=source_origin(spec,native)
        self.target=np.array(spec.target)*[768,576]
        self.offset=self.target-self.anchor
        self.local=np.array(spec.outline,float)
        if area(self.local)<0:self.local=self.local[::-1]
        edges=np.roll(self.local,-1,axis=0)-self.local
        if (cross2(edges,np.roll(edges,-1,axis=0))< -1e-5).any():raise ValueError(f'non-convex district: {spec.name}')
        self.polygon=self.local+self.target
        self.source_polygon=self.local+self.anchor
        self.inner_polygon=inset_polygon(self.polygon,240)
        self.forest_polygon=outset_polygon(self.source_polygon,900)
        self.lo=self.polygon.min(axis=0);self.hi=self.polygon.max(axis=0)
        self.source_lo=self.source_polygon.min(axis=0);self.source_hi=self.source_polygon.max(axis=0)
        self.native_tiles=list(library.bounds(self.world,self.source_lo-FEATHER-160,self.source_hi+FEATHER+160))
        self.faces=[]
        for _,(_,faces) in self.native_tiles:
            self.faces.extend(faces)
        self.sampler=SurfaceSampler(self.faces)
        # Display cliffs can differ substantially from their simplified walking
        # collision. Join the actual painted surface, with every native edge
        # breakpoint retained, rather than fitting a coarse collision raster.
        visible=[]
        for _,(parts,_) in self.native_tiles:
            for part,primitives in parts:
                for primitive in primitives:
                    if not is_ground(primitive):continue
                    mountain=is_mountain(primitive['name'])
                    if (part.get('resource')==2 or primitive['material'].get('blend')) and not mountain:continue
                    triangles=primitive['data'][:,:,:3]
                    center=triangles[:,:,:2].mean(1)
                    radius=np.linalg.norm(triangles[:,:,:2]-center[:,None,:],axis=-1).max(1)
                    near=np.abs(distance(center,self.source_polygon))<=radius+1
                    visible.extend(triangles[near])
        self.visible_faces=np.asarray(visible).reshape(-1,3,3)
        self.visible_sampler=SurfaceSampler([('grassland',face) for face in visible])
        self.profiles=[]
        for a,b in zip(self.source_polygon,np.roll(self.source_polygon,-1,axis=0)):
            edge=b-a
            q=self.visible_faces[:,:,:2].reshape(-1,2)
            s=(np.roll(self.visible_faces[:,:,:2],-1,axis=1)-self.visible_faces[:,:,:2]).reshape(-1,2)
            den=cross2(edge,s);nonparallel=np.abs(den)>1e-8
            t=np.divide(cross2(q-a,s),den,out=np.full_like(den,-1),where=nonparallel)
            u=np.divide(cross2(q-a,edge),den,out=np.full_like(den,-1),where=nonparallel)
            cuts=t[nonparallel&(t>=0)&(t<=1)&(u>=0)&(u<=1)]
            ts=np.unique(np.round(np.r_[np.linspace(0,1,max(2,int(np.linalg.norm(edge)/80)+1)),cuts],12))
            p=a+ts[:,None]*edge
            h,k=self.sampler.sample(*p.T);visual,_=self.visible_sampler.sample(*p.T,highest=True)
            water=(k==SURFACES.index('water'))|((k==SURFACES.index('barrier'))&(np.abs(h)<.01))
            height=np.where(np.isfinite(visual)&(visual>0)&~water,visual,h)
            height=np.where(water,-18,height)
            valid=np.isfinite(height)
            if not valid.any():raise ValueError(f'{spec.name} has an uncovered scenery border')
            height=np.interp(ts,ts[valid],height[valid])
            self.profiles.append((ts,height))
        self.collisions=[]
        self.parts=defaultdict(list)
        self.aprons=defaultdict(Mesh)
        self.forest_fringe=defaultdict(Mesh)
        self.coast_segments=[]
        self.visual_triangles=0

    def export_forest_edges(self,output,write_json,field,regions):
        """Continue connected native crowns and trunks across a district cut."""
        slug=self.spec.name.lower().replace(' ','-').replace("'",'')
        emitted=[];owners=[];definitions={}
        from surfaces import clip_level
        for native_tile,(parts,_) in self.native_tiles:
            for part_i,(definition,primitives) in enumerate(parts):
                definitions[native_tile,part_i]=(definition,primitives)
                for primitive_i,primitive in enumerate(primitives):
                    if not primitive['forest']:continue
                    for face in primitive['data']:
                        if (face[:,:2].max(0)<self.source_lo-900).any() or (face[:,:2].min(0)>self.source_hi+900).any():continue
                        if contains(face[:,:2],self.source_polygon).all():continue
                        poly=clip_polygon(face,self.forest_polygon)
                        if not valid_polygon(poly):continue
                        bands=[];inner=self.source_polygon
                        for width in [160,480,760,900]:
                            outer=outset_polygon(self.source_polygon,width)
                            clipped=clip_polygon(poly,outer)
                            if valid_polygon(clipped):bands.extend(subtract_polygon(clipped,inner))
                            inner=outer
                        for extension in bands:
                            extension=extension.copy();source=extension[:,:2].copy();extension[:,:2]+=self.offset
                            d=np.maximum(0,distance(extension[:,:2],self.polygon))
                            ground=field.sample(field.display_height,*extension[:,:2].T)
                            old,_=self.sampler.sample(*source.T);old=np.where(np.isfinite(old),old,ground)
                            extension[:,2]+=(ground-old)*smooth(0,160,d)
                            edge_width=650+120*np.sin(extension[:,0]/630)+80*np.sin(extension[:,1]/470)
                            extension=clip_level(np.column_stack([extension,edge_width-d]),11,0)
                            if len(extension)<3:continue
                            if field.sample(field.water.astype(float),*extension[:,:2].mean(0))>.5:continue
                            pieces=[extension]
                            for other in regions:
                                if other is self:continue
                                if (extension[:,:2].max(0)<other.lo).any() or (extension[:,:2].min(0)>other.hi).any():continue
                                pieces=[p for piece in pieces for p in subtract_polygon(piece,other.polygon)]
                            for piece in pieces:
                                triangles=fan(piece);emitted.extend(triangles)
                                owners.extend([(native_tile,part_i,primitive_i)]*len(triangles))
        if not emitted:return
        emitted=np.asarray(emitted);keep=rooted_forest_faces(emitted,self.polygon)
        groups=defaultdict(list)
        for face,owner in zip(emitted[keep],[owner for owner,take in zip(owners,keep) if take]):
            for tile,piece in split_tiles(face[:,:11]):groups[owner[:2],tile,owner[2]].extend(fan(piece))
            if cross2(face[1,:2]-face[0,:2],face[2,:2]-face[0,:2])>=-1e-7:continue
            for pa,pb in zip(face,np.roll(face,-1,axis=0)):
                if abs(pa[11])>1e-6 or abs(pb[11])>1e-6:continue
                xy=np.array([pa[:2],pb[:2],pb[:2],pa[:2]])
                bottom=field.sample(field.display_height,*xy.T)
                z=np.array([pa[2],pb[2],bottom[2],bottom[3]])
                if max(z[:2]-bottom[:2])<10:continue
                uv=np.column_stack([xy@np.array([.6,.8])/240,[0,0,1,1]])
                wall=np.column_stack([xy,z,uv,np.ones((4,4))])
                for (col,row),piece in split_tiles(wall):
                    data=fan(piece).reshape(-1,9);p=data[:,:3].copy()
                    p[:,0]-=col*TILE+TILE/2;p[:,1]=row*TILE+TILE/2-p[:,1]
                    self.forest_fringe[col,row].triangles(p,data[:,3:5],data[:,5:9])
        for owner,(definition,primitives) in definitions.items():
            native_tile,part_i=owner
            for tile in sorted({tile for key,tile,_ in groups if key==owner}):
                col,row=tile;meshes=[];materials=[]
                for i,primitive in enumerate(primitives):
                    data=groups.get((owner,tile,i))
                    if not data:continue
                    data=np.asarray(data).reshape(-1,11);p=data[:,:3].copy()
                    p[:,0]-=col*TILE+TILE/2;p[:,1]=row*TILE+TILE/2-p[:,1]
                    mesh=Mesh();mesh.triangles(p,data[:,3:5],data[:,5:9],data[:,9:11])
                    material=deepcopy(primitive['material']);material.update(meshes=[len(meshes)],draw_order=[primitive['draw_order']])
                    meshes.append((primitive['name'],mesh));materials.append(material)
                path=f'world/united/districts/{slug}/forest-edge-{col:02}-{row:02}-source-{native_tile[0]:02}-{native_tile[1]:02}-{part_i}'
                write_glb(output/(path+'.glb'),meshes)
                part=deepcopy(definition);part.update(mesh=path+'.glb',materials=materials)
                self.parts[tile].append(part)

    def sample(self,x,y):
        return self.sampler.sample(np.asarray(x)-self.offset[0],np.asarray(y)-self.offset[1])

    def border(self,points):
        """Closest boundary point and its exact piecewise display elevation."""
        best=np.full(points.shape[:-1],np.inf);height=np.zeros_like(best);nearest=np.zeros_like(points)
        for (a,b),(ts,z) in zip(zip(self.polygon,np.roll(self.polygon,-1,axis=0)),self.profiles):
            edge=b-a;t=np.clip(((points-a)*edge).sum(-1)/(edge@edge),0,1)
            p=a+t[...,None]*edge;d=np.linalg.norm(points-p,axis=-1);take=d<best
            height=np.where(take,np.interp(t,ts,z),height);nearest=np.where(take[...,None],p,nearest);best=np.minimum(best,d)
        return nearest,height

    def stitch(self,polygon):
        """Split a cut mesh edge at the native scenery's own height knots."""
        directions=np.roll(self.polygon,-1,axis=0)-self.polygon
        delta=polygon[:,None,:2]-self.polygon[None,:,:]
        collinear=np.abs(cross2(directions[None,:,:],delta))<.02*np.linalg.norm(directions,axis=-1)
        if not collinear.any():return polygon
        p=polygon.copy();profiles=[]
        for (a,b),(ts,z) in zip(zip(self.polygon,np.roll(self.polygon,-1,axis=0)),self.profiles):
            edge=b-a;length=np.linalg.norm(edge)
            t=((p[:,:2]-a)*edge).sum(-1)/(edge@edge)
            collinear=np.abs(cross2(edge,p[:,:2]-a))<.02*length
            on=collinear&(t>=-1e-8)&(t<=1+1e-8)
            p[on,2]=np.interp(t[on],ts,z)
            # Polygon subtraction can leave an edge extending beyond a district
            # corner. Its overlapping interval still needs every native knot,
            # including the corner itself, even if one endpoint lies past it.
            if np.count_nonzero(collinear)>1:profiles.append((ts,z,t,collinear))
        if not profiles:return p
        result=[]
        for i in range(len(p)):
            j=(i+1)%len(p);result.append(p[i])
            for ts,z,t,on in profiles:
                if not (on[i] and on[j]) or abs(t[j]-t[i])<1e-12:continue
                cuts=ts[(ts>min(t[i],t[j])+1e-10)&(ts<max(t[i],t[j])-1e-10)]
                if t[j]<t[i]:cuts=cuts[::-1]
                for cut in cuts:
                    v=p[i]+(p[j]-p[i])*((cut-t[i])/(t[j]-t[i]));v[2]=np.interp(cut,ts,z);result.append(v)
                break
        return np.asarray(result)

    def export(self,output,write_json,field=None):
        """Clip and split display/collision with the SAME rigid map transform."""
        owners=[]
        slug=self.spec.name.lower().replace(' ','-').replace("'",'')
        for native_tile,(parts,collisions) in self.native_tiles:
            for kind,face in collisions:
                if (face[:,:2].max(0)<self.source_lo).any() or (face[:,:2].min(0)>self.source_hi).any():continue
                poly=clip_polygon(face,self.source_polygon)
                if len(poly)<3:continue
                poly=poly.copy();poly[:,:2]+=self.offset
                for tile,piece in split_tiles(poly):
                    self.collisions.extend((tile,kind,t) for t in fan(piece))
            for part_i,(definition,primitives) in enumerate(parts):
                groups=defaultdict(list)
                for primitive_i,primitive in enumerate(primitives):
                    if is_ocean(primitive):continue
                    if 'wave' in primitive['name'].lower() and definition.get('resource')==2:continue
                    faces=primitive['data']
                    select=(faces[:,:,:2].max(1)>=self.source_lo).all(1)&(faces[:,:,:2].min(1)<=self.source_hi).all(1)
                    for face in faces[select]:
                        poly=clip_polygon(face,self.source_polygon)
                        if not valid_polygon(poly):continue
                        poly=poly.copy();poly[:,:2]+=self.offset
                        material=primitive['material']
                        if not material.get('blend') and not primitive['forest']:
                            from surfaces import sea_crossing
                            for face_part in fan(poly):
                                coast=face_part[:,:3].copy();coast[:,2]-=.5
                                crossing=sea_crossing(coast)
                                if crossing is not None:
                                    a,b,normal=crossing
                                    if field is not None:
                                        middle=(a+b)/2
                                        plus=field.sample(field.water.astype(float),*(middle+normal*60))
                                        minus=field.sample(field.water.astype(float),*(middle-normal*60))
                                        if plus<minus:normal=-normal
                                    self.coast_segments.append((a,b,normal))
                        if definition.get('resource')==2 and material.get('multiply') and material.get('multiply')==material.get('color'):
                            poly[:,3:5]=poly[:,:2]/800
                            poly[:,9:11]=np.stack([poly[:,1],-poly[:,0]],axis=-1)/800
                            poly[:,5:9]=[240/255,240/255,240/255,128/255]
                        for tile,piece in split_tiles(poly):
                            groups[(tile,primitive_i)].extend(fan(piece))
                            if not is_ground(primitive) or definition.get('resource')==2 or material.get('blend') or piece[:,2].max()<3:continue
                            if cross2(face[1,:2]-face[0,:2],face[2,:2]-face[0,:2])>=-1e-7:continue
                            if (distance(piece[:,:2],self.polygon)<-240).all():continue
                            for ring in subtract_polygon(piece,self.inner_polygon):
                                if not valid_polygon(ring):continue
                                data=fan(ring).reshape(-1,11);col,row=tile
                                position=data[:,:3].copy();position[:,0]-=col*TILE+TILE/2
                                position[:,1]=row*TILE+TILE/2-position[:,1];position[:,2]+=.03
                                uv=(data[:,:2]-[col*TILE,row*TILE])/TILE
                                uv=(uv*(TEX-2)+1)/TEX
                                color=np.ones((len(data),4));color[:,3]=smooth(-240,0,distance(data[:,:2],self.polygon))
                                self.aprons[tile].triangles(position,uv,color)
                tiles=sorted({tile for tile,_ in groups})
                for tile in tiles:
                    col,row=tile;meshes=[];materials=[]
                    for primitive_i,primitive in enumerate(primitives):
                        data=groups.get((tile,primitive_i))
                        if not data:continue
                        data=np.asarray(data).reshape(-1,11)
                        local=data[:,:3].copy()
                        local[:,0]-=col*TILE+TILE/2
                        local[:,1]=row*TILE+TILE/2-local[:,1]
                        mesh=Mesh();mesh.triangles(local,data[:,3:5],data[:,5:9],data[:,9:11])
                        index=len(meshes);meshes.append((primitive['name'],mesh))
                        material=deepcopy(primitive['material']);material['meshes']=[index];material['draw_order']=[primitive['draw_order']]
                        materials.append(material)
                        self.visual_triangles+=len(data)//3
                    owner=f'world/united/districts/{slug}/column-{col:02}-row-{row:02}-source-{native_tile[0]:02}-{native_tile[1]:02}-part-{part_i}'
                    write_glb(output/(owner+'.glb'),meshes)
                    part=deepcopy(definition);part['mesh']=owner+'.glb';part['materials']=materials
                    for material in materials:
                        if part.get('resource')==2 and material.get('multiply')==material.get('color') and material.get('multiply'):
                            texture=material['color']['texture']
                            part['textures'][texture]='world/tethe-alla/terrain/column-03-row-04/part-1-texture-1.ktx2'
                    self.parts[tile].append(part);owners.append(owner+'.glb')
        return owners

    def bake_map(self,output,resolution=32):
        """Orthographic native paint for transition matching and atlas review."""
        lo=np.floor((self.source_lo-FEATHER-64)/resolution)*resolution
        hi=np.ceil((self.source_hi+FEATHER+64)/resolution)*resolution
        width,height=(np.ceil((hi-lo)/resolution).astype(int)+1)
        xx,yy=np.meshgrid(np.arange(width)*resolution+lo[0],np.arange(height)*resolution+lo[1])
        # Sample the native floor on a coarser grid; this only separates canopy
        # paint from ground paint. Geometry and collision never use this raster.
        coarse_x,coarse_y=np.meshgrid(np.linspace(lo[0],hi[0],max(2,width//3)),np.linspace(lo[1],hi[1],max(2,height//3)))
        floor,_=self.sample(coarse_x+self.offset[0],coarse_y+self.offset[1])
        visible,_=self.visible_sampler.sample(coarse_x,coarse_y,highest=True)
        floor=np.where(np.isfinite(visible),np.fmax(floor,visible),floor)
        coordinates=np.meshgrid(np.linspace(0,floor.shape[1]-1,width),np.linspace(0,floor.shape[0]-1,height))
        floor=ndi.map_coordinates(np.nan_to_num(floor,nan=-10000),[coordinates[1],coordinates[0]],order=1,mode='nearest')
        full=np.zeros((height,width,4),np.float32);ground=np.zeros_like(full)
        depth=np.full((height,width),-1e9);ground_depth=depth.copy()
        ordered=[]
        for _,(parts,_) in self.native_tiles:
            for part,primitives in parts:
                for primitive in primitives:
                    ordered.append((part,primitive))
        ordered.sort(key=lambda item:bool(item[1]['material'].get('blend')))
        for part,primitive in ordered:
            material=primitive['material']
            if 'color' not in material:continue
            color_ref=material['color'];texture=self.library.texture(part['textures'][color_ref['texture']])
            th,tw=texture.shape[:2]
            multiply=material.get('multiply');multiply_texture=None
            if multiply:multiply_texture=self.library.texture(part['textures'][multiply['texture']])
            faces=primitive['data'];select=(faces[:,:,:2].max(1)>=lo).all(1)&(faces[:,:,:2].min(1)<=hi).all(1)
            for triangle in faces[select]:
                a,b,c=triangle;den=cross2(b[:2]-a[:2],c[:2]-a[:2])
                if abs(den)<1e-7:continue
                cull=material.get('cull','back')
                if (cull=='back' and den>0) or (cull=='front' and den<0):continue
                lower=np.maximum(0,np.floor((triangle[:,:2].min(0)-lo)/resolution).astype(int))
                upper=np.minimum([width-1,height-1],np.ceil((triangle[:,:2].max(0)-lo)/resolution).astype(int))
                if (upper<lower).any():continue
                w=np.s_[lower[1]:upper[1]+1,lower[0]:upper[0]+1]
                px=xx[w]-a[0];py=yy[w]-a[1]
                u=(px*(c[1]-a[1])-py*(c[0]-a[0]))/den
                v=((b[0]-a[0])*py-(b[1]-a[1])*px)/den
                inside=(u>=-1e-5)&(v>=-1e-5)&(u+v<=1.00001)
                if not inside.any():continue
                uv=a[3:5]+u[...,None]*(b[3:5]-a[3:5])+v[...,None]*(c[3:5]-a[3:5])
                tint=a[5:9]+u[...,None]*(b[5:9]-a[5:9])+v[...,None]*(c[5:9]-a[5:9])
                rgba=texture_sample(texture,uv,color_ref)
                if material.get('vertex_color',True):rgba*=tint
                if multiply_texture is not None:
                    mh,mw=multiply_texture.shape[:2]
                    uv_b=a[9:11]+u[...,None]*(b[9:11]-a[9:11])+v[...,None]*(c[9:11]-a[9:11])
                    rgba[:,:,:3]*=texture_sample(multiply_texture,uv_b,multiply)[:,:,:3]
                z=a[2]+u*(b[2]-a[2])+v*(c[2]-a[2])
                opaque=inside&(rgba[...,3]>=1/255)
                if not material.get('blend'):rgba[...,3]=np.where(opaque,1,0)
                show=opaque&(z>=depth[w]-.01)
                target=full[w];target[show]=rgba[show];full[w]=target
                zbuffer=depth[w];zbuffer[show]=z[show];depth[w]=zbuffer
                terrain_height=(z<=floor[w]+24)|is_mountain(primitive['name'])
                show=opaque&terrain_height&(z>=ground_depth[w]-.01)&is_ground(primitive)&((part.get('resource')!=2) or is_mountain(primitive['name']))
                target=ground[w];target[show]=rgba[show];ground[w]=target
                zbuffer=ground_depth[w];zbuffer[show]=z[show];ground_depth[w]=zbuffer
        self.map_lo=lo+self.offset;self.map_step=resolution
        self.map_full=np.clip(full,0,1);self.map_floor=np.clip(ground,0,1);self.map_depth=depth;self.map_floor_depth=ground_depth
        alpha=ndi.gaussian_filter(self.map_floor[:,:,3],2)
        self.map_ground_tone=np.empty_like(self.map_floor)
        for channel in range(3):
            self.map_ground_tone[:,:,channel]=ndi.gaussian_filter(self.map_floor[:,:,channel]*self.map_floor[:,:,3],2)/np.maximum(alpha,1e-6)
        self.map_ground_tone[:,:,3]=alpha
        from PIL import Image
        owner=output/'review'/(self.spec.name.lower().replace(' ','-')+'.png')
        owner.parent.mkdir(parents=True,exist_ok=True)
        Image.fromarray((self.map_full*255).astype(np.uint8)).save(owner)

    def paint(self,x,y,floor=False):
        source=self.map_floor if floor else self.map_full
        x,y=np.broadcast_arrays(x,y)
        coords=[(y-self.map_lo[1])/self.map_step,(x-self.map_lo[0])/self.map_step]
        return np.stack([ndi.map_coordinates(source[...,i],coords,order=1,mode='nearest') for i in range(4)],axis=-1)



class Composition:
    def __init__(self,assets,original):
        self.library=NativeLibrary(assets)
        self.native={e['id']:e for world in original.values() for e in world}
        self.regions=[Region(spec,self.native,self.library) for spec in DISTRICTS]
        self.parts=defaultdict(list);self.collision=defaultdict(lambda:defaultdict(list));self.collision_ordered=defaultdict(list)
        self.aprons=defaultdict(list)
        self.forest_fringe=defaultdict(list)
        self.coast_segments=[]
        self.check_layout()

    def check_layout(self):
        for i,a in enumerate(self.regions):
            for b in self.regions[i+1:]:
                if (a.hi<=b.lo).any() or (b.hi<=a.lo).any():continue
                overlap=clip_polygon(a.polygon,b.polygon)
                if len(overlap)>=3 and abs(area(overlap))>1:
                    raise ValueError(f'districts overlap: {a.spec.name} / {b.spec.name} ({abs(area(overlap)):.0f} units²)')

    def nearby(self,lo,hi):
        return [r for r in self.regions if (r.hi>=lo).all() and (r.lo<=hi).all()]

    def outside(self,triangle,candidates=None):
        result=[triangle]
        for region in (self.nearby(triangle[:,:2].min(0),triangle[:,:2].max(0)) if candidates is None else candidates):
            if (triangle[:,:2].max(0)<region.lo).any() or (triangle[:,:2].min(0)>region.hi).any():continue
            next_pieces=[]
            for poly in result:
                if contains(poly[:,:2],region.polygon).all():continue
                next_pieces.extend(subtract_polygon(poly,region.polygon))
            result=next_pieces
        return result

    def conform(self,field):
        self.core=np.zeros(field.shape,bool)
        self.native_height=np.full(field.shape,np.nan,np.float32)
        self.native_kind=np.full(field.shape,-1,np.int16)
        base_weight=1/FEATHER**2
        total=np.full(field.shape,base_weight,np.float64)
        weighted_height=np.zeros(field.shape,np.float64)
        weighted_sdf=field.sdf.astype(np.float64)*base_weight
        core_water=np.zeros(field.shape,bool)
        for region in self.regions:
            lo=np.maximum(0,np.floor((region.lo-FEATHER)/field.step).astype(int))
            hi=np.minimum(np.array(field.shape[::-1])-1,np.ceil((region.hi+FEATHER)/field.step).astype(int))
            window=np.s_[lo[1]:hi[1]+1,lo[0]:hi[0]+1]
            x=field.x[window];y=field.y[window]
            d=distance(np.stack([x,y],axis=-1),region.polygon)
            h,k=region.sample(x,y)
            valid=np.isfinite(h)&(d<FEATHER)
            native_land=(k!=SURFACES.index('water'))&~((k==SURFACES.index('barrier'))&(np.abs(h)<.01))
            signed=(ndi.distance_transform_edt(native_land)-ndi.distance_transform_edt(~native_land))*field.step
            signed=np.clip(signed,-FEATHER*2,FEATHER*2)
            weight=(1-smooth(0,FEATHER,np.maximum(0,d)))/np.maximum(1,d)**2
            weight=np.where(valid,weight,0)
            target=np.where(native_land,h,-18)
            # Retain the exact core. The outer ring carries terrain form, not
            # isolated collision details such as a fence or roof.
            target=ndi.median_filter(np.nan_to_num(target,nan=0),size=3)
            ring=(d>0)&(d<480)&native_land&valid
            if ring.any():
                border,display=region.border(np.stack([x[ring],y[ring]],axis=-1))
                support,_=region.sample(*border.T)
                correction=np.where(np.isfinite(support)&(display>0),display-support,0)
                target[ring]+=correction*(1-smooth(0,480,d[ring]))
            weighted_height[window]+=target*weight
            weighted_sdf[window]+=signed*weight;total[window]+=weight
            core=(d<=0)&valid
            self.core[window]|=core
            v=self.native_height[window];v[core]=h[core];self.native_height[window]=v
            v=self.native_kind[window];v[core]=k[core];self.native_kind[window]=v
            core_water[window]=np.where(core,~native_land,core_water[window])
            field.clear[window]=np.maximum(field.clear[window],1-smooth(0,FEATHER,np.maximum(0,d)))
        field.sdf=(weighted_sdf/total).astype(np.float32)
        field.water=np.where(self.core,core_water,field.sdf<0)
        base=np.maximum(28,field.height)*smooth(0,400,field.sdf)
        field.height=((weighted_height+base*base_weight)/total).astype(np.float32)
        field.height=np.where(field.water,-18,field.height)
        field.height=np.where(self.core,self.native_height,field.height)
        print(f'Assembled {len(self.regions)} complete native districts with rigid foundations.',flush=True)

    def seam_height(self,points,field,candidates=None):
        result=field.sample(field.height,points[:,0],points[:,1])
        for region in (self.nearby(points[:,:2].min(0)-1,points[:,:2].max(0)+1) if candidates is None else candidates):
            near=np.abs(distance(points[:,:2],region.polygon))<.02
            if near.any():
                h,k=region.sample(points[near,0],points[near,1]);h=np.where((k==SURFACES.index('water'))|((k==SURFACES.index('barrier'))&(np.abs(h)<.01)),-18,h);v=result[near];v[np.isfinite(h)]=h[np.isfinite(h)];result[near]=v
        return result

    def stitch(self,polygon,candidates):
        for region in candidates:
            if (polygon[:,:2].max(0)<region.lo-.02).any() or (polygon[:,:2].min(0)>region.hi+.02).any():continue
            polygon=region.stitch(polygon)
        return polygon

    def display_height(self,field):
        # The renderer interpolates across cells straddling a cut. Carry the
        # display datum a short distance inside that sampling grid too, while
        # leaving native collision and the native scenery itself untouched.
        result=field.height.copy()
        for region in self.regions:
            box=(field.x>=region.lo[0]-1)&(field.x<=region.hi[0]+1)&(field.y>=region.lo[1]-1)&(field.y<=region.hi[1]+1)
            p=np.stack([field.x[box],field.y[box]],axis=-1);d=distance(p,region.polygon)
            inside=(d<=0)&(d>-320)
            if not inside.any():continue
            border,display=region.border(p[inside]);support,_=region.sample(*border.T)
            correction=np.where(np.isfinite(support)&(display>0),display-support,0)
            values=result[box];values[inside]+=correction*(1-smooth(0,320,-d[inside]));result[box]=values
        return result

    def paint(self,x,y,base,map_view=False,wet=None):
        result=base.astype(float)/255
        points=np.stack([x,y],axis=-1)
        for region in self.regions:
            lo=region.lo-FEATHER;hi=region.hi+FEATHER
            box=(x>=lo[0])&(x<=hi[0])&(y>=lo[1])&(y<=hi[1])
            if not box.any():continue
            d=distance(points[box],region.polygon)
            colors=region.paint(x[box],y[box],floor=not map_view)
            if not map_view:
                coords=[(y[box]-region.map_lo[1])/region.map_step,(x[box]-region.map_lo[0])/region.map_step]
                tone=np.stack([ndi.map_coordinates(region.map_ground_tone[:,:,i],coords,order=1,mode='nearest') for i in range(4)],axis=-1)
                blend=smooth(0,240,np.maximum(0,d))
                colors=colors*(1-blend[:,None])+tone*blend[:,None]
            weight=(1-smooth(0,FEATHER,np.maximum(0,d)))*colors[:,3]
            if wet is not None:
                source_depth=region.map_depth if map_view else region.map_floor_depth
                z=ndi.map_coordinates(source_depth,[(y[box]-region.map_lo[1])/region.map_step,(x[box]-region.map_lo[0])/region.map_step],order=1,mode='nearest')
                weight=np.where((wet[box]&((z<10)|~np.bool_(map_view)))|(~wet[box]&(z<3)),0,weight)
                if not map_view:
                    actual=self.current_field.sample(self.current_field.display_height,x[box],y[box])
                    weight*=np.where(d>0,1-smooth(24,100,np.abs(actual-z)),1)
            if not map_view:weight=np.where(d>=-241,weight,0)
            view=result[box];view[:,:3]=view[:,:3]*(1-weight[:,None])+colors[:,:3]*weight[:,None];result[box]=view
        return np.clip(result*255,0,255).astype(np.uint8)

    def export(self,output,write_json,field):
        self.current_field=field
        report=[]
        for i,region in enumerate(self.regions):
            files=region.export(output,write_json,field)
            region.export_forest_edges(output,write_json,field,self.regions)
            region.bake_map(output)
            for tile,parts in region.parts.items():self.parts[tile].extend(parts)
            for tile,mesh in region.aprons.items():
                if mesh.indices:self.aprons[tile].append((region.spec.name,mesh))
            for tile,mesh in region.forest_fringe.items():
                if mesh.indices:self.forest_fringe[tile].append((region.spec.name,mesh))
            self.coast_segments.extend(region.coast_segments)
            for tile,kind,face in region.collisions:
                self.collision[tile][kind].extend(face.tolist())
                groups=self.collision_ordered[tile]
                if groups and groups[-1][0]==kind:groups[-1][1].extend(face.tolist())
                else:groups.append((kind,face.tolist()))
            report.append({'name':region.spec.name,'anchor':region.spec.anchor,'members':region.spec.members,
                           'target':region.target.tolist(),'polygon':region.polygon.tolist(),
                           'native_anchor':region.anchor.tolist(),'translation':region.offset.tolist(),
                           'height_offset':0,'intent':region.spec.intent,'display_triangles':region.visual_triangles,'files':files})
            print(f'  district {i+1}/{len(self.regions)}: {region.spec.name}',flush=True)
        for tile,parts in self.parts.items():
            order=64
            for part in parts:
                # Native mountain shells and forest fringes have authored passes.
                # Moving every blended draw behind opaque draws (or using mesh
                # index) lets a translucent front shell occlude its own backing.
                for material in sorted(part['materials'],key=lambda m:m['draw_order'][0]):
                    material['draw_order']=[order];order+=1
                if order>=2000:raise ValueError(f'tile {tile} exceeds native material budget')
        write_json(output,'world/united/composition.json',{'districts':report,'native_foundation_tolerance':.02})
        return report
