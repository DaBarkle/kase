"""Renders Kase's AirPods 4 clips with Blender (bpy 5.2): one PNG per frame at 144 fps.

  python shots.py SCENE_GLTF OUT_DIR CLIP [--preview]

Clips (docs/JOURNEYS.md says which moment plays which):
  spin-open  first connect of the day: the closed case turns 360 degrees, the lid opens,
             the pods spiral up out of the wells and settle with a small bounce
  put-away   both pods drop back in and the lid closes

--preview renders 220 px at 16 samples without motion blur, for judging the motion.
"""

import argparse
import math
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rig  # noqa: E402
from springs import settle, step  # noqa: E402

FPS = 144


def spin_open(t: float):
    """Pose at t seconds: spin degrees, lid closed amount, per-bud (seated, orbit)."""
    spin = 360.0 * step(t, 0.9, 0.0)
    lid_at = 0.62
    lid = 1.0 - step(t - lid_at, 0.55, 0.25)
    buds = {}
    for side, delay in (("L", 0.0), ("R", 0.08)):
        p = step(t - lid_at - 0.22 - delay, 0.6, 0.28)
        # The vortex: each pod swings round the case while it rises, and is square
        # again when seated and when out (so it never passes through a wall).
        q = min(max(p, 0.0), 1.0)
        orbit = 140.0 * 4.0 * q * (1.0 - q)
        buds[side] = (1.0 - p, orbit)
    return spin, lid, buds


def spin_open_length() -> float:
    return 0.62 + 0.22 + 0.08 + settle(0.6, 0.28)


def put_away(t: float):
    buds = {}
    for side, delay in (("L", 0.0), ("R", 0.06)):
        buds[side] = (step(t - delay, 0.45, 0.0), 0.0)
    lid = step(t - 0.42, 0.35, 0.1)
    return 0.0, lid, buds


def put_away_length() -> float:
    return 0.42 + settle(0.35, 0.1)


CLIPS = {
    "spin-open": (spin_open, spin_open_length),
    "put-away": (put_away, put_away_length),
}


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    ap = argparse.ArgumentParser()
    ap.add_argument("gltf")
    ap.add_argument("out")
    ap.add_argument("clip", choices=sorted(CLIPS))
    ap.add_argument("--preview", action="store_true")
    ap.add_argument("--res", type=int, default=440)
    ap.add_argument("--samples", type=int, default=64)
    a = ap.parse_args(argv)
    pose, length = CLIPS[a.clip]
    r = rig.Rig(a.gltf)
    res, samples = (220, 16) if a.preview else (a.res, a.samples)
    rig.stage(res, samples, blur=not a.preview, fps=FPS)
    frames = math.ceil(length() * FPS) + 1
    for f in range(frames):
        spin, lid, buds = pose(f / FPS)
        r.set_spin(spin)
        r.set_lid(lid)
        for side, (seated, orbit) in buds.items():
            r.set_bud(side, seated, orbit)
        r.key(f + 1)
    sc = bpy.context.scene
    sc.frame_start, sc.frame_end = 1, frames
    os.makedirs(a.out, exist_ok=True)
    sc.render.filepath = os.path.join(a.out, "f")
    print(f"{a.clip}: {frames} frames at {FPS} fps, {res} px")
    bpy.ops.render.render(animation=True)


if __name__ == "__main__":
    main()
