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

## Notes

- Rendering is Mesa's software Vulkan (lavapipe), so colours and anti-aliasing match a real
  GPU closely but not bit for bit. Compare runs made on the same machine.
- Frame bursts grab as fast as the software renderer allows (roughly 10 to 20 frames a
  second), which is enough to judge layout and timing, not smoothness. Judge smoothness on
  real hardware.
- A shot marked "still moving at capture" had a looping animation (for example the charging
  light) and was taken after the timeout.
