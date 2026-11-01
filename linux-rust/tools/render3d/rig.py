"""Rig for the AirPods 4 model ("AirPods 4" by Falah3D, CC BY 4.0) in Blender 5.2 (bpy).

Imports the glTF from the `assets-3d-models` branch and adds empties that drive the
parts as rigid bodies, by node name (Sketchfab names never change; indices can):

  spin   whole case and pods, yaw about the case centre
  lid    opens/closes about the hinge (y 9.5, z 11.0 mm), +115.5 deg about X = closed
  bud.L / bud.R   from "out" (the model's pose, hovering above the case) to "seated"
                  (tilt 20 deg about X, yaw +/-55 deg about the stem bottom, then
                  +9 mm y and -30 mm z), plus an extra orbit about the case axis

The "Designed by Apple" text on the back is hidden: no Apple wording in the app.
Coordinates in mm in comments; Blender works in metres.
"""

import math

import bpy
from mathutils import Euler, Matrix, Vector

MM = 0.001
LID = "nJoEOlgGwcdXBHJ"
BUDS = {"L": "oTgukKFHsbAaGBa", "R": "MSpBhwzeYpJKGDV"}
LED = "tFfdZKQcDdlkGzD"
DECAL = "LnYYAmqquNQJFje"
LID_PIVOT = Vector((0.0, 9.5, 11.0)) * MM
LID_CLOSED_DEG = 115.5
STEM = {"L": Vector((-12.64, -4.60, 20.54)) * MM, "R": Vector((12.64, -4.60, 20.54)) * MM}
SEAT_TILT_DEG = 20.0
SEAT_YAW_DEG = {"L": 55.0, "R": -55.0}
SEAT_SHIFT = Vector((0.0, 9.0, -30.0)) * MM
CASE_CENTRE = Vector((0.0, 0.0, 2.0)) * MM


def _empty(name, at, parent=None):
    e = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(e)
    e.location = at
    if parent:
        bpy.context.view_layer.update()
        e.parent = parent
        e.matrix_parent_inverse = parent.matrix_world.inverted()
    bpy.context.view_layer.update()
    return e


def _adopt(child, parent):
    # matrix_world is only current after an update; stale matrices shift parts.
    bpy.context.view_layer.update()
    mw = child.matrix_world.copy()
    child.parent = parent
    child.matrix_parent_inverse = parent.matrix_world.inverted()
    child.matrix_world = mw
    bpy.context.view_layer.update()


def _hide_tree(name):
    o = bpy.data.objects.get(name)
    if not o:
        return
    for x in [o, *o.children_recursive]:
        x.hide_render = True
        x.hide_viewport = True


class Rig:
    def __init__(self, gltf: str):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        bpy.ops.import_scene.gltf(filepath=gltf)
        bpy.context.view_layer.update()
        _hide_tree(DECAL)
        root = bpy.data.objects["Sketchfab_model"]
        self.spin = _empty("spin", CASE_CENTRE)
        _adopt(root, self.spin)
        bpy.context.view_layer.update()
        self.lid = _empty("lid", LID_PIVOT, self.spin)
        _adopt(bpy.data.objects[LID], self.lid)
        self.buds = {}
        for side, node in BUDS.items():
            # orbit: about the case's vertical axis; pivot: at the stem bottom.
            orbit = _empty(f"orbit.{side}", CASE_CENTRE, self.spin)
            pivot = _empty(f"bud.{side}", STEM[side], orbit)
            _adopt(bpy.data.objects[node], pivot)
            self.buds[side] = (orbit, pivot)
        bpy.context.view_layer.update()
        self.led = self._led_material()

    def _led_material(self):
        o = bpy.data.objects.get(LED)
        meshes = [m for m in ([o] + list(o.children_recursive)) if m.type == "MESH"] if o else []
        for m in meshes:
            for slot in m.material_slots:
                if slot.material and slot.material.node_tree:
                    return slot.material
        return None

    # -- poses ------------------------------------------------------------------

    def set_spin(self, deg: float):
        self.spin.rotation_euler = Euler((0.0, 0.0, math.radians(deg)))

    def set_lid(self, closed: float):
        """0 = open (model pose), 1 = closed."""
        self.lid.rotation_euler = Euler((math.radians(LID_CLOSED_DEG * closed), 0.0, 0.0))

    def set_bud(self, side: str, seated: float, orbit_deg: float = 0.0, lift_mm: float = 0.0):
        """seated 0 = out (model pose), 1 = in its well. orbit spins it about the case."""
        orbit, pivot = self.buds[side]
        orbit.rotation_euler = Euler((0.0, 0.0, math.radians(orbit_deg)))
        s = seated
        pivot.rotation_euler = Euler(
            (math.radians(SEAT_TILT_DEG * s), 0.0, math.radians(SEAT_YAW_DEG[side] * s)))
        pivot.location = STEM[side] + SEAT_SHIFT * s + Vector((0.0, 0.0, lift_mm * MM))

    def key(self, frame: int):
        for o in [self.spin, self.lid, *[x for pair in self.buds.values() for x in pair]]:
            o.keyframe_insert("rotation_euler", frame=frame)
            o.keyframe_insert("location", frame=frame)


def stage(res: int, samples: int, blur: bool = True, fps: int = 144):
    """Camera, lights and render settings (front-on, 150 mm lens, transparent film)."""
    sc = bpy.context.scene
    cam = bpy.data.cameras.new("cam")
    cam.lens = 150
    co = bpy.data.objects.new("cam", cam)
    sc.collection.objects.link(co)
    sc.camera = co
    target = Vector((0.0, 0.0, 12.0)) * MM
    co.location = target + Vector((0.0, -0.40, 0.045))
    co.rotation_euler = (target - co.location).to_track_quat("-Z", "Y").to_euler()
    for name, loc, energy, size in [
        ("key", (0.25, -0.30, 0.35), 6.0, 0.3),
        ("fill", (-0.30, -0.20, 0.10), 2.0, 0.4),
        ("rim", (0.0, 0.30, 0.30), 4.0, 0.3),
    ]:
        light = bpy.data.lights.new(name, "AREA")
        light.energy = energy
        light.size = size
        lo = bpy.data.objects.new(name, light)
        sc.collection.objects.link(lo)
        lo.location = target + Vector(loc)
        lo.rotation_euler = (target - lo.location).to_track_quat("-Z", "Y").to_euler()
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.15
    sc.world = world
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = samples
    sc.cycles.use_denoising = True
    sc.render.film_transparent = True
    sc.render.resolution_x = sc.render.resolution_y = res
    sc.render.resolution_percentage = 100
    sc.render.fps = fps
    sc.render.use_motion_blur = blur
    sc.render.motion_blur_shutter = 0.5
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
