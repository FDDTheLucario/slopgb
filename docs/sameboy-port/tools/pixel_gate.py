"""Split failing pixel-reference rows into GEOMETRY misses and COLOUR-only ones.

`classify_pixel.py` answers "does SameBoy match the reference", but it compares
luminance RANKS, so it cannot see a wrong colour at a right position — a pixel
row's `sameboy` verdict is therefore weaker than a glyph row's. This tool asks
the complementary question about OUR frame: is the miss a shape difference, or
only a colour one?

A row whose raw mismatch is non-zero while its rank mismatch is zero differs
from the reference only in colour. In practice that means the deciding pixel
reads an UNWRITTEN CGB palette entry — power-on contents the reference asset
captured from one particular console and no emulator reproduces (see
`docs/hardware-state/ppu-render.md`, the class-F note). Those rows are not
chaseable; the geometry ones are.

Usage:
    cargo build -p slopgb-core --example dump_gambatte_frame
    DUMP=target/debug/examples/dump_gambatte_frame \\
        python3 pixel_gate.py rowlist.txt

`rowlist.txt` holds one `<rel> [Model]` key per line — the census's own key
format, so it can be fed straight from `floor-census.tsv`:

    awk -F'\\t' 'NR>1 && $6=="reference-png"{print $1}' floor-census.tsv
"""
import os
import subprocess
import sys

import numpy as np
from PIL import Image

H, W = 144, 160
LUMA = np.array([0.299, 0.587, 0.114])
ROOT = os.environ.get(
    'SLOPGB_GBTR_ROOT',
    os.path.join(
        os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))))),
        'test-roms', 'game-boy-test-roms-v7.0',
    ),
)
DUMP = os.environ.get('DUMP')
if not DUMP or not os.path.exists(DUMP):
    sys.exit("set DUMP= to the dump_gambatte_frame binary "
             "(cargo build -p slopgb-core --example dump_gambatte_frame)")


def our_frame(rom, model, scratch='/tmp/s7/pixgate.raw'):
    os.makedirs(os.path.dirname(scratch), exist_ok=True)
    subprocess.run([DUMP, rom, model, scratch], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    a = np.frombuffer(open(scratch, 'rb').read(), dtype='<u4').reshape(H, W)
    return np.dstack([(a >> 16) & 0xFF, (a >> 8) & 0xFF, a & 0xFF]).astype(np.uint8)


def gambatte_rgb(img):
    """gambatte's CGB-to-RGB lut, the frame the `_cgb04c.png` assets are in."""
    r5 = (img[:, :, 0] >> 3).astype(int)
    g5 = (img[:, :, 1] >> 3).astype(int)
    b5 = (img[:, :, 2] >> 3).astype(int)
    return np.dstack([(r5 * 13 + g5 * 2 + b5) // 2,
                      (g5 * 3 + b5) * 2,
                      (r5 * 3 + g5 * 2 + b5 * 11) // 2]).astype(np.uint8)


def rank(img):
    """Per-pixel luminance rank among the image's distinct colours — the same
    tint/gamma-invariant metric `classify_pixel.py` compares under."""
    cols, inv = np.unique(img.reshape(-1, 3), axis=0, return_inverse=True)
    order = np.argsort(cols.astype(float) @ LUMA, kind='stable')
    rk = np.empty(len(cols))
    denom = max(len(cols) - 1, 1)
    for r, ci in enumerate(order):
        rk[ci] = r / denom
    return rk[inv].reshape(H, W)


def reference(stem, model):
    tagged = f"{stem}_{'cgb04c' if model == 'Cgb' else 'dmg08'}.png"
    return tagged if os.path.exists(tagged) else f"{stem}.png"


def main():
    for line in open(sys.argv[1]):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        rel, _, model = line.rpartition(' [')
        model = model.rstrip(']')
        rom = os.path.join(ROOT, rel)
        png = reference(rom.rsplit('.', 1)[0], model)
        if not (os.path.exists(rom) and os.path.exists(png)):
            print(f"{rel:58s} [{model}] MISSING ASSET")
            continue
        ours = our_frame(rom, 'cgb' if model == 'Cgb' else 'dmg')
        if model == 'Cgb':
            ours = gambatte_rgb(ours)
        ref = np.array(Image.open(png).convert('RGB'))
        raw = int((((ours.astype(int) ^ ref.astype(int)) & 0xF8) != 0).any(axis=2).sum())
        geo = int((np.abs(rank(ours) - rank(ref)) > 0.5).sum())
        verdict = 'COLOUR-ONLY (unwritten palette entry — class F)' if raw and not geo \
            else ('match' if not raw else 'GEOMETRY (chaseable)')
        print(f"{rel:58s} [{model}] raw={raw:6d} geo={geo:6d}  {verdict}")


if __name__ == '__main__':
    main()
