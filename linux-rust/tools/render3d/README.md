# 3D renders

The AirPods 4 clips in `assets/anim/` are rendered from "AirPods 4" by Falah3D
(CC BY 4.0, credited in the README and the app) with Blender's Python module.

```bash
python3 -m venv ~/bpy && ~/bpy/bin/pip install bpy==5.2.3   # a few minutes
git fetch origin assets-3d-models && git worktree add /tmp/models origin/assets-3d-models
~/bpy/bin/python tools/render3d/shots.py /tmp/models/airpods4-falah3d/scene.gltf /tmp/r/spin-open spin-open --preview
~/bpy/bin/python tools/render3d/shots.py /tmp/models/airpods4-falah3d/scene.gltf /tmp/r/spin-open spin-open
python3 tools/render3d/encode.py /tmp/r/spin-open assets/anim/airpods4-spin-open
```

| File | What it does |
| --- | --- |
| `rig.py` | Imports the model and adds empties for the spin, the lid hinge and each pod (seated or out, plus an orbit), by node name |
| `springs.py` | Apple's spring formula for keyframes (same tokens as `src/ui/tokens.rs`) |
| `shots.py` | The clips: `spin-open`, `put-away`. 144 fps, motion blur; `--preview` is 220 px and quick |
| `encode.py` | Scales and places frames to line up with the case-open clip, writes WebP |

Final renders take about 7 to 10 s a frame at 440 px on a 4-core cloud machine (a
237-frame clip is about 35 minutes). Which moment plays which clip is in
`docs/JOURNEYS.md`; the player is `src/ui/anim.rs`.
