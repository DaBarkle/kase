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


# The live card's hover pose: beside the case and a little lower than the model's "out"
# pose, so a pod that is out stays in frame and clear of the lid as it closes (collide.py
# lid-close-L/R; the lid sweeps through the space right above the wells).
HOVER_LIFT_MM = -6.0
HOVER_OUT_MM = 16.0


def rise(t: float, twirl: bool, hover: bool = False):
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
    up, side = (HOVER_LIFT_MM * u, HOVER_OUT_MM * u) if hover else (0.0, 0.0)
    if not twirl:
        return seated, 0.0, lift + up, 0.0, side
    w = clamp01(step(t - 0.26, 0.8, 0.0))
    bump = math.sin(math.pi * w)
    return seated, 0.0, lift + up + 3.5 * bump, 360.0 * w, side + 8.0 * bump


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


# --- Live case clips --------------------------------------------------------------
# The live case card stacks three layers that line up pixel for pixel: the case (with the
# lid and any seated pods baked in) and one layer per pod. The state machine in the
# overlay (case_pose.rs) picks a clip per layer from what the AirPods report.
#
#   lid-open-<seated>   the lid opens over the pods that are in the case
#   lid-close-<seated>  the lid closes over them; a pod that is out hovers clear of the lid
#   case-<seated>       a still of the open case
#   rise-<side>         one pod lifts straight out of its well, then twirls up to hover
#   sink-<side>         from hovering, one pod turns and drops straight into its well
#
# <seated> is both, L, R or none. Pods that are out are hidden in the case layer (their own
# layer draws them), so collide.py still checks them against the lid.

SEATED = (1.0, 0.0, 0.0, 0.0, 0.0)
OUT = (0.0, 0.0, HOVER_LIFT_MM, 0.0, HOVER_OUT_MM)


def _pods(seated: str):
    return {s: (SEATED if seated in ("both", s) else OUT) for s in ("L", "R")}


def lid_open(seated: str):
    return lambda t: (0.0, 1.0 - step(t, 0.55, 0.25), _pods(seated))


def spin_lid_open(t: float):
    """First connect of the day, live: the closed case turns 360 degrees and the lid opens
    over both pods, which stay seated until they are really taken out."""
    spin = 360.0 * step(t, 0.9, 0.0)
    lid = 1.0 - step(t - 0.62, 0.55, 0.25)
    return spin, lid, _pods("both")


def spin_lid_open_length() -> float:
    return 0.62 + settle(0.55, 0.25)


def lid_close(seated: str):
    return lambda t: (0.0, step(t, 0.35, 0.1), _pods(seated))


def still(seated: str):
    return lambda t: (0.0, 0.0, _pods(seated))


def rise_one(side: str, other_seated: bool):
    other = "R" if side == "L" else "L"
    return lambda t: (0.0, 0.0, {side: rise(t, True, hover=True), other: SEATED if other_seated else OUT})


def sink(t: float):
    u = step(t, 0.32, 0.0)  # turn to the seated angle and come in over the well
    drop = step(t - 0.22, 0.34, 0.0)  # straight down into the well
    v = 1.0 - u
    return (u, 0.0, rig.SEAT_DEPTH_MM * u * (1.0 - drop) + HOVER_LIFT_MM * v, 0.0,
            HOVER_OUT_MM * v)


def sink_one(side: str, other_seated: bool):
    other = "R" if side == "L" else "L"
    return lambda t: (0.0, 0.0, {side: sink(t), other: SEATED if other_seated else OUT})


def lid_open_length() -> float:
    return settle(0.55, 0.25)


def lid_close_length() -> float:
    return settle(0.35, 0.1)


def rise_length() -> float:
    return 0.26 + max(settle(0.62, 0.26), settle(0.8, 0.0))


def sink_length() -> float:
    return 0.22 + settle(0.34, 0.0)


CLIPS = {
    "open": (open_case, open_case_length),
    "spin-open": (spin_open, spin_open_length),
    "put-away": (put_away, put_away_length),
}
# Which layer each live clip renders: "case" hides the pods that are out; "L"/"R" render
# that pod alone with the case and lid as a holdout (cut out where they are in front).
LAYER = {}
for _s in ("both", "L", "R"):
    CLIPS[f"lid-open-{_s}"] = (lid_open(_s), lid_open_length)
    CLIPS[f"lid-close-{_s}"] = (lid_close(_s), lid_close_length)
    LAYER[f"lid-open-{_s}"] = LAYER[f"lid-close-{_s}"] = "case"
CLIPS["spin-lid-open"] = (spin_lid_open, spin_lid_open_length)
LAYER["spin-lid-open"] = "case"
for _s in ("both", "L", "R", "none"):
    CLIPS[f"case-{_s}"] = (still(_s), lambda: 0.0)
    LAYER[f"case-{_s}"] = "case"
for _side in ("L", "R"):
    # The pod layer is rendered with the other pod hidden; collide.py checks the move
    # with the other pod seated (the tight case) via these clips, and hovering via -beside.
    CLIPS[f"rise-{_side}"] = (rise_one(_side, True), rise_length)
    CLIPS[f"sink-{_side}"] = (sink_one(_side, True), sink_length)
    CLIPS[f"rise-{_side}-beside"] = (rise_one(_side, False), rise_length)
    CLIPS[f"sink-{_side}-beside"] = (sink_one(_side, False), sink_length)
    LAYER[f"rise-{_side}"] = LAYER[f"sink-{_side}"] = _side


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    ap = argparse.ArgumentParser()
    ap.add_argument("gltf")
    ap.add_argument("out")
    ap.add_argument("clip", choices=sorted(CLIPS))
    ap.add_argument("--preview", action="store_true")
    ap.add_argument("--res", type=int, default=440)
    ap.add_argument("--samples", type=int, default=64)
    ap.add_argument("--resume", action="store_true", help="keep frames already in OUT")
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
    layer = LAYER.get(a.clip)
    if layer:
        r.set_layer(layer, buds_out={s for s, b in pose(0.0)[2].items() if b == OUT})
    sc = bpy.context.scene
    sc.frame_start, sc.frame_end = 1, frames
    os.makedirs(a.out, exist_ok=True)
    sc.render.filepath = os.path.join(a.out, "f")
    sc.render.use_overwrite = not a.resume
    print(f"{a.clip}: {frames} frames at {FPS} fps, {res} px")
    bpy.ops.render.render(animation=True)


if __name__ == "__main__":
    main()
