"""Native scene access and source-floor probes for small movable props.

Town assembly lives in native_regions.py. Buildings and their landscape use a
single transform; this module deliberately does not extract or raise towns.
"""
from pathlib import Path
import json
import struct
import numpy as np


class GLB:
    def __init__(self,path):
        blob=Path(path).read_bytes();n=struct.unpack_from('<I',blob,12)[0]
        self.doc=json.loads(blob[20:20+n]);self.binary=blob[28+n:]
        self.parents={child:i for i,node in enumerate(self.doc['nodes']) for child in node.get('children',[])}
    def array(self,i):
        a=self.doc['accessors'][i];v=self.doc['bufferViews'][a['bufferView']]
        size={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4,'MAT4':16}[a['type']]
        dtype={5126:'<f4',5125:'<u4',5123:'<u2',5121:'u1'}[a['componentType']]
        offset=v.get('byteOffset',0)+a.get('byteOffset',0)
        stride=v.get('byteStride',np.dtype(dtype).itemsize*size)
        result=np.ndarray((a['count'],size),dtype=dtype,buffer=self.binary,offset=offset,strides=(stride,np.dtype(dtype).itemsize)).copy()
        if a.get('normalized'):result=result.astype(float)/np.iinfo(dtype).max
        return result
    def transform(self,i):
        node=self.doc['nodes'][i]
        if 'matrix' in node:local=np.array(node['matrix']).reshape(4,4).T
        else:
            x,y,z,w=node.get('rotation',[0,0,0,1]);sx,sy,sz=node.get('scale',[1,1,1])
            local=np.eye(4)
            local[:3,:3]=np.array([[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w)],[2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w)],[2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y)]])@np.diag([sx,sy,sz])
            local[:3,3]=node.get('translation',[0,0,0])
        return self.transform(self.parents[i])@local if i in self.parents else local


def sample_ground(assets,world,x,y):
    x,y=np.broadcast_arrays(x,y);result=np.full(x.shape,np.nan)
    # Point-in-triangle queries preserve the native stairways and terraces.
    for col in range(max(0,int(x.min()//6400)),min(11,int(x.max()//6400))+1):
        for row in range(max(0,int(y.min()//6400)),min(8,int(y.max()//6400))+1):
            path=assets/f'world/{world}/terrain/column-{col:02}-row-{row:02}/collision.glb'
            g=GLB(path)
            for node in g.doc['nodes']:
                if 'mesh' not in node or node.get('extras',{}).get('surface') in ['water','barrier']:continue
                for prim in g.doc['meshes'][node['mesh']]['primitives']:
                    vertices=g.array(prim['attributes']['POSITION']).astype(float)
                    vertices[:,0]+=col*6400+3200;vertices[:,2]+=row*6400+3200
                    for triangle in vertices[g.array(prim['indices']).reshape(-1,3)]:
                        a,b,c=triangle
                        den=(b[2]-c[2])*(a[0]-c[0])+(c[0]-b[0])*(a[2]-c[2])
                        if abs(den)<1e-8:continue
                        bounds=(x>=triangle[:,0].min())&(x<=triangle[:,0].max())&(y>=triangle[:,2].min())&(y<=triangle[:,2].max())
                        if not bounds.any():continue
                        px=x[bounds];py=y[bounds]
                        u=((b[2]-c[2])*(px-c[0])+(c[0]-b[0])*(py-c[2]))/den
                        v=((c[2]-a[2])*(px-c[0])+(a[0]-c[0])*(py-c[2]))/den
                        inside=(u>=-1e-5)&(v>=-1e-5)&(u+v<=1.00001)
                        values=result[bounds];h=u*a[1]+v*b[1]+(1-u-v)*c[1]
                        values[inside]=np.fmax(values[inside],h[inside]);result[bounds]=values
    return result
