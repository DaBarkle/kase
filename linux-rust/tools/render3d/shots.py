"""Renders Kase's AirPods 4 clips with Blender (bpy 5.2): one PNG per frame at 144 fps.

  python shots.py SCENE_GLTF OUT_DIR CLIP [--preview]

Clips (docs/JOURNEYS.md says which moment plays which):
  open       the case opens and the pods lift straight out, then turn to face you
  spin-open  first connect of the day: the closed case turns 360 degrees, the lid opens,
             the pods lift straight out, then spiral once and settle with a small bounce
  put-away   both pods turn above their wells, drop straight in, and the lid closes

Every clip must pass collide.py (no part passes through another at any frame).

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


def clamp01(x: float) -> float:
    return min(max(x, 0.0), 1.0)


def rise(t: float, twirl: bool):
    """One pod coming out of its well, t seconds after it starts moving.

    First it lifts straight up out of the well, keeping its seated angle, so it never
    touches the case. Only once it is clear does it turn to face forward and settle into
    the "out" pose with a small bounce; with `twirl` it also spins once about its own
    axis on the way, drifting outwards and upwards a little so it clears the other pod
    and the open lid (the vortex). Returns (seated, orbit, lift, twirl, out).
    """
    lift = rig.SEAT_DEPTH_MM * step(t, 0.42, 0.0)
    u = step(t - 0.26, 0.62, 0.26)  # turn and settle; overshoots past "out"
    seated = 1.0 - u
    # Keep the height: lifting a seated pod by SEAT_DEPTH_MM is the out height.
    lift = lift * clamp01(seated) if u > 0 else lift
    if not twirl:
        return seated, 0.0, lift, 0.0, 0.0
    w = clamp01(step(t - 0.26, 0.8, 0.0))
    bump = math.sin(math.pi * w)
    return seated, 0.0, lift + 3.5 * bump, 360.0 * w, 8.0 * bump


def spin_open(t: float):
    """Pose at t seconds: spin degrees, lid closed amount, per-pod pose."""
    spin = 360.0 * step(t, 0.9, 0.0)
    lid_at = 0.62
    lid = 1.0 - step(t - lid_at, 0.55, 0.25)
    buds = {side: rise(t - lid_at - 0.2 - delay, True) for side, delay in (("L", 0.0), ("R", 0.06))}
    return spin, lid, buds


def spin_open_length() -> float:
    return 0.62 + 0.2 + 0.06 + 0.26 + max(settle(0.62, 0.26), settle(0.8, 0.0))


def open_case(t: float):
    """The everyday case-open clip: the lid opens and the pods rise out."""
    lid = 1.0 - step(t, 0.55, 0.25)
    buds = {side: rise(t - 0.2 - delay, False) for side, delay in (("L", 0.0), ("R", 0.06))}
    return 0.0, lid, buds


def open_case_length() -> float:
    return 0.2 + 0.06 + 0.26 + settle(0.62, 0.26)


def put_away(t: float):
    """Both pods turn back to their seated angle above the wells, drop straight in, and
    the lid closes."""
    buds = {}
    for side, delay in (("L", 0.0), ("R", 0.05)):
        u = step(t - delay, 0.32, 0.0)  # turn to the seated angle, at out height
        drop = step(t - delay - 0.22, 0.34, 0.0)  # straight down into the well
        buds[side] = (u, 0.0, rig.SEAT_DEPTH_MM * u * (1.0 - drop), 0.0, 0.0)
    lid = step(t - 0.62, 0.35, 0.1)
    return 0.0, lid, buds


def put_away_length() -> float:
    return 0.62 + settle(0.35, 0.1)


CLIPS = {
    "open": (open_case, open_case_length),
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
        for side, b in buds.items():
            r.set_bud(side, *b)
        r.key(f + 1)
    sc = bpy.context.scene
    sc.frame_start, sc.frame_end = 1, frames
    os.makedirs(a.out, exist_ok=True)
    sc.render.filepath = os.path.join(a.out, "f")
    print(f"{a.clip}: {frames} frames at {FPS} fps, {res} px")
    bpy.ops.render.render(animation=True)


if __name__ == "__main__":
    main()
