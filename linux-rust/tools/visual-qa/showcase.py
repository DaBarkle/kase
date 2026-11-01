#!/usr/bin/env python3
"""Records the README demos: the real island and app window, frame by frame on the
test clock, encoded as animated WebP with a transparent background.

    showcase.py                 # every clip, into docs/media/
    showcase.py hero window     # only these (name prefixes)
    showcase.py --list

Nothing is screen-recorded: the app draws each frame at an exact test time
(`KASE_TEST_STEP_MS`, see src/testclock.rs) and saves it, so every frame of every
spring is there at 60 fps however slow the software renderer is. The island already
draws on a transparent surface; the window gets rounded corners and a hairline edge.
The 3D clips are re-timed straight from assets/anim/ (rendered at 144 fps).

Needs what kase_qa.py needs (sway, grim, lavapipe, dbus-run-session, Pillow with WebP)
and release builds: cargo build --release.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kase_qa  # noqa: E402

ROOT = kase_qa.ROOT
BIN = kase_qa.BIN
SHOWCASE = Path(__file__).resolve().parent / "showcase"
OUT = ROOT / "docs" / "media"
WORK = ROOT / "target" / "showcase"
FPS = 60
SCALE = 2


@dataclass
class Clip:
    name: str
    kind: str  # island | window | anim
    script: str = ""  # island: journey file in showcase/
    scenario: str = "airpods4"  # window: starting --ui-demo scenario
    steps: str = ""  # window: KASE_UI_SCRIPT
    end_ms: int = 4000
    theme: str = "dark"
    height: int | None = None
    anim: list[str] = field(default_factory=list)  # anim: clip dirs in assets/anim
    width: int = 0  # output width in px (0 = as captured)
    trim_ms: int = 0  # leave out what comes before this test time
    quality: int = 90
    note: str = ""


# The window: settle on the closed case, connect, change noise control, visit each tab.
WINDOW_STEPS = ("2600:scenario=anc;6200:mode=transparency;7600:mode=adaptive;"
                "9000:tab=island;10600:tab=settings;12000:tab=airpods")

CLIPS = [
    Clip("hero", "island", script="hero.json",
         note="Case opens, the card plays the 3D spin, pods come out, noise control"),
    Clip("alerts", "island", script="alerts.json",
         note="Charging, iPhone takes the audio and gives it back, mic, low battery"),
    Clip("window-dark", "window", scenario="putaway", theme="dark", end_ms=14600,
         steps=WINDOW_STEPS, trim_ms=2000, width=560, quality=88,
         note="AirPods connect: case opens, rings fill; noise control; then the tabs"),
    Clip("window-light", "window", scenario="putaway", theme="light", end_ms=14600,
         steps=WINDOW_STEPS, trim_ms=2000, width=560, quality=88),
    Clip("case-3d", "anim", anim=["airpods4-spin-open", "airpods4-put-away"], width=560,
         quality=92, note="The 3D clips on their own"),
]


# --------------------------------------------------------------------------- recording


def fresh_home(tmp: Path, settings: dict) -> dict:
    home = tmp / "home"
    cfg = home / ".config" / "kase"
    cfg.mkdir(parents=True, exist_ok=True)
    (cfg / "app_settings.json").write_text(json.dumps(settings))
    return dict(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                XDG_DATA_HOME=str(home / ".local/share"),
                XDG_STATE_HOME=str(home / ".local/state"))


def record(clip: Clip, frames: Path, timeout: float) -> None:
    shutil.rmtree(frames, ignore_errors=True)
    frames.mkdir(parents=True)
    tmp = Path(tempfile.mkdtemp(prefix="kase-showcase-"))
    sway = kase_qa.Sway(SCALE, tmp)
    try:
        env = sway.env()
        env.update(KASE_TEST_STEP_MS=str(1000.0 / FPS), KASE_SHOT_DIR=str(frames),
                   WGPU_BACKEND="vulkan", RUST_LOG="info")
        if clip.kind == "island":
            argv = [str(BIN / "kase-overlay"), "--script", str(SHOWCASE / clip.script)]
        else:
            env.update(fresh_home(tmp, {"appearance": clip.theme}))
            env.update(KASE_UI_SCRIPT=clip.steps, KASE_TEST_END_MS=str(clip.end_ms),
                       KASE_UI_TAB="AirPods")
            if clip.height:
                env["KASE_UI_HEIGHT"] = str(clip.height)
            argv = ["dbus-run-session", "--", str(BIN / "kase"),
                    f"--ui-demo={clip.scenario}", "--no-tray"]
        started = time.time()
        with open(frames.parent / "app.log", "w") as log:
            r = subprocess.run(argv, env=env, stdout=log, stderr=subprocess.STDOUT,
                               timeout=timeout)
        print(f"  recorded in {time.time() - started:.0f} s (exit {r.returncode})")
    finally:
        sway.close()
        shutil.rmtree(tmp, ignore_errors=True)


def timed_frames(frames: Path, surface: str) -> list[tuple[Path | None, float]]:
    """(file, test time in ms) for every captured frame, in order."""
    out = []
    for line in (frames / "frames.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        f = json.loads(line)
        name = (f.get("files") or {}).get(surface)
        out.append((frames / name if name else None, float(f["t_ms"])))
    return out


# --------------------------------------------------------------------------- framing


def round_window(img: Image.Image, radius: int, edge: tuple[int, int, int, int]):
    """Rounded corners and a hairline edge, transparent outside: just the window."""
    w, h = img.size
    mask = Image.new("L", (w * 4, h * 4), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w * 4 - 1, h * 4 - 1), radius * 4, 255)
    mask = mask.resize((w, h), Image.LANCZOS)
    out = img.convert("RGBA")
    out.putalpha(ImageChops.multiply(out.getchannel("A"), mask))
    ring = Image.new("L", (w * 4, h * 4), 0)
    d = ImageDraw.Draw(ring)
    d.rounded_rectangle((0, 0, w * 4 - 1, h * 4 - 1), radius * 4, outline=255, width=4)
    ring = ring.resize((w, h), Image.LANCZOS).point(lambda v: v * edge[3] // 255)
    line = Image.new("RGBA", (w, h), edge[:3] + (0,))
    line.putalpha(ring)
    return Image.alpha_composite(out, line)


def union_box(paths, pad: int):
    box = None
    size = None
    for p in paths:
        if p is None:
            continue
        a = Image.open(p).getchannel("A")
        size = a.size
        b = a.point(lambda v: 255 if v > 3 else 0).getbbox()
        if b:
            box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]),
                                         max(box[2], b[2]), max(box[3], b[3]))
    if box is None:
        return (0, 0) + (size or (2, 2))
    W, H = size
    return (max(box[0] - pad, 0), max(box[1] - pad, 0), min(box[2] + pad, W),
            min(box[3] + pad, H))


# --------------------------------------------------------------------------- encoding


def encode(images: list[Image.Image], durations: list[int], out: Path, quality: int):
    """Animated WebP, lossy colour with exact alpha, merging repeated frames."""
    merged_imgs, merged_d = [], []
    for img, d in zip(images, durations):
        if merged_imgs and ImageChops.difference(merged_imgs[-1], img).getbbox(alpha_only=False) is None:
            merged_d[-1] += d
            continue
        merged_imgs.append(img)
        merged_d.append(d)
    blinked = blinks(merged_imgs)
    if len(blinked) > MAX_HELD:
        raise SystemExit(f"  {out.name}: something blinks for one frame at frames "
                         f"{blinked[:10]} (see BLINK_ERROR)")
    for i in blinked:
        # A lone one-frame dropout (the glyph atlas growing mid-frame can blank a text
        # layer once): hold the frame before instead, and say so.
        print(f"  holding frame {i - 1} over a one-frame blink at frame {i}")
        merged_imgs[i] = merged_imgs[i - 1]
    if blinked:
        imgs, ds = [], []
        for img, d in zip(merged_imgs, merged_d):
            if imgs and img is imgs[-1]:
                ds[-1] += d
                continue
            imgs.append(img)
            ds.append(d)
        merged_imgs, merged_d = imgs, ds
    out.parent.mkdir(parents=True, exist_ok=True)
    merged_imgs[0].save(out, save_all=True, append_images=merged_imgs[1:],
                        duration=merged_d, loop=0, quality=quality, alpha_quality=100,
                        method=4, minimize_size=False, allow_mixed=False, exact=False,
                        background=(0, 0, 0, 0))
    total = sum(merged_d) / 1000
    print(f"  {out.relative_to(ROOT)}: {len(merged_imgs)} frames, {total:.1f} s, "
          f"{out.stat().st_size / 1e6:.2f} MB")
    bad = verify(out, merged_imgs)
    if bad:
        worst = max(bad, key=lambda b: b[1])
        raise SystemExit(f"  {out.name}: {len(bad)} frames differ from what was recorded "
                         f"(worst: frame {worst[0]}, tile error {worst[1]:.1f})")


# A decoded frame may differ from its source by lossy noise (about 11 at quality 88),
# never by more than this in any 16 px tile (mean absolute error, composited onto black
# and onto white). A stale or missing element is far above it.
MAX_TILE_ERROR = 24.0
# A tile that changes by more than this for a single frame and then comes back is a
# blink (the status pill text once vanished on alternate frames while its width settled).
BLINK_ERROR = 24.0
BLINK_TILES = 12
# More blinks than this in one clip is a bug to fix in the app, not to paper over.
MAX_HELD = 3


def tile_error(a: Image.Image, b: Image.Image, tile: int = 16) -> Image.Image:
    """Mean absolute error per tile, composited onto black and onto white (worst)."""
    worst = None
    for bg in ((0, 0, 0, 255), (255, 255, 255, 255)):
        x = Image.new("RGBA", a.size, bg)
        x.alpha_composite(a)
        y = Image.new("RGBA", a.size, bg)
        y.alpha_composite(b)
        diff = ImageChops.difference(x.convert("RGB"), y.convert("RGB")).convert("L")
        small = diff.resize((max(diff.width // tile, 1), max(diff.height // tile, 1)),
                            Image.BOX)
        worst = small if worst is None else ImageChops.lighter(worst, small)
    return worst


def blink_tiles(images: list[Image.Image], i: int) -> int:
    """8 px tiles that were still, change at frame i and are back (and still) after."""
    jump = tile_error(images[i - 1], images[i], 8)
    if jump.getextrema()[1] <= BLINK_ERROR:
        return 0
    # Tiles that are the same the frame before (and the one before that) as for the two
    # frames after: still, apart from what happens at frame i.
    still = Image.new("L", jump.size, 255)
    for j in (i - 2, i + 1, i + 2):
        if 0 <= j < len(images):
            same = tile_error(images[i - 1], images[j], 8)
            still = ImageChops.multiply(still, same.point(lambda v: 255 if v < 2 else 0))
    # Something crossing a still area within one frame looks the same; holding it costs
    # one repeated frame, which nobody sees, so that is fine.
    hit = ImageChops.multiply(jump.point(lambda v: 255 if v > BLINK_ERROR else 0), still)
    return hit.histogram()[255]


def blinks(images: list[Image.Image]) -> list[int]:
    """Frames where a still part of the picture changes for one frame only.

    Fast motion trips the tile test too, but over several frames in a row; a blink is
    a lone frame (the status pill's text once vanished on alternate frames).
    """
    n = [0, 0] + [blink_tiles(images, i) for i in range(2, len(images) - 1)] + [0]
    return [i for i in range(2, len(images) - 1)
            if n[i] >= BLINK_TILES and n[i - 1] < BLINK_TILES and n[i + 1] < BLINK_TILES]


def verify(path: Path, sources: list[Image.Image]) -> list[tuple[int, float]]:
    """Decodes every frame and checks it against its source, tile by tile."""
    im = Image.open(path)
    if im.n_frames != len(sources):
        raise SystemExit(f"  {path.name}: {im.n_frames} frames decoded, "
                         f"{len(sources)} encoded")
    bad = []
    for i, src in enumerate(sources):
        im.seek(i)
        got = im.convert("RGBA")
        err = tile_error(src, got).getextrema()[1]
        if err > MAX_TILE_ERROR:
            bad.append((i, err))
    return bad


def frame_durations(times: list[float], tail_ms: float) -> list[int]:
    """Whole-ms durations that add up exactly to the test-time gaps (no drift)."""
    ends = times[1:] + [times[-1] + tail_ms]
    out, shown = [], 0
    for t0, t1 in zip(times, ends):
        target = round(t1 - times[0])
        out.append(max(target - shown, 1))
        shown += out[-1]
    return out


def scaled(img: Image.Image, width: int) -> Image.Image:
    if not width or img.width == width:
        return img
    h = round(img.height * width / img.width)
    return img.resize((width, h), Image.LANCZOS)


def build(clip: Clip, frames: Path) -> None:
    surface = "island" if clip.kind == "island" else "window"
    tf = timed_frames(frames, surface)
    # Drop frames from before time zero (warm-up) and empty ones at the start.
    tf = [(p, t) for p, t in tf if t >= 0]
    if clip.trim_ms:
        before = [x for x in tf if x[1] <= clip.trim_ms]
        tf = ([(before[-1][0], float(clip.trim_ms))] if before else []) + [
            x for x in tf if x[1] > clip.trim_ms]
    if clip.kind == "island":
        # Start on the first frame with the island on screen, end on the last one.
        drawn = [i for i, (p, _) in enumerate(tf) if p is not None
                 and Image.open(p).getchannel("A").getextrema()[1] > 8]
        if drawn:
            tf = tf[drawn[0]:drawn[-1] + 1]
        box = union_box([p for p, _ in tf], pad=24)
        blank = Image.new("RGBA", (box[2] - box[0], box[3] - box[1]), (0, 0, 0, 0))
        imgs = [scaled(Image.open(p).convert("RGBA").crop(box) if p else blank, clip.width)
                for p, _ in tf]
    else:
        light = clip.theme == "light"
        edge = (0, 0, 0, 40) if light else (255, 255, 255, 46)
        imgs = []
        for p, _ in tf:
            img = Image.open(p).convert("RGBA")
            imgs.append(scaled(round_window(img, 14 * SCALE, edge), clip.width))
    times = [t for _, t in tf]
    encode(imgs, frame_durations(times, 1500), OUT / f"{clip.name}.webp", clip.quality)


def build_anim(clip: Clip) -> None:
    """The 3D clips back to back, re-timed from 144 fps to 60 fps, with a pause between."""
    imgs, durs = [], []
    for name in clip.anim:
        files = sorted((ROOT / "assets" / "anim" / name).glob("f*.webp"))
        n = len(files)
        src_fps = 144.0
        length_ms = n / src_fps * 1000
        steps = int(length_ms * FPS / 1000)
        for k in range(steps + 1):
            i = min(round(k / FPS * src_fps), n - 1)
            imgs.append(scaled(Image.open(files[i]).convert("RGBA"), clip.width))
        # Hold the last frame a second before the next clip.
        durs.extend(frame_durations([k * 1000 / FPS for k in range(steps + 1)], 1000))
    # Crop everything to the union of what is drawn.
    box = None
    for im in imgs:
        b = im.getchannel("A").point(lambda v: 255 if v > 3 else 0).getbbox()
        if b:
            box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]),
                                         max(box[2], b[2]), max(box[3], b[3]))
    pad = 12
    W, H = imgs[0].size
    box = (max(box[0] - pad, 0), max(box[1] - pad, 0), min(box[2] + pad, W),
           min(box[3] + pad, H))
    imgs = [im.crop(box) for im in imgs]
    encode(imgs, durs, OUT / f"{clip.name}.webp", clip.quality)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("names", nargs="*", help="clip name prefixes (default: all)")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--encode-only", action="store_true",
                    help="re-encode from the last recording")
    ap.add_argument("--timeout", type=float, default=1800)
    a = ap.parse_args()
    clips = [c for c in CLIPS if not a.names or any(c.name.startswith(n) for n in a.names)]
    if a.list:
        for c in CLIPS:
            print(f"{c.name:14} {c.kind:7} {c.note}")
        return
    for c in clips:
        print(c.name, flush=True)
        if c.kind == "anim":
            build_anim(c)
            continue
        frames = WORK / c.name / "frames"
        if not a.encode_only:
            record(c, frames, a.timeout)
        build(c, frames)


if __name__ == "__main__":
    main()
