"""Clipped foliage and coast ribbons; no whole-atlas UVs or centroid holes."""
from collections import defaultdict
import numpy as np


def clip_level(polygon,column,level):
    result=[]
    for i,a in enumerate(polygon):
        b=polygon[(i+1)%len(polygon)];inside=a[column]>=level;following=b[column]>=level
        if inside:result.append(a)
        if inside!=following:result.append(a+(b-a)*((level-a[column])/(b[column]-a[column])))
    return np.asarray(result).reshape(-1,polygon.shape[1])


def sea_crossing(face):
    points=[]
    for a,b in zip(face,np.roll(face,-1,axis=0)):
        if a[2]==0:points.append(a[:2])
        if (a[2]<0)!=(b[2]<0):points.append(a[:2]+(b[:2]-a[:2])*(-a[2]/(b[2]-a[2])))
    if len(points)<2:return None
    points=np.unique(np.round(points,7),axis=0)
    if len(points)<2:return None
    a,b=max(((a,b) for a in points for b in points),key=lambda pair:np.linalg.norm(pair[1]-pair[0]))
    if np.linalg.norm(b-a)<.01:return None
    wet=face[face[:,2]<0,:2]
    if not len(wet):return None
    edge=b-a;normal=np.array([-edge[1],edge[0]])/np.linalg.norm(edge)
    if normal@(wet.mean(0)-(a+b)/2)<0:normal=-normal
    return a,b,normal


def shade_canopies(meshes):
    """Bake rounded crown shading, sharing normals across terrain tile cuts."""
    normals={};keys_by_tile={}
    for tile,mesh in meshes.items():
        if not mesh.indices:continue
        p=np.asarray(mesh.p);world=p.copy()
        world[:,0]+=tile[0]*6400+3200;world[:,1]=tile[1]*6400+3200-world[:,1]
        keys=[tuple(v) for v in np.round(world,3)];keys_by_tile[tile]=keys
        for indices in np.asarray(mesh.indices).reshape(-1,3):
            face=p[indices];normal=np.cross(face[1]-face[0],face[2]-face[0])
            if normal[2]<0:normal=-normal
            for index in indices:
                key=keys[index]
                if key not in normals:normals[key]=np.zeros(3)
                normals[key]+=normal
    for tile,keys in keys_by_tile.items():
        n=np.asarray([normals[key] for key in keys]);n/=np.maximum(1e-12,np.linalg.norm(n,axis=1))[:,None]
        light=np.clip(.4+.6*n[:,2]**4+.12*(-n[:,0]+n[:,1]),.45,1)
        edge=(1-light)/.55
        tint=1-edge[:,None]*np.array([.55,.47,.58])
        meshes[tile].c=np.column_stack([tint,np.ones(len(tint))]).tolist()


def forest_meshes(field,composition):
    from landscape import Mesh,smooth
    from native_regions import fan,split_tiles,area
    result=defaultdict(lambda:[Mesh(),Mesh()])
    def append(mesh_index,data):
        for polygon in composition.outside(data):
            for (col,row),piece in split_tiles(polygon):
                face=fan(piece).reshape(-1,9);local=face[:,:3].copy()
                local[:,0]-=col*6400+3200;local[:,1]=row*6400+3200-local[:,1]
                result[col,row][mesh_index].triangles(local,face[:,3:5],face[:,5:9])
    for _,outline in field.forest_outlines:
        lo=np.floor(outline.min(0)/120)*120;hi=np.ceil(outline.max(0)/120)*120
        xx,yy=np.meshgrid(np.arange(lo[0],hi[0]+1,120),np.arange(lo[1],hi[1]+1,120))
        points=np.stack([xx.ravel(),yy.ravel()],axis=-1)
        density=field.sample(field.forest,*points.T)*(1-smooth(.35,.7,field.sample(field.slope,*points.T)))
        ground=field.sample(field.display_height,*points.T)
        # A low skirt rises into the crown; a constant-height cut leaves a
        # floating leaf plate on slopes. Steep cliffs do not carry reserves.
        crown=16+78*smooth(.2,.85,density)
        # Native Tethe'alla crowns repeat their foliage every ~200 world units.
        data=np.column_stack([points,ground+crown,points/200,np.ones((len(points),4)),density])
        width=xx.shape[1]
        for j in range(xx.shape[0]-1):
            for i in range(width-1):
                a=j*width+i;b=a+1;c=a+width;d=c+1
                for index in [(a,c,b),(b,c,d)]:
                    polygon=clip_level(data[list(index)],9,.2)
                    if len(polygon)<3:continue
                    append(0,polygon[:,:9])
                    for pa,pb in zip(polygon,np.roll(polygon,-1,axis=0)):
                        if abs(pa[9]-.2)>1e-7 or abs(pb[9]-.2)>1e-7:continue
                        xy=np.array([pa[:2],pb[:2],pb[:2],pa[:2]])
                        bottom=field.sample(field.display_height,*xy.T)
                        z=np.array([pa[2],pb[2],bottom[2],bottom[3]])
                        u=xy@np.array([.6,.8])/240
                        uv=np.column_stack([u,[0,0,1,1]])
                        append(1,np.column_stack([xy,z,uv,np.ones((4,4))]))
    shade_canopies({tile:meshes[0] for tile,meshes in result.items()})
    return result


def coast_meshes(segments,composition,texture_height,width=90):
    from landscape import Mesh
    from native_regions import fan,split_tiles,area
    nodes={};adjacency=defaultdict(set);normals={}
    for a,b,normal in segments:
        ka,kb=tuple(np.round(a,3)),tuple(np.round(b,3))
        if ka==kb:continue
        nodes[ka]=a;nodes[kb]=b;adjacency[ka].add(kb);adjacency[kb].add(ka)
        normals[tuple(sorted([ka,kb]))]=normal
    unused=set(normals);result=defaultdict(Mesh)
    while unused:
        first=min(unused);start=next((p for p in first if len(adjacency[p])!=2),first[0])
        chain=[start]
        while True:
            available=sorted(p for p in adjacency[chain[-1]] if tuple(sorted([p,chain[-1]])) in unused)
            if not available:break
            nxt=available[0];unused.remove(tuple(sorted([nxt,chain[-1]])));chain.append(nxt)
            if nxt==start:break
        if len(chain)<2:continue
        shore=np.array([nodes[p] for p in chain]);offset=[]
        for i,p in enumerate(chain):
            incident=[]
            if i:incident.append(normals[tuple(sorted([chain[i-1],p]))])
            if i+1<len(chain):incident.append(normals[tuple(sorted([p,chain[i+1]]))])
            if chain[0]==chain[-1] and i in [0,len(chain)-1]:
                incident=[normals[tuple(sorted([chain[-2],p]))],normals[tuple(sorted([p,chain[1]]))]]
            normal=np.sum(incident,axis=0);length=np.linalg.norm(normal)
            normal=incident[0] if length<1e-6 else normal/length
            offset.append(normal*width/max(.5,float(normal@incident[0])))
        outside=shore+np.array(offset)
        along=np.r_[0,np.linalg.norm(np.diff(shore,axis=0),axis=-1).cumsum()]/800
        if chain[0]==chain[-1]:along=along/along[-1]*max(1,round(float(along[-1])))
        v0=.5/texture_height;v1=1/8-.5/texture_height
        for i in range(len(chain)-1):
            xy=np.array([shore[i],shore[i+1],outside[i+1],outside[i]])
            uv=np.array([[along[i],v0],[along[i+1],v0],[along[i+1],v1],[along[i],v1]])
            data=np.column_stack([xy,np.full(4,1.5),uv,np.tile([1,1,1,.65],(4,1))])
            if area(xy)>0:data=data[::-1]
            # Segments already come from the visible sea-level intersection,
            # including native coastlines. Subtracting district cores here
            # removes their replacement waves after the old overlays are gone.
            for (col,row),piece in split_tiles(data):
                face=fan(piece).reshape(-1,9);local=face[:,:3].copy()
                local[:,0]-=col*6400+3200;local[:,1]=row*6400+3200-local[:,1]
                result[col,row].triangles(local,face[:,3:5],face[:,5:9])
    return result
