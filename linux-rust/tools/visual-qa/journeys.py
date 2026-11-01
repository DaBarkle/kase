"""Scripted journeys: play a user journey on the island with the frame-exact test clock
and turn every frame into things a person (or Claude) can review.

Each journey in `journeys/*.json` is a script for `kase-overlay --script` (format in
`src/bin/kase-overlay/record.rs`). For each one this writes, under OUT/<journey>/:

  frames/        every captured frame as drawn (transparent around the island) and
                 frames.jsonl, one line per frame: test time, what the island showed
  samples/       one still every SAMPLE ms of test time, on the desktop colour, named
                 by time (t01250.png); `kase_qa.py compare` diffs two runs of these
  filmstrip.png  the samples as a contact sheet with timestamps
  moments.png    the first and the settled frame of every presentation
  journey.mp4    real-speed video (frames held for exactly as long as they were shown)
  checks.json    the automatic rules and what broke them
  index.html     all of it on one page, with the timeline

Rules checked on every run (see `RULES`):
  min-display    a presentation is replaced by another before 1.2 s on screen
  edge-clip      something is drawn on the surface edge (the island got cut off)
  long-motion    the island keeps moving for more than 2 s at a stretch (rich layer
                 art excluded)
"""

from __future__ import annotations

import html
import json
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from PIL import Image

JOURNEYS = Path(__file__).resolve().parent / "journeys"
MIN_DISPLAY_MS = 1200
MAX_MOTION_MS = 2000
RULES = {
    "min-display": f"a presentation is replaced before {MIN_DISPLAY_MS} ms on screen",
    "edge-clip": "pixels drawn on the surface edge (the island is cut off)",
    "long-motion": f"the island moves for over {MAX_MOTION_MS} ms at a stretch",
}


def load_frames(frames_dir: Path) -> list[dict]:
    log = frames_dir / "frames.jsonl"
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines() if line.strip()]


def presentation(f: dict) -> str:
    """A short name for what the island shows."""
    shown = f.get("shown")
    if not f.get("mapped") or shown is None:
        return "hidden" if not f.get("mapped") else "folding"
    if shown.startswith("Alert("):
        kind = shown[len("Alert("):].split("(")[0].split(" ")[0].rstrip(")")
        return f"alert:{kind}"
    return shown.lower()


def segments(frames: list[dict]) -> list[dict]:
    """Runs of frames with the same presentation and headline."""
    segs: list[dict] = []
    for f in frames:
        what = presentation(f)
        key = (what, "" if what == "hidden" else f.get("headline", ""))
        if segs and segs[-1]["key"] == key:
            segs[-1]["frames"].append(f)
            continue
        segs.append({"key": key, "frames": [f]})
    for i, s in enumerate(segs):
        s["start"] = s["frames"][0]["t_ms"]
        s["end"] = segs[i + 1]["frames"][0]["t_ms"] if i + 1 < len(segs) else s["frames"][-1][
            "t_ms"]
        s["what"], s["headline"] = s["key"]
    return segs


def frame_image(frames_dir: Path, f: dict, surface="island") -> Image.Image | None:
    name = f.get("files", {}).get(surface)
    if not name:
        return None
    return Image.open(frames_dir / name).convert("RGBA")


def alpha_box(img: Image.Image):
    return img.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox()


def edge_touch(img: Image.Image) -> bool:
    a = img.getchannel("A")
    w, h = a.size
    strips = [a.crop((0, 0, w, 1)), a.crop((0, h - 1, w, h)), a.crop((0, 0, 1, h)),
              a.crop((w - 1, 0, w, h))]
    return any(s.getextrema()[1] > 24 for s in strips)


def check(frames: list[dict], frames_dir: Path) -> list[dict]:
    problems = []
    segs = segments(frames)
    for i, s in enumerate(segs[:-1]):
        nxt = segs[i + 1]
        visible = s["what"] not in ("hidden", "folding")
        replaced = nxt["what"] not in ("hidden", "folding")
        dur = s["end"] - s["start"]
        if visible and replaced and dur < MIN_DISPLAY_MS:
            problems.append({"rule": "min-display", "t_ms": s["start"],
                             "detail": f"{s['what']} '{s['headline']}' shown {dur:.0f} ms, "
                                       f"then {nxt['what']} '{nxt['headline']}'"})
    run_start = None
    for f in frames:
        moving = f.get("animating") and f.get("mapped") and not f.get("art")
        if moving and run_start is None:
            run_start = f["t_ms"]
        elif not moving and run_start is not None:
            if f["t_ms"] - run_start > MAX_MOTION_MS:
                problems.append({"rule": "long-motion", "t_ms": run_start,
                                 "detail": f"moving for {f['t_ms'] - run_start:.0f} ms"})
            run_start = None
    clipped = 0
    for f in frames:
        img = frame_image(frames_dir, f)
        if img is not None and edge_touch(img):
            clipped += 1
            if clipped == 1:
                problems.append({"rule": "edge-clip", "t_ms": f["t_ms"],
                                 "detail": f"frame {f['frame']} touches the surface edge"})
    return problems


def at_time(frames: list[dict], t: float) -> dict | None:
    """The frame on screen at test time t (the last one at or before it)."""
    best = None
    for f in frames:
        if f["t_ms"] <= t + 1e-6:
            best = f
        else:
            break
    return best


def on_bg(img: Image.Image | None, size, box, bg) -> Image.Image:
    canvas = Image.new("RGBA", size, bg + (255,))
    if img is not None:
        crop = img.crop(box)
        canvas.alpha_composite(crop, (0, 0))
    return canvas.convert("RGB")


def render(name: str, out: Path, sample_ms: int, bg, fps: int, contact_sheet, font) -> dict:
    frames_dir = out / "frames"
    frames = load_frames(frames_dir)
    if not frames:
        raise SystemExit(f"{name}: no frames recorded (see {out}/overlay.log)")
    boxes = []
    for f in frames:
        img = frame_image(frames_dir, f)
        if img is not None:
            b = alpha_box(img)
            if b:
                boxes.append(b)
    if boxes:
        pad = 24
        first = frame_image(frames_dir, next(f for f in frames if f.get("files", {}).get(
            "island")))
        W, H = first.size
        box = (max(min(b[0] for b in boxes) - pad, 0), max(min(b[1] for b in boxes) - pad, 0),
               min(max(b[2] for b in boxes) + pad, W), min(max(b[3] for b in boxes) + pad, H))
    else:
        box = (0, 0, 400, 120)
    size = (box[2] - box[0], box[3] - box[1])
    cache: dict[str, Image.Image] = {}

    def still(f: dict | None) -> Image.Image:
        key = (f or {}).get("files", {}).get("island", "")
        if key not in cache:
            cache[key] = on_bg(frame_image(frames_dir, f) if f else None, size, box, bg)
        return cache[key]

    samples = out / "samples"
    shutil.rmtree(samples, ignore_errors=True)
    samples.mkdir()
    end = frames[-1]["t_ms"]
    t, paths, labels = 0.0, [], []
    while t <= end + 1e-6:
        f = at_time(frames, t)
        p = samples / f"t{int(round(t)):05d}.png"
        still(f).save(p)
        paths.append(p)
        labels.append(f"{t / 1000:.2f} s  {presentation(f) if f else ''}")
        t += sample_ms
    contact_sheet(paths, cols=6, labels=labels, max_cell=700).save(out / "filmstrip.png")

    segs = segments(frames)
    mpaths, mlabels = [], []
    mdir = out / "moments"
    shutil.rmtree(mdir, ignore_errors=True)
    mdir.mkdir()
    for i, s in enumerate(segs):
        for tag, f in (("in", s["frames"][0]), ("settled", s["frames"][-1])):
            p = mdir / f"{i:02d}-{tag}.png"
            still(f).save(p)
            mpaths.append(p)
            mlabels.append(f"{f['t_ms'] / 1000:.2f} s {tag}: {s['what']}")
    contact_sheet(mpaths, cols=4, labels=mlabels, max_cell=700).save(out / "moments.png")

    video = None
    if shutil.which("ffmpeg"):
        tmpd = Path(tempfile.mkdtemp(prefix="kase-journey-"))
        lines = []
        for i, f in enumerate(frames):
            nxt = frames[i + 1]["t_ms"] if i + 1 < len(frames) else f["t_ms"] + 500
            p = tmpd / f"{i:05d}.png"
            img = still(f)
            # Even sizes for yuv420p.
            img = img.crop((0, 0, img.width - img.width % 2, img.height - img.height % 2))
            img.save(p)
            lines.append(f"file '{p}'\nduration {(nxt - f['t_ms']) / 1000:.6f}")
        lines.append(f"file '{tmpd / f'{len(frames) - 1:05d}.png'}'")
        (tmpd / "list.txt").write_text("\n".join(lines) + "\n")
        r = subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0",
                            "-i", str(tmpd / "list.txt"), "-vf", f"fps={fps}",
                            "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "18",
                            str(out / "journey.mp4")], capture_output=True, text=True)
        shutil.rmtree(tmpd, ignore_errors=True)
        if r.returncode == 0:
            video = "journey.mp4"
        else:
            print(f"  ffmpeg failed: {r.stderr.strip()[:300]}")

    problems = check(frames, frames_dir)
    (out / "checks.json").write_text(json.dumps(problems, indent=2))
    timeline = [{"from_ms": s["start"], "to_ms": s["end"], "shows": s["what"],
                 "headline": s["headline"]} for s in segs]
    (out / "timeline.json").write_text(json.dumps(timeline, indent=2))
    write_page(out, name, timeline, problems, video)
    return {"journey": name, "frames": len(frames), "problems": problems,
            "timeline": timeline}


def write_page(out: Path, name: str, timeline, problems, video):
    rows = "".join(
        f"<tr><td>{s['from_ms'] / 1000:.2f}</td><td>{s['to_ms'] / 1000:.2f}</td>"
        f"<td>{(s['to_ms'] - s['from_ms']) / 1000:.2f}</td><td>{html.escape(s['shows'])}</td>"
        f"<td>{html.escape(s['headline'])}</td></tr>" for s in timeline)
    probs = "".join(f"<li><b>{p['rule']}</b> at {p['t_ms'] / 1000:.2f} s: "
                    f"{html.escape(p['detail'])}</li>" for p in problems) or "<li>none</li>"
    vid = f"<video src='{video}' controls loop></video>" if video else ""
    (out / "index.html").write_text(
        "<!doctype html><meta charset=utf-8><title>" + html.escape(name) + "</title>"
        "<style>body{background:#16171a;color:#ddd;font:14px system-ui;margin:16px}"
        "img,video{max-width:100%}td,th{padding:2px 10px;text-align:left}"
        "</style><h1>" + html.escape(name) + "</h1>" + vid +
        "<h2>Timeline (s)</h2><table><tr><th>from</th><th>to</th><th>for</th><th>shows</th>"
        "<th>headline</th></tr>" + rows + "</table><h2>Checks</h2><ul>" + probs + "</ul>"
        "<h2>Moments</h2><img src='moments.png'><h2>Filmstrip</h2><img src='filmstrip.png'>")


def run(names: list[str], out_root: Path, sway_cls, bin_dir: Path, scale: float,
        step_ms: float, sample_ms: int, bg, fps: int, timeout: float, contact_sheet,
        font) -> list[dict]:
    scripts = sorted(JOURNEYS.glob("*.json"))
    if names:
        scripts = [s for s in scripts if any(s.stem.startswith(n) for n in names)]
    if not scripts:
        raise SystemExit("no journeys match")
    results = []
    for n, script in enumerate(scripts, 1):
        name = script.stem
        out = out_root / name
        shutil.rmtree(out, ignore_errors=True)
        (out / "frames").mkdir(parents=True)
        print(f"[{n}/{len(scripts)}] {name}", flush=True)
        tmp = Path(tempfile.mkdtemp(prefix="kase-journey-"))
        sway = sway_cls(scale, tmp)
        try:
            env = sway.env()
            env.update(KASE_TEST_STEP_MS=str(step_ms), KASE_SHOT_DIR=str(out / "frames"),
                       WGPU_BACKEND="vulkan", RUST_LOG="info")
            started = time.time()
            with open(out / "overlay.log", "w") as log:
                r = subprocess.run([str(bin_dir / "kase-overlay"), "--script", str(script)],
                                   env=env, stdout=log, stderr=subprocess.STDOUT,
                                   timeout=timeout)
            print(f"  recorded in {time.time() - started:.0f} s (exit {r.returncode})")
        finally:
            sway.close()
            shutil.rmtree(tmp, ignore_errors=True)
        res = render(name, out, sample_ms, bg, fps, contact_sheet, font)
        meta = json.loads(script.read_text())
        res["title"] = meta.get("title", name)
        for p in res["problems"]:
            print(f"  {p['rule']}: {p['detail']}")
        results.append(res)
    index = "".join(
        f"<li><a href='{r['journey']}/index.html'>{html.escape(r['journey'])}</a>: "
        f"{html.escape(r['title'])} ({len(r['problems'])} problems)</li>" for r in results)
    (out_root / "index.html").write_text(
        "<!doctype html><meta charset=utf-8><title>Kase journeys</title><style>body{"
        "background:#16171a;color:#ddd;font:15px system-ui;margin:16px}a{color:#8cf}</style>"
        "<h1>Kase journeys</h1><ul>" + index + "</ul>")
    (out_root / "summary.json").write_text(json.dumps(results, indent=2))
    return results
