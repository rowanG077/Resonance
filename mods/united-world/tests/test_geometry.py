"""Regressions for authored terrain cuts and native mesh attributes."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

import numpy as np

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from native_regions import Region, SurfaceSampler, fan, cross2, texture_sample, split_tiles
from settlements import GLB


class TerrainSeams(unittest.TestCase):
    def test_cut_forest_keeps_connected_crowns_but_discards_detached_scraps(self):
        from native_regions import rooted_forest_faces
        core=np.array([[0,0],[100,0],[100,100],[0,100]],float)
        faces=np.array([[[100,30,60],[160,30,60],[100,80,60]],
                        [[160,30,60],[200,30,60],[160,80,60]],
                        [[600,30,60],[650,30,60],[600,80,60]]],float)
        self.assertEqual(rooted_forest_faces(faces,core).tolist(),[True,True,False])

    def test_reserves_do_not_drape_leaf_plates_over_steep_cliffs(self):
        from surfaces import forest_meshes
        class Field:
            forest_outlines=[('reserve',np.array([[0,0],[360,0],[360,360],[0,360]]))]
            forest=1;display_height=0;slope=.8
            def sample(self,value,x,y):return np.full(np.broadcast(x,y).shape,value)
        class Outside:
            def outside(self,polygon):return [polygon]
        field=Field()
        self.assertFalse(forest_meshes(field,Outside()))
        field.slope=0
        self.assertTrue(forest_meshes(field,Outside()))

    def test_trees_and_architecture_cannot_become_transition_ground(self):
        from native_regions import is_ground
        for name in ['forest_ask','Mesh38','ask_bokujou_house01','hol_seidou','lig_tree01','sil_top']:
            self.assertFalse(is_ground({'name':name,'forest':name in ['forest_ask','Mesh38']}),name)
        for name in ['tairiku/draw1','ground','mountain_isa_top01','ask_boku_field','izr_gake01','him_base']:
            self.assertTrue(is_ground({'name':name,'forest':False}),name)

    def test_snow_crown_does_not_box_in_eastern_grass_settlements(self):
        from landscape import snow_region
        self.assertGreater(float(snow_region(43,14)),.99) # Flanoir
        self.assertGreater(float(snow_region(49,24)),.99) # Ice temple
        self.assertEqual(float(snow_region(76,20)),0) # Hot spring retreat
        self.assertEqual(float(snow_region(89,25)),0) # Lightning tableland
    def test_vertical_mountain_faces_on_tile_boundaries_keep_their_area(self):
        for triangle in [np.array([[6400,100,0],[6400,300,0],[6400,300,500.]],float),
                         np.array([[100,6400,0],[300,6400,0],[300,6400,500.]],float)]:
            expected=np.linalg.norm(np.cross(triangle[1]-triangle[0],triangle[2]-triangle[0]))/2
            pieces=[face for _,polygon in split_tiles(triangle) for face in fan(polygon)]
            actual=sum(np.linalg.norm(np.cross(f[1]-f[0],f[2]-f[0]))/2 for f in pieces)
            self.assertAlmostEqual(actual,expected)

    def test_cut_edge_extending_past_a_corner_still_keeps_its_knots(self):
        region=Region.__new__(Region)
        region.polygon=np.array([[0,0],[100,0],[100,100],[0,100]],float)
        region.profiles=[(np.array([0,.98,1]),np.array([10,40,10])),
                         *[(np.array([0,1]),np.array([10,10])) for _ in range(3)]]
        triangle=np.array([[95,0,10],[105,0,10],[100,-100,10]],float)
        cut=region.stitch(triangle)
        self.assertTrue(any(np.array_equal(v,[98,0,40]) for v in cut))
        self.assertTrue(any(np.array_equal(v,[100,0,10]) for v in cut))

    def test_native_height_break_is_part_of_the_walkable_transition(self):
        region=Region.__new__(Region)
        region.polygon=np.array([[0,0],[100,0],[100,100],[0,100]],float)
        region.profiles=[(np.array([0,.25,.5,.75,1]),np.array([10,10,40,10,10])),
                         *[(np.array([0,1]),np.array([10,10])) for _ in range(3)]]
        triangle=np.array([[0,0,10],[100,0,10],[50,-100,10]],float)
        cut=region.stitch(triangle)
        center=triangle.mean(0)
        triangles=fan(cut,center)
        areas=cross2(triangles[:,1,:2]-triangles[:,0,:2],triangles[:,2,:2]-triangles[:,0,:2])
        upward=triangles[areas<-.001]
        self.assertTrue(any(np.array_equal(v,[50,0,40]) for face in upward for v in face))
        self.assertAlmostEqual(abs(areas.sum()/2),5000)
        self.assertEqual(len(upward),6)

    def test_visible_cliff_height_can_differ_from_walking_collision(self):
        lower=np.array([[0,0,10],[0,100,10],[100,0,10]],float)
        upper=lower.copy();upper[:,2]=140
        sampler=SurfaceSampler([('grassland',lower),('grassland',upper)])
        self.assertAlmostEqual(float(sampler.sample(20,20)[0]),10)
        self.assertAlmostEqual(float(sampler.sample(20,20,highest=True)[0]),140)

    def test_forest_tips_survive_when_the_triangle_centroid_is_outside(self):
        from surfaces import clip_level
        triangle=np.array([[0,0,1.],[100,0,0],[0,100,0]])
        clipped=clip_level(triangle,2,.4)
        self.assertEqual(len(clipped),3)
        self.assertTrue((clipped[:,2]>=.4-1e-9).all())
        self.assertAlmostEqual(abs(cross2(clipped[1,:2]-clipped[0,:2],clipped[2,:2]-clipped[0,:2]))/2,1800)


class ShoreRibbons(unittest.TestCase):
    def test_native_coasts_keep_replacement_waves_inside_district_cores(self):
        from surfaces import coast_meshes
        class Core:
            def outside(self,polygon):return []
        meshes=coast_meshes([(np.array([100.,100]),np.array([900.,100]),np.array([0.,1]))],Core(),256)
        self.assertTrue(meshes)

    def test_exported_waves_never_sample_adjacent_animation_frames(self):
        root=os.environ.get('RESONANCE_UNITED_MOD')
        if not root:self.skipTest('set RESONANCE_UNITED_MOD to a completed map build')
        count=0
        for path in Path(root).glob('world/united/terrain/*/wave-ribbons.glb'):
            model=GLB(path)
            for obj in model.doc['meshes']:
                uv=model.array(obj['primitives'][0]['attributes']['TEXCOORD_0'])
                self.assertGreaterEqual(float(uv[:,1].min()),.5/256-1e-7)
                self.assertLessEqual(float(uv[:,1].max()),1/8-.5/256+1e-7)
                count+=1
        self.assertGreater(count,0)

    def test_wave_coordinates_stay_inside_one_animation_frame(self):
        from surfaces import coast_meshes
        class Ocean:
            def outside(self,polygon):return [polygon]
        meshes=coast_meshes([(np.array([100.,100]),np.array([900.,100]),np.array([0.,1]))],Ocean(),256)
        self.assertTrue(meshes)
        for mesh in meshes.values():
            uv=np.asarray(mesh.uv)
            self.assertGreaterEqual(uv[:,1].min(),.5/256)
            self.assertLessEqual(uv[:,1].max(),1/8-.5/256)
            self.assertTrue(np.isfinite(mesh.p).all())

    def test_coast_is_extracted_at_the_visible_ground_water_intersection(self):
        from surfaces import sea_crossing
        face=np.array([[0,0,-10.],[100,0,10],[0,100,-10]])
        a,b,normal=sea_crossing(face)
        self.assertAlmostEqual(a[0],50)
        self.assertAlmostEqual(b[0],50)
        self.assertLess(normal[0],0)


class NativeDoorway(unittest.TestCase):
    def test_unnamed_iselia_crowns_are_classified_by_their_forest_material(self):
        from native_regions import NativeLibrary
        assets=Path(__file__).resolve().parents[3]/'local/all-assets'
        if not (assets/'world/landmarks.json').exists():self.skipTest('requires imported native assets')
        parts=NativeLibrary(assets).tile('sylvarant',1,4)[0]
        unnamed=[p for _,primitives in parts for p in primitives if p['name'] in ['Mesh38','Mesh39','Mesh40']]
        self.assertEqual(len(unnamed),3)
        self.assertTrue(all(p['forest'] for p in unnamed))

    def test_second_uv_layer_cannot_be_reinterpreted_as_positions(self):
        from build import build_confluence_entrance
        assets=Path(__file__).resolve().parents[3]/'local/all-assets'
        if not (assets/'world/landmarks.json').exists():self.skipTest('requires imported native assets')
        with tempfile.TemporaryDirectory() as directory:
            out=Path(directory);(out/'world/markers').mkdir(parents=True)
            (out/'world/markers/mappings.json').write_text('{}')
            build_confluence_entrance(assets,out)
            model=GLB(out/'world/markers/confluence-mouth/model.glb')
            for node in model.doc['nodes']:
                mesh=model.doc['meshes'][node['mesh']]['primitives'][0]
                attributes=mesh['attributes']
                p=model.array(attributes['POSITION'])
                self.assertLess(np.ptp(p,axis=0).max(),600)
                self.assertIn('TEXCOORD_1',attributes)
                colors=model.array(attributes['COLOR_0'])
                self.assertTrue(((colors>=0)&(colors<=1)).all())
                self.assertNotIn('doorway',node['name'])


class TextureJoins(unittest.TestCase):
    def test_native_paint_uses_filtered_texel_centres_and_wrap_modes(self):
        texture=np.array([[[1.,0,0,1],[0,1,0,1]],[[0,0,1,1],[1,1,1,1]]])
        np.testing.assert_allclose(texture_sample(texture,np.array([.25,.25]),{}),[1,0,0,1])
        np.testing.assert_allclose(texture_sample(texture,np.array([0.,0.]),{}),[.5,.5,.5,1])
        np.testing.assert_allclose(texture_sample(texture,np.array([0.,0.]),{'wrap_u':'clamp','wrap_v':'clamp'}),[1,0,0,1])

    def test_adjacent_generated_tiles_share_filtering_gutters(self):
        from landscape import read_ktx
        root=os.environ.get('RESONANCE_UNITED_MOD')
        if not root:self.skipTest('set RESONANCE_UNITED_MOD to a completed map build')
        tiles={}
        def tile(col,row):
            if (col,row) not in tiles:
                tiles[col,row]=read_ktx(Path(root)/f'world/united/terrain/column-{col:02}-row-{row:02}/albedo.ktx2')
            return tiles[col,row]
        for row in range(9):
            for col in range(12):
                current=tile(col,row);self.assertEqual(current.shape[:2],(514,514))
                if col<11:np.testing.assert_array_equal(current[:,-2:],tile(col+1,row)[:,:2])
                if row<8:np.testing.assert_array_equal(current[-2:],tile(col,row+1)[:2])


class ExportedSeams(unittest.TestCase):
    def test_all_exported_world_mesh_attributes_and_indices_are_valid(self):
        root=os.environ.get('RESONANCE_UNITED_MOD')
        if not root:self.skipTest('set RESONANCE_UNITED_MOD to a completed map build')
        count=0
        for path in Path(root).glob('world/united/**/*.glb'):
            model=GLB(path)
            for mesh in model.doc['meshes']:
                for primitive in mesh['primitives']:
                    attrs=primitive['attributes'];positions=model.array(attrs['POSITION'])
                    for name,index in attrs.items():
                        self.assertTrue(np.isfinite(model.array(index)).all(),f'{path}: {name}')
                    indices=model.array(primitive['indices'])
                    self.assertLess(int(indices.max(initial=0)),len(positions),str(path))
                    self.assertLessEqual(float(np.abs(positions[:,:2]).max(initial=0)),3200.1,str(path))
            count+=1
        self.assertGreater(count,100)

    def test_mountain_backing_draws_before_its_translucent_cap(self):
        root=os.environ.get('RESONANCE_UNITED_MOD')
        if not root:self.skipTest('set RESONANCE_UNITED_MOD to a completed map build')
        root=Path(root);checked=0
        for path in (root/'world/united/terrain').glob('*/terrain.json'):
            for part in json.loads(path.read_text())['parts']:
                if '/iselia-woodland-basin/' not in part['mesh'] or not part['mesh'].endswith('source-01-04-part-0.glb'):continue
                model=GLB(root/part['mesh'])
                orders={mesh:material['draw_order'][i] for material in part['materials'] for i,mesh in enumerate(material['meshes'])}
                names={node['name'].rpartition('-')[0]:orders[node['mesh']] for node in model.doc['nodes']}
                if 'mountain_isa' in names and 'mountain_isa_top01' in names:
                    self.assertLess(names['mountain_isa'],names['mountain_isa_top01']);checked+=1
        self.assertGreater(checked,0)

    def test_native_border_knots_are_present_in_the_exported_join(self):
        from native_regions import NativeLibrary
        from design import DISTRICTS
        from scipy.spatial import cKDTree
        root=os.environ.get('RESONANCE_UNITED_MOD')
        if not root:self.skipTest('set RESONANCE_UNITED_MOD to a completed map build')
        root=Path(root);assets=Path(__file__).resolve().parents[3]/'local/all-assets'
        original=json.loads((assets/'world/landmarks.json').read_text())
        landmarks={e['id']:e for world in original.values() for e in world}
        library=NativeLibrary(assets)
        for spec in DISTRICTS:
            name=spec.name;region=Region(spec,landmarks,library)
            points=[]
            for col in range(int(region.lo[0]//6400),int(region.hi[0]//6400)+1):
                for row in range(int(region.lo[1]//6400),int(region.hi[1]//6400)+1):
                    path=root/f'world/united/terrain/column-{col:02}-row-{row:02}/land.glb'
                    if not path.exists():continue
                    mesh=GLB(path)
                    for obj in mesh.doc['meshes']:
                        p=mesh.array(obj['primitives'][0]['attributes']['POSITION']).astype(float)
                        p[:,0]+=col*6400+3200;p[:,1]=row*6400+3200-p[:,1]
                        points.extend(p)
            exported=cKDTree(np.asarray(points))
            for (a,b),(ts,z) in zip(zip(region.polygon,np.roll(region.polygon,-1,axis=0)),region.profiles):
                expected=np.column_stack([a+ts[:,None]*(b-a),z])
                error,_=exported.query(expected)
                self.assertLess(float(error.max()),.005,f'{name} has a missing native border vertex')


if __name__=='__main__':unittest.main()
