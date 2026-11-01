# Visual QA

`kase_qa.py` renders every screen and state of Kase without a monitor, so the UI can be
inspected pixel by pixel and every change checked before and after.

Kase is a native Wayland app (iced + wgpu), not a web page, so browser tools like
Playwright have nothing to open. The harness does the same job for a desktop app: it starts
a headless Sway compositor (no screen, software rendering), runs the real `kase` and
`kase-overlay` binaries with their built-in demo states, waits until each frame settles and
grabs it with `grim`. Nothing touches Bluetooth, the network, or your real settings: each
shot runs with its own temporary `HOME`.

## What it captures

| Group | Shots |
| --- | --- |
| Window | AirPods, Island and Settings tabs, dark and light, default height and full height |
| Device states | Pro, Max, low battery, disconnected, in case, put away, no AirPods, Bluetooth off |
| Apple device mode | confirm, working, done, failed |
| Island | all 10 demo states in Purist and Classic, 4 sizes, compact, light and busy desktops |
| Tray quick look | on dark and light desktops |
| Animations | frame bursts of the window connect (rings + 3D case) and the island case card |

Shots are taken at scale 2 (HiDPI), so 1 logical pixel is 2 image pixels. Each run writes
PNGs, `_sheet-window.png` / `_sheet-island.png` contact sheets, an `index.html` gallery and
a `manifest.json` (commit, arguments, whether each shot had settled).

## Use

```bash
# Ubuntu/Debian
sudo apt install sway swaybg grim mesa-vulkan-drivers dbus python3-pil
# Fedora / a Bazzite toolbox
sudo dnf install sway swaybg grim mesa-vulkan-drivers dbus-daemon python3-pillow

cargo build --release
tools/visual-qa/kase_qa.py capture --out target/visual-qa/before
# ...change the UI, rebuild...
tools/visual-qa/kase_qa.py capture --out target/visual-qa/after
tools/visual-qa/kase_qa.py compare target/visual-qa/before target/visual-qa/after
```

`compare` writes a before | after | diff image for every shot that changed (changed pixels
in pink) and `report.txt`, sorted by how much changed. Only a few shots: `--only
'window-airpods-*,quicklook'`. All names: `capture --list`.

`inspect` zooms into a region and draws a spacing grid in logical pixels (major lines every
4 steps), for checking alignment and padding:

```bash
tools/visual-qa/kase_qa.py inspect target/visual-qa/after/window-airpods-dark.png --grid 8 --zoom 2
```

## Journeys: frame-exact animation capture

`journey` plays scripted user journeys (putting the pods away, a low battery mid-game, the
iPhone taking the audio...) on the island and records **every frame**. It uses a test
clock inside the app: time only moves forward after each frame has been saved, so a slow
software renderer still captures all 120 frames of every second, and two runs give the
same frames. While nothing moves the clock jumps straight to the next event, so a 25 s
journey takes about 40 s to record.

```bash
tools/visual-qa/kase_qa.py journey                # all of them
tools/visual-qa/kase_qa.py journey j03 j06        # by name prefix
tools/visual-qa/kase_qa.py journey --list
tools/visual-qa/kase_qa.py compare target/visual-qa/journeys-before/j03/samples \
    target/visual-qa/journeys/j03/samples         # what changed, frame by frame
```

Per journey, under `target/visual-qa/journeys/<name>/`:

| Output | What it is |
| --- | --- |
| `index.html` | Video, timeline, checks, moments and filmstrip on one page |
| `journey.mp4` | Real-speed video (each frame held exactly as long as it was shown) |
| `filmstrip.png` | One still every 100 ms (`--sample-ms`) with timestamps |
| `moments.png` | The first and the settled frame of every presentation |
| `samples/` | The filmstrip stills, named by time, for `compare` |
| `frames/` | Every frame as drawn, plus `frames.jsonl` (time, what was shown, island size) |
| `timeline.json` | What the island showed, from when to when |
| `checks.json` | Broken rules (below) |

Rules checked on every frame: a presentation replaced by another in under 1.2 s
(`min-display`), the island drawn into the surface edge (`edge-clip`), and the island
moving for more than 2 s at a stretch, case art excluded (`long-motion`). `--strict` exits
1 when one is broken.

A journey is a JSON script in `journeys/`: a starting `state`, then `events` at test times,
each a patch over the state so far or a protocol `cmd`. The format is documented at the top
of `src/bin/kase-overlay/record.rs`. Timings in the scripts come from a real capture of
AirPods 4 packets (2026-11-01). The island first settles on the starting state, unseen, so
each journey starts from a calm island (`"start": "cold"` skips that).

The app side: `KASE_TEST_STEP_MS` turns the test clock on (`src/testclock.rs`) and
`KASE_SHOT_DIR` says where frames go; `kase-overlay --script FILE` plays a journey. Normal
runs never set them.

## Showcase: the README demos

`showcase.py` records the animated WebPs at the top of the main README (`docs/media/`):
the island playing the journeys in `showcase/`, the app window connecting and moving
between tabs (dark and light), and the 3D clips. Both apps run on the test clock at 60 fps
and save every frame themselves, so nothing is screen-recorded and every frame is exact.

```bash
tools/visual-qa/showcase.py                # all clips (a few minutes each, mostly encoding)
tools/visual-qa/showcase.py hero window    # by name prefix
tools/visual-qa/showcase.py --encode-only  # re-encode the last recording
```

The window recorder is `src/ui/window/record.rs`: with `KASE_TEST_STEP_MS` and
`KASE_SHOT_DIR` set, `kase --ui-demo` saves every frame and plays `KASE_UI_SCRIPT` (timed
tab, scenario and appearance changes) until `KASE_TEST_END_MS`.

## Notes

- Rendering is Mesa's software Vulkan (lavapipe), so colours and anti-aliasing match a real
  GPU closely but not bit for bit. Compare runs made on the same machine.
- Frame bursts grab as fast as the software renderer allows (roughly 10 to 20 frames a
  second), which is enough to judge layout and timing, not smoothness. Judge smoothness on
  real hardware.
- A shot marked "still moving at capture" had a looping animation (for example the charging
  light) and was taken after the timeout.
