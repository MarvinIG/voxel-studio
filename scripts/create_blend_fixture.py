"""blender --background --python scripts/create_blend_fixture.py -- assets/textured-cube.blend"""
import bpy, sys
from pathlib import Path
output = Path(sys.argv[sys.argv.index('--') + 1]).resolve()
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
bpy.ops.mesh.primitive_cube_add()
mat = bpy.data.materials.new('Checker')
mat.use_nodes = True
nodes = mat.node_tree.nodes
tex = nodes.new('ShaderNodeTexImage')
tex.image = bpy.data.images.load(str(output.parent / 'checker.png'))
tex.image.pack()
mat.node_tree.links.new(tex.outputs['Color'], nodes.get('Principled BSDF').inputs['Base Color'])
bpy.context.object.data.materials.append(mat)
bpy.ops.wm.save_as_mainfile(filepath=str(output))
