"""Checks a clip for parts passing through each other, frame by frame.

  python collide.py SCENE_GLTF CLIP [--every N]

Builds a BVH of every rigid part (case body, lid, left pod, right pod) in world space at
each frame of the clip and reports frames where two parts' surfaces intersect. Exits 1 if
any frame collides.

The wells in the model are shallower than the pods, so a seated pod's stem goes through
the well floor. That part of the case (below WELL_FLOOR_MM, covered by the pod or reading
as the stem hole) is left out; everything you can see, the rim, the well walls above it,
the outside of the case and the lid, must never be touched. The seated rest pose still
touches the well walls where the pod rests, so the overlap in the rest poses is the
baseline and a frame is a collision when a pair overlaps more than that.
"""

import argparse
import math
import os
import sys

import bpy
from mathutils.bvhtree import BVHTree

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rig  # noqa: E402
import shots  # noqa: E402


def _meshes(o):
    return [x for x in [o, *o.children_recursive] if x.type == "MESH" and not x.hide_render]


def parts(r):
    lid = set(_meshes(bpy.data.objects[rig.LID]))
    buds = {s: set(_meshes(bpy.data.objects[n])) for s, n in rig.BUDS.items()}
    taken = lid | buds["L"] | buds["R"]
    body = {m for m in _meshes(bpy.data.objects["Sketchfab_model"]) if m not in taken}
    return {"case": body, "lid": lid, "L": buds["L"], "R": buds["R"]}


WELL_FLOOR_MM = 2.0


def tree(meshes, zmin=None):
    """BVH of the meshes in world space; zmin (mm, case frame) drops polygons below it."""
    dg = bpy.context.evaluated_depsgraph_get()
    verts, polys = [], []
    for m in meshes:
        ev = m.evaluated_get(dg)
        me = ev.to_mesh()
        mw = ev.matrix_world
        base = len(verts)
        verts += [mw @ v.co for v in me.vertices]
        for p in me.polygons:
            idx = [base + i for i in p.vertices]
            if zmin is not None:
                # Height in the case's own frame, so the spin doesn't change it.
                z = sum((m.matrix_world @ me.vertices[i - base].co).z for i in idx) / len(idx)
                if z / rig.MM < zmin:
                    continue
            polys.append(idx)
        ev.to_mesh_clear()
    return BVHTree.FromPolygons(verts, polys)


PAIRS = [("L", "case"), ("R", "case"), ("L", "lid"), ("R", "lid"), ("L", "R")]


def overlaps(p):
    bpy.context.view_layer.update()
    t = {k: tree(v, WELL_FLOOR_MM if k == "case" else None) for k, v in p.items()}
    return {f"{a}-{b}": len(t[a].overlap(t[b])) for a, b in PAIRS}


def pose(r, spin, lid, buds):
    r.set_spin(spin)
    r.set_lid(lid)
    for side, b in buds.items():
        r.set_bud(side, *b)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    ap = argparse.ArgumentParser()
    ap.add_argument("gltf")
    ap.add_argument("clip", choices=sorted(shots.CLIPS))
    ap.add_argument("--every", type=int, default=1)
    a = ap.parse_args(argv)
    r = rig.Rig(a.gltf)
    p = parts(r)
    # Baselines: what the rest poses touch by design.
    pose(r, 0, 1.0, {"L": (1.0, 0.0), "R": (1.0, 0.0)})
    seated_closed = overlaps(p)
    pose(r, 0, 0.0, {"L": (1.0, 0.0), "R": (1.0, 0.0)})
    seated_open = overlaps(p)
    pose(r, 0, 0.0, {"L": (0.0, 0.0), "R": (0.0, 0.0)})
    out_open = overlaps(p)
    base = {k: max(seated_closed[k], seated_open[k], out_open[k]) for k in seated_closed}
    print("baseline (rest poses):", base)
    fn, length = shots.CLIPS[a.clip]
    frames = math.ceil(length() * shots.FPS) + 1
    bad = []
    for f in range(0, frames, a.every):
        pose(r, *fn(f / shots.FPS))
        o = overlaps(p)
        # Resting contact counts wobble by a few triangles as a pod settles the last
        # fraction of a millimetre into its well; 2 % over the rest pose is still resting.
        hit = {k: v for k, v in o.items() if v > base[k] * 1.02}
        if hit:
            bad.append(f)
            print(f"frame {f:3d} t={f / shots.FPS:.3f}s COLLIDES {hit}")
    print(f"{a.clip}: {frames} frames, {len(bad)} colliding")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
