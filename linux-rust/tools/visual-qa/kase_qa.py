#!/usr/bin/env python3
"""Visual QA for Kase: render every screen and state headlessly, then compare runs.

Kase is a native Wayland app (iced + wgpu), so a browser tool like Playwright has nothing
to drive. Instead this runs the real binaries inside a headless Sway compositor (no
monitor, software rendering), feeds them the built-in demo states (`kase --ui-demo`,
`kase-overlay --demo`), and grabs pixel-exact screenshots with grim.

    kase_qa.py capture [--out DIR] [--only GLOB] [--scale 2]   # screenshots + sheets
    kase_qa.py compare BEFORE AFTER [--out DIR]                # before/after/diff report
    kase_qa.py inspect IMAGE [--grid 8] [--box x,y,w,h] [--zoom 4]

Needs: sway, grim, swaybg, a Vulkan driver (Mesa lavapipe is fine), dbus-run-session,
python3-pil, and release builds of `kase` and `kase-overlay` (cargo build --release).
See README.md next to this file.
"""

from __future__ import annotations

import argparse
import fnmatch
import glob
import html
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]  # linux-rust/
BIN = ROOT / "target" / "release"

# Desktop backgrounds the screens are shot on. The island floats over anything, so it
# is also shot on a light desktop and a busy one.
BG_DARK = (58, 66, 80)
BG_LIGHT = (226, 230, 236)


# --------------------------------------------------------------------------- scenarios


@dataclass
class Shot:
    name: str
    group: str
    argv: list[str]
    env: dict[str, str] = field(default_factory=dict)
    settings: dict = field(default_factory=dict)
    bg: str = "dark"
    # Frame burst for animations: seconds to record from launch (0 = a settled still).
    burst: float = 0.0
    # Seconds to wait before looking for a settled frame.
    wait: float = 3.5
    note: str = ""


def window(name, scenario="airpods4", tab="AirPods", theme="dark", height=None, burst=0.0,
           env=None, note=""):
    e = {"KASE_UI_TAB": tab}
    if height:
        e["KASE_UI_HEIGHT"] = str(height)
    e.update(env or {})
    return Shot(
        name=name,
        group="window",
        argv=[str(BIN / "kase"), f"--ui-demo={scenario}", "--no-tray"],
        env=e,
        settings={"appearance": theme},
        bg="light" if theme == "light" else "dark",
        burst=burst,
        note=note,
    )


OVERLAY_STEPS = [
    "normal", "case-charging", "pod-out", "low", "stale-case",
    "all-charging", "elsewhere", "max", "in-case", "ear-out",
]


def overlay(name, step=0, style="purist", size="medium", extra=(), bg="dark", burst=0.0,
            note="", wait=3.5):
    argv = [str(BIN / "kase-overlay"), "--demo", "--step", str(step), "--style", style,
            "--size", size, *extra]
    return Shot(name=name, group="island", argv=argv, bg=bg, burst=burst, note=note,
                wait=wait)


def scenarios() -> list[Shot]:
    s: list[Shot] = []
    # App window: every tab in both themes, at the default size and full height.
    for theme in ("dark", "light"):
        for tab in ("AirPods", "Island", "Settings"):
            s.append(window(f"window-{tab.lower()}-{theme}", tab=tab, theme=theme))
            s.append(window(f"window-{tab.lower()}-{theme}-full", tab=tab, theme=theme,
                            height=1000))
    # AirPods tab for every device state.
    for sc in ("pro", "max", "low", "disconnected", "incase", "putaway", "empty", "btoff"):
        s.append(window(f"window-state-{sc}", scenario=sc))
    s.append(window("window-state-pro-light", scenario="pro", theme="light"))
    # Apple device mode steps.
    for step in ("confirm", "working", "done", "failed"):
        s.append(window(f"window-apple-{step}", tab="Settings", height=1000,
                        env={"KASE_UI_APPLE": step}))
    # Island: every demo state in both styles.
    for style in ("purist", "classic"):
        for i, st in enumerate(OVERLAY_STEPS):
            s.append(overlay(f"island-{style}-{i}-{st}", step=i, style=style))
    # Sizes, compact, light and busy desktops, tray quick look.
    for size in ("small", "medium", "large", "extra_large"):
        s.append(overlay(f"island-size-{size}", size=size))
    s.append(overlay("island-compact", extra=["--demo-compact"],
                     note="compact rest state after the first expand", wait=8.0))
    s.append(overlay("island-on-light", bg="light"))
    s.append(overlay("island-on-busy", bg="busy"))
    s.append(overlay("quicklook", extra=["--quick-look"]))
    s.append(overlay("quicklook-light-desktop", extra=["--quick-look"], bg="light"))
    # Animations: frame bursts from launch.
    s.append(window("anim-window-connect", burst=2.6,
                    note="rings fill + 3D case opens on the AirPods tab"))
    s.append(overlay("anim-island-card", step=8, burst=3.0,
                     note="island case card with the 3D case opening"))
    return s


# --------------------------------------------------------------------------- compositor


class Sway:
    """A headless Sway with one 2560x1600 output (1280x800 logical at scale 2)."""

    def __init__(self, scale: float, tmp: Path):
        self.tmp = tmp
        self.runtime = tmp / "xdg"
        self.runtime.mkdir(mode=0o700)
        w, h = int(1280 * scale), int(800 * scale)
        self.size = (w, h)
        self.scale = scale
        self.cfg = tmp / "sway.cfg"
        self.cfg.write_text(
            f"output HEADLESS-1 mode {w}x{h} scale {scale} bg #3a4250 solid_color\n"
            "default_border none\ndefault_floating_border none\n"
            'for_window [app_id=".*"] floating enable, move position center\n'
            "seat * hide_cursor 1\n"
        )
        env = dict(os.environ)
        env.update(
            XDG_RUNTIME_DIR=str(self.runtime),
            WLR_BACKENDS="headless",
            WLR_RENDERER="pixman",
            WLR_LIBINPUT_NO_DEVICES="1",
        )
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("DISPLAY", None)
        self.log = open(tmp / "sway.log", "w")
        self.proc = subprocess.Popen(["sway", "-c", str(self.cfg)], env=env,
                                     stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(100):
            socks = list(self.runtime.glob("wayland-*"))
            ipc = list(self.runtime.glob("sway-ipc.*.sock"))
            if ipc and any(not p.name.endswith(".lock") for p in socks):
                break
            time.sleep(0.05)
        else:
            raise SystemExit("sway did not start; see " + str(tmp / "sway.log"))
        self.display = next(p.name for p in socks if not p.name.endswith(".lock"))
        self.ipc = str(ipc[0])
        self.bg_proc = None

    def env(self) -> dict[str, str]:
        e = dict(os.environ)
        e.update(XDG_RUNTIME_DIR=str(self.runtime), WAYLAND_DISPLAY=self.display,
                 SWAYSOCK=self.ipc, XDG_SESSION_TYPE="wayland",
                 XDG_CURRENT_DESKTOP="KDE")
        e.pop("DISPLAY", None)
        return e

    def set_background(self, kind: str, path: Path) -> tuple[int, int, int] | None:
        """Solid backgrounds return their colour (used to crop); busy returns None."""
        if self.bg_proc:
            self.bg_proc.terminate()
            self.bg_proc.wait()
            self.bg_proc = None
        if kind == "busy":
            busy_wallpaper(self.size).save(path)
            self.bg_proc = subprocess.Popen(["swaybg", "-o", "*", "-i", str(path), "-m",
                                             "fill"], env=self.env(),
                                            stdout=subprocess.DEVNULL,
                                            stderr=subprocess.DEVNULL)
            time.sleep(0.6)
            return None
        rgb = BG_LIGHT if kind == "light" else BG_DARK
        subprocess.run(["swaymsg", "-s", self.ipc, "output", "*", "bg",
                        "#%02x%02x%02x" % rgb, "solid_color"], env=self.env(),
                       stdout=subprocess.DEVNULL, check=False)
        time.sleep(0.2)
        return rgb

    def grab(self, path: Path) -> Image.Image:
        subprocess.run(["grim", str(path)], env=self.env(), check=True)
        return Image.open(path).convert("RGBA")

    def close(self):
        if self.bg_proc:
            self.bg_proc.terminate()
        self.proc.terminate()
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def busy_wallpaper(size) -> Image.Image:
    """A colourful, high-contrast stand-in for a game frame."""
    w, h = size
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / h
        d.line([(0, y), (w, y)], fill=(int(40 + 180 * t), int(120 + 100 * (1 - t)), 200))
    step = max(w // 16, 1)
    for i in range(0, w, step):
        d.rectangle([i, 0, i + step // 3, h], fill=(250, 250, 240))
    d.ellipse([w * 0.35, -h * 0.2, w * 0.65, h * 0.25], fill=(20, 20, 24))
    return img


# --------------------------------------------------------------------------- capture


def content_bbox(img: Image.Image, bg, pad: int):
    if bg is None:
        return None
    solid = Image.new("RGBA", img.size, bg + (255,))
    diff = ImageChops.difference(img, solid).convert("L").point(lambda v: 255 if v > 6 else 0)
    box = diff.getbbox()
    if not box:
        return None
    x0, y0, x1, y1 = box
    return (max(x0 - pad, 0), max(y0 - pad, 0), min(x1 + pad, img.width),
            min(y1 + pad, img.height))


def same(a: Image.Image, b: Image.Image) -> bool:
    return ImageChops.difference(a, b).getbbox() is None


def run_shot(sway: Sway, shot: Shot, out: Path, tmp: Path, timeout: float) -> dict:
    bg = sway.set_background(shot.bg, tmp / "busy.png")
    home = tmp / ("home-" + shot.name)
    cfg = home / ".config" / "kase"
    cfg.mkdir(parents=True)
    (cfg / "app_settings.json").write_text(json.dumps(shot.settings))
    env = sway.env()
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(home / ".local/share"),
               XDG_STATE_HOME=str(home / ".local/state"), RUST_LOG="error")
    env.setdefault("WGPU_BACKEND", "vulkan")
    env.setdefault("KASE_ANIM_DIR", str(ROOT / "assets/anim/airpods4-open"))
    env.update(shot.env)
    log = open(tmp / f"{shot.name}.log", "w")
    proc = subprocess.Popen(["dbus-run-session", "--", *shot.argv], env=env,
                            stdout=log, stderr=subprocess.STDOUT,
                            stdin=subprocess.PIPE, start_new_session=True)
    t0 = time.monotonic()
    meta = {"name": shot.name, "group": shot.group, "note": shot.note,
            "argv": [Path(shot.argv[0]).name, *shot.argv[1:]], "env": shot.env,
            "settings": shot.settings, "bg": shot.bg}
    try:
        if shot.burst:
            frames = []
            raw = tmp / "burst"
            raw.mkdir(exist_ok=True)
            i = 0
            while time.monotonic() - t0 < shot.burst:
                p = raw / f"{shot.name}-{i:03d}.png"
                t = time.monotonic() - t0
                img = sway.grab(p)
                # Start the strip at the first frame with something on screen.
                if frames or content_bbox(img, bg, 0) is not None:
                    frames.append((t, img))
                i += 1
            box = union_box([content_bbox(f, bg, 24) for _, f in frames])
            fdir = out / shot.name
            fdir.mkdir(parents=True, exist_ok=True)
            names = []
            for k, (t, f) in enumerate(frames):
                fn = f"{k:03d}-{int(t * 1000):05d}ms.png"
                (f.crop(box) if box else f).save(fdir / fn)
                names.append(fn)
            sheet = contact_sheet([fdir / n for n in names], cols=6,
                                  labels=[n.split("-")[1][:-4] for n in names])
            sheet.save(out / f"{shot.name}.png")
            meta.update(file=f"{shot.name}.png", frames=[f"{shot.name}/{n}" for n in names])
        else:
            # Entrance animations (ring fill, digit roll, 3D case) run ~2.5 s after the
            # first frame, so wait at least that long, then for 3 identical grabs.
            time.sleep(shot.wait)
            prev = sway.grab(tmp / "a.png")
            settled, still = False, 0
            while time.monotonic() - t0 < shot.wait + timeout:
                time.sleep(0.3)
                cur = sway.grab(tmp / "b.png")
                if same(prev, cur) and (bg is None or content_bbox(cur, bg, 0) is not None):
                    still += 1
                    if still >= 2:
                        settled = True
                        break
                else:
                    still = 0
                prev = cur
            box = content_bbox(prev, bg, 24)
            (prev.crop(box) if box else prev).save(out / f"{shot.name}.png")
            meta.update(file=f"{shot.name}.png", settled=settled,
                        crop=box, elapsed=round(time.monotonic() - t0, 2))
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
            proc.wait(3)
        except Exception:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except Exception:
                pass
        log.close()
        time.sleep(0.25)
    return meta


def union_box(boxes):
    boxes = [b for b in boxes if b]
    if not boxes:
        return None
    return (min(b[0] for b in boxes), min(b[1] for b in boxes),
            max(b[2] for b in boxes), max(b[3] for b in boxes))


def font(size=22):
    for f in ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
              "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


def contact_sheet(paths, cols=4, labels=None, cell_bg=(30, 30, 34), max_cell=None):
    imgs = [Image.open(p).convert("RGBA") for p in paths]
    if not imgs:
        return Image.new("RGB", (10, 10))
    cw = max(i.width for i in imgs)
    ch = max(i.height for i in imgs)
    if max_cell and cw > max_cell:
        k = max_cell / cw
        imgs = [i.resize((int(i.width * k), int(i.height * k)), Image.LANCZOS) for i in imgs]
        cw, ch = max(i.width for i in imgs), max(i.height for i in imgs)
    lh = 34
    rows = (len(imgs) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cw, rows * (ch + lh)), cell_bg)
    d = ImageDraw.Draw(sheet)
    f = font(20)
    for k, im in enumerate(imgs):
        x, y = (k % cols) * cw, (k // cols) * (ch + lh)
        sheet.paste(im, (x + (cw - im.width) // 2, y + lh), im)
        label = labels[k] if labels else Path(paths[k]).stem
        d.text((x + 8, y + 6), label, fill=(220, 220, 225), font=f)
    return sheet


def write_gallery(out: Path, metas: list[dict], title: str):
    rows = []
    for m in metas:
        note = f"<p class=n>{html.escape(m.get('note', ''))}</p>" if m.get("note") else ""
        warn = "" if m.get("settled", True) else " <b>(still moving at capture)</b>"
        rows.append(
            f"<figure id='{m['name']}'><a href='{m['file']}'><img src='{m['file']}' "
            f"loading=lazy></a><figcaption>{html.escape(m['name'])}{warn}</figcaption>"
            f"{note}</figure>")
    (out / "index.html").write_text(
        "<!doctype html><meta charset=utf-8><title>" + html.escape(title) + "</title>"
        "<style>body{background:#16171a;color:#ddd;font:14px system-ui;margin:16px}"
        "figure{display:inline-block;vertical-align:top;margin:8px;max-width:640px}"
        "img{max-width:100%;background:#2a2c31}figcaption{font-weight:600}"
        ".n{color:#999;margin:2px 0}</style><h1>" + html.escape(title) + "</h1>"
        + "".join(rows))


def cmd_capture(a):
    for b in ("kase", "kase-overlay"):
        if not (BIN / b).exists():
            raise SystemExit(f"missing {BIN / b}: run `cargo build --release` first")
    for tool in ("sway", "grim", "swaybg", "dbus-run-session"):
        if not shutil.which(tool):
            raise SystemExit(f"missing tool: {tool} (see tools/visual-qa/README.md)")
    shots = [s for s in scenarios()
             if not a.only or any(fnmatch.fnmatch(s.name, g) for g in a.only.split(","))]
    out = Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="kase-qa-"))
    sway = Sway(a.scale, tmp)
    metas = []
    try:
        for n, shot in enumerate(shots, 1):
            print(f"[{n}/{len(shots)}] {shot.name}", flush=True)
            try:
                metas.append(run_shot(sway, shot, out, tmp, a.timeout))
            except Exception as e:  # keep going; one broken state shouldn't stop the run
                print(f"  failed: {e}", file=sys.stderr)
    finally:
        sway.close()
    commit = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"],
                            capture_output=True, text=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", str(ROOT), "status", "--porcelain", "src"],
                           capture_output=True, text=True).stdout.strip()
    manifest = {"commit": commit + ("+dirty" if dirty else ""), "scale": a.scale,
                "time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "shots": metas}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2))
    for group in sorted({m["group"] for m in metas}):
        files = [out / m["file"] for m in metas
                 if m["group"] == group and not m["name"].startswith("anim-")]
        if files:
            contact_sheet(files, cols=4 if group == "island" else 6, max_cell=900).save(
                out / f"_sheet-{group}.png")
    write_gallery(out, metas, f"Kase visual QA {manifest['commit']}")
    if not a.keep:
        shutil.rmtree(tmp, ignore_errors=True)
    print(f"done: {len(metas)} shots in {out}")


# --------------------------------------------------------------------------- compare


def diff_image(a: Image.Image, b: Image.Image):
    w, h = max(a.width, b.width), max(a.height, b.height)
    ca = Image.new("RGBA", (w, h), (0, 0, 0, 255))
    cb = ca.copy()
    ca.paste(a, (0, 0))
    cb.paste(b, (0, 0))
    d = ImageChops.difference(ca.convert("RGB"), cb.convert("RGB")).convert("L")
    mask = d.point(lambda v: 255 if v > 10 else 0)
    changed = sum(1 for v in mask.getdata() if v) / (w * h)
    heat = Image.composite(Image.new("RGB", (w, h), (255, 40, 120)),
                           cb.convert("RGB").point(lambda v: v // 3), mask)
    return heat, changed, a.size != b.size


def cmd_compare(a):
    before, after = Path(a.before), Path(a.after)
    out = Path(a.out or (after / "_compare"))
    out.mkdir(parents=True, exist_ok=True)
    names = sorted({p.name for p in before.glob("*.png")} & {p.name for p in after.glob("*.png")})
    names = [n for n in names if not n.startswith("_")]
    only_b = sorted({p.name for p in before.glob("*.png")} - {p.name for p in after.glob("*.png")})
    only_a = sorted({p.name for p in after.glob("*.png")} - {p.name for p in before.glob("*.png")})
    rows = []
    for n in names:
        ia, ib = Image.open(before / n).convert("RGBA"), Image.open(after / n).convert("RGBA")
        heat, changed, resized = diff_image(ia, ib)
        if changed == 0 and not resized:
            continue
        f = font(22)
        w = ia.width + ib.width + heat.width
        h = max(ia.height, ib.height) + 36
        side = Image.new("RGB", (w, h), (24, 24, 28))
        d = ImageDraw.Draw(side)
        x = 0
        for label, im in (("before", ia), ("after", ib), ("diff", heat)):
            side.paste(im, (x, 36), im if im.mode == "RGBA" else None)
            d.text((x + 8, 6), label, fill=(230, 230, 230), font=f)
            x += im.width
        side.save(out / n)
        rows.append((changed, n, resized))
    rows.sort(reverse=True)
    lines = [f"{c * 100:6.2f}%  {n}{'  (size changed)' if r else ''}" for c, n, r in rows]
    report = "\n".join(lines) or "no visual changes"
    if only_a:
        report += "\nnew: " + ", ".join(only_a)
    if only_b:
        report += "\nremoved: " + ", ".join(only_b)
    (out / "report.txt").write_text(report + "\n")
    write_gallery(out, [{"name": n, "file": n, "note": f"{c * 100:.2f}% of pixels changed"}
                        for c, n, _ in rows], "Kase visual diff")
    print(report)


# --------------------------------------------------------------------------- inspect


def cmd_inspect(a):
    img = Image.open(a.image).convert("RGBA")
    if a.box:
        x, y, w, h = (int(v) for v in a.box.split(","))
        img = img.crop((x, y, x + w, y + h))
    if a.zoom > 1:
        img = img.resize((img.width * a.zoom, img.height * a.zoom), Image.NEAREST)
    d = ImageDraw.Draw(img)
    if a.grid:
        step = a.grid * a.scale * a.zoom
        for x in range(0, img.width, step):
            major = (x // step) % 4 == 0
            d.line([(x, 0), (x, img.height)],
                   fill=(0, 255, 255, 110 if major else 45))
        for y in range(0, img.height, step):
            major = (y // step) % 4 == 0
            d.line([(0, y), (img.width, y)],
                   fill=(0, 255, 255, 110 if major else 45))
    out = a.out or str(Path(a.image).with_suffix("")) + "-inspect.png"
    img.save(out)
    print(out)


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("capture", help="render every state to PNGs")
    c.add_argument("--out", default=str(ROOT / "target" / "visual-qa" / "latest"))
    c.add_argument("--only", help="comma-separated name globs, e.g. 'window-*,quicklook'")
    c.add_argument("--scale", type=float, default=2.0, help="output scale (2 = HiDPI)")
    c.add_argument("--timeout", type=float, default=9.0, help="max seconds to settle")
    c.add_argument("--keep", action="store_true", help="keep the temp dir (logs)")
    c.add_argument("--list", action="store_true", help="list scenario names and exit")
    m = sub.add_parser("compare", help="before/after/diff of two capture runs")
    m.add_argument("before")
    m.add_argument("after")
    m.add_argument("--out")
    i = sub.add_parser("inspect", help="zoom a region and draw a spacing grid")
    i.add_argument("image")
    i.add_argument("--grid", type=int, default=8, help="grid step in logical px (0 = off)")
    i.add_argument("--scale", type=int, default=2, help="capture scale")
    i.add_argument("--box", help="crop x,y,w,h in image pixels")
    i.add_argument("--zoom", type=int, default=1)
    i.add_argument("--out")
    a = p.parse_args()
    if a.cmd == "capture" and a.list:
        for s in scenarios():
            print(s.name)
        return
    {"capture": cmd_capture, "compare": cmd_compare, "inspect": cmd_inspect}[a.cmd](a)


if __name__ == "__main__":
    main()
