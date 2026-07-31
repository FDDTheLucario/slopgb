#!/usr/bin/env python3
"""Which BG map columns does a reference PNG actually ACCEPT at each on-screen tile?

Renders every map column's 8-pixel signature from the ROM's own VRAM (via the
`mapdump` probe in ../reference/mapdump.rs) and matches the reference's 8-pixel
segments against them. Output per tile position is the SET of columns that would
reproduce the reference — which is what tells you whether a row DISCRIMINATES a
law, or is DEGENERATE (accepts everything) / UNRESOLVED (fine scroll splits a
tile across two columns, so whole-tile matching cannot read it).

  colreq.py <mapdump-binary> <rom> <dmg|cgb> <ly> [x-offsets, default 0,144,152]
"""
import os
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image


def gambatte_rgb(px):
    """gambatte's CGB->RGB conversion (tests/common/framecmp.rs)."""
    px = int(px)
    r5, g5, b5 = (px >> 19) & 31, (px >> 11) & 31, (px >> 3) & 31
    return ((((r5 * 13 + g5 * 2 + b5) // 2) << 16)
            | (((g5 * 3 + b5) * 2) << 8)
            | ((r5 * 3 + g5 * 2 + b5 * 11) // 2))


def dump(binary, rom, model):
    with tempfile.TemporaryDirectory() as t:
        out = subprocess.run([binary, rom, model, os.path.join(t, "f.raw")],
                             capture_output=True, text=True, check=True).stdout
    mp = attr = bgpal = None
    tiles = {}
    for line in out.splitlines():
        p = line.split()
        if p[0] == "MAP":
            mp = [int(x, 16) for x in p[1:]]
        elif p[0] == "ATTR":
            attr = [int(x, 16) for x in p[1:]]
        elif p[0] == "TILE":
            tiles[(int(p[1]), int(p[2][1:]))] = [int(x, 16) for x in p[3:]]
        elif p[0] == "BGPAL":
            bgpal = [int(x, 16) for x in p[1:]]
    return mp, attr, tiles, bgpal


def col_sig(c, row, mp, attr, tiles, bgpal, cgb):
    """The 8 on-screen pixels map column `c` would produce for tile-row `row`."""
    t, a = mp[c], attr[c] if cgb else 0
    data = tiles.get((t, 1 if a & 0x08 else 0))
    if data is None:
        return None
    w = data[7 - row if a & 0x40 else row]
    lo, hi = w >> 8, w & 0xFF
    sig = []
    for b in range(8):
        bit = b if a & 0x20 else 7 - b          # CGB attribute bit 5 = X flip
        ci = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1)
        pal = a & 7
        raw = bgpal[pal * 8 + ci * 2] | (bgpal[pal * 8 + ci * 2 + 1] << 8)
        r5, g5, b5 = raw & 31, (raw >> 5) & 31, (raw >> 10) & 31
        px = ((r5 << 3 | r5 >> 2) << 16) | ((g5 << 3 | g5 >> 2) << 8) | (b5 << 3 | b5 >> 2)
        sig.append(gambatte_rgb(px))
    return tuple(sig)


def ref_png(stem, model):
    order = ["_dmg08", ""] if model == "dmg" else ["_cgb04c", "_cgb", ""]
    for suf in order:
        if os.path.isfile(stem + suf + ".png"):
            return stem + suf + ".png"
    return None


def main():
    binary, rom, model, ly = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
    offs = [int(v) for v in sys.argv[5].split(",")] if len(sys.argv) > 5 else [0, 144, 152]
    mp, attr, tiles, bgpal = dump(binary, rom, model)
    png = ref_png(rom.rsplit(".", 1)[0], model)
    if png is None:
        sys.exit(f"no reference PNG beside {rom}")
    img = np.asarray(Image.open(png).convert("RGB")).astype(np.int64)
    want = ((img[:, :, 0] << 16) | (img[:, :, 1] << 8) | img[:, :, 2])[ly]
    sigs = {c: col_sig(c, ly & 7, mp, attr, tiles, bgpal, model == "cgb") for c in range(32)}
    print(f"{os.path.basename(rom)} [{model}] ly={ly}  ref={os.path.basename(png)}")
    for st in offs:
        seg = tuple(int(v) for v in want[st:st + 8])
        hits = [c for c in range(32) if sigs[c] == seg]
        if len(hits) >= 20:
            verdict = "DEGENERATE (accepts everything — constrains nothing)"
        elif not hits:
            verdict = "UNRESOLVED (fine scroll splits the tile; not 'no constraint')"
        else:
            verdict = f"DISCRIMINATING {hits}"
        print(f"  x{st}-{st + 7}: {verdict}")


main()
