"""Turns rendered PNG frames into the app's WebP clip frames.

  python3 encode.py IN_DIR OUT_DIR [--quality 90]

Frames are scaled and shifted so the case sits exactly where it does in the case-open
clip that shipped in 0.11 (assets/anim/airpods4-open: case 262 px wide, base at 413 px in
a 440 px square), so clips cut together. Measured from the open pose: the render's case
spans x 104..336 and ends at y 381 at 440 px.
"""

import argparse
import glob
import os

from PIL import Image

SIZE = 440
SCALE = 262 / 232
SHIFT = (89 - 104 * SCALE, 413 - 381 * SCALE)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("out")
    ap.add_argument("--quality", type=int, default=90)
    a = ap.parse_args()
    files = sorted(glob.glob(os.path.join(a.src, "f*.png")))
    os.makedirs(a.out, exist_ok=True)
    clipped, total = 0, 0
    for i, f in enumerate(files):
        im = Image.open(f).convert("RGBA")
        k = SCALE * SIZE / im.width
        big = im.resize((round(im.width * k), round(im.height * k)), Image.LANCZOS)
        frame = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
        frame.alpha_composite(big, (round(SHIFT[0]), round(SHIFT[1])))
        box = big.getchannel("A").getbbox()
        if box:
            x0, y0 = box[0] + SHIFT[0], box[1] + SHIFT[1]
            x1, y1 = box[2] + SHIFT[0], box[3] + SHIFT[1]
            if x0 < 0 or y0 < 0 or x1 > SIZE or y1 > SIZE:
                clipped += 1
        out = os.path.join(a.out, f"f{i:03d}.webp")
        frame.save(out, "WEBP", quality=a.quality, method=6)
        total += os.path.getsize(out)
    print(f"{len(files)} frames, {total / 1e6:.2f} MB, {clipped} clipped at the edge")


if __name__ == "__main__":
    main()
