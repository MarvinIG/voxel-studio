"""Invoked by the Rust importer: blender -b input.blend --python ... -- output.obj"""
import bpy
import sys
from pathlib import Path

output = Path(sys.argv[sys.argv.index('--') + 1])
bpy.ops.object.select_all(action='DESELECT')
for obj in bpy.context.scene.objects:
    if obj.type in {'MESH', 'CURVE', 'SURFACE', 'FONT', 'META'} and not obj.hide_render:
        obj.select_set(True)
# Extract packed images alongside the OBJ so map_Kd stays resolvable.
for image in bpy.data.images:
    if image.packed_file:
        image.filepath_raw = str(output.parent / (Path(image.name).stem + '.png'))
        image.file_format = 'PNG'
        image.save()
if hasattr(bpy.ops.wm, 'obj_export'):
    bpy.ops.wm.obj_export(filepath=str(output), export_selected_objects=True,
                          export_materials=True, export_triangulated_mesh=True,
                          path_mode='COPY', forward_axis='NEGATIVE_Z', up_axis='Y')
else:
    bpy.ops.export_scene.obj(filepath=str(output), use_selection=True,
                             use_materials=True, use_triangles=True, path_mode='COPY',
                             axis_forward='-Z', axis_up='Y')
