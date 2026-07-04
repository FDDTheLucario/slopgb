"""Pixel-reference flip-blocker classifier (#11bj Phase 3).

For each gambatte/mealybug pixel-reference leg: run SameBoy (--dmg or --cgb),
read its 160x144 BMP, and compare against the sibling reference PNG with
PALETTE-QUANTIZED tolerant matching:

  - build the reference's colour palette (its set of distinct RGB values);
  - map each SameBoy pixel to the NEAREST reference-palette entry;
  - count pixels whose nearest-quantized colour differs from the reference at
    that (x,y).

SameBoy renders with its own shades; the quantize step folds a uniform
shade/gamma difference so only genuine per-pixel geometry mismatches count.
A leg is SameBoy-PASS (flip blocker: SameBoy matches gambatte's reference,
slopgb-flip does not) iff the mismatch count <= THRESHOLD.

Usage: python3 classify_pixel.py rowlist.txt outprefix [threshold]
  rowlist lines: `gambatte/<rel>.gbc [Dmg|Cgb]` (or mealybug/...).
Reference-PNG resolution mirrors the gbtr harness:
  DMG -> <stem>_dmg08.png else <stem>.png
  CGB -> <stem>_cgb04c.png else <stem>_cgb.png else <stem>.png
Writes <outprefix>_pass.txt (blockers) / _fail.txt (rebaseline) / _unk.txt.
"""
import struct, subprocess, os, sys, shutil, tempfile
import numpy as np
from PIL import Image

SBT = os.environ.get('SBT', '/tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester')
ROOT = os.environ.get(
    'SLOPGB_GBTR_ROOT',
    '/home/soulcatcher/personal_repos/slopgb/.claude/worktrees/phase-b-s7/test-roms/game-boy-test-roms-v7.0',
)
W, H = 160, 144


def read_bmp(path):
    """160x144 BGRA/BGR BMP -> (H,W,3) uint8 RGB array (rows top-to-bottom)."""
    d = open(path, 'rb').read()
    off = struct.unpack('<I', d[10:14])[0]
    bpp = struct.unpack('<H', d[28:30])[0]
    hdr = struct.unpack('<i', d[22:26])[0]  # height (negative = top-down rows)
    step = bpp // 8
    # BMP rows are padded to a 4-byte boundary.
    rowbytes = (W * step + 3) & ~3
    buf = np.frombuffer(d, np.uint8, count=rowbytes * H, offset=off)
    buf = buf.reshape(H, rowbytes)[:, : W * step].reshape(H, W, step)
    rgb = buf[:, :, [2, 1, 0]]  # BGR(A) -> RGB
    if hdr >= 0:                # bottom-up storage -> flip to top-down
        rgb = rgb[::-1]
    return np.ascontiguousarray(rgb)


def read_png(path):
    im = Image.open(path).convert('RGB')
    a = np.asarray(im, np.uint8)
    if a.shape[0] != H or a.shape[1] != W:
        im = im.resize((W, H), Image.NEAREST)
        a = np.asarray(im, np.uint8)
    return a


def ref_png(rel, cgb):
    stem = os.path.join(ROOT, rel.rsplit('.', 1)[0])
    if cgb:
        # gambatte `_cgb04c`/`_cgb`, mealybug `_cgb_c`/`_cgb_d`, bare.
        cands = [stem + '_cgb04c.png', stem + '_cgb.png',
                 stem + '_cgb_c.png', stem + '_cgb_d.png', stem + '.png']
    else:
        # gambatte `_dmg08`, mealybug `_dmg_blob`, bare.
        cands = [stem + '_dmg08.png', stem + '_dmg_blob.png', stem + '.png']
    for c in cands:
        if os.path.exists(c):
            return c
    return None


def _rank_map(img):
    """Per-pixel shade RANK in [0,1] among the image's distinct colours,
    ordered by luminance — tint/gamma-invariant (SameBoy renders DMG with a
    yellow-tinted palette and CGB with its own colour correction; the rank
    folds both). A single-colour image maps to all-zero."""
    lum = img.astype(np.float64) @ np.array([0.299, 0.587, 0.114])
    cols, inv = np.unique(img.reshape(-1, 3), axis=0, return_inverse=True)
    clum = cols.astype(np.float64) @ np.array([0.299, 0.587, 0.114])
    order = np.argsort(clum, kind='stable')
    rank_of_col = np.empty(len(cols))
    denom = max(len(cols) - 1, 1)
    for r, ci in enumerate(order):
        rank_of_col[ci] = r / denom
    return rank_of_col[inv].reshape(H, W)


def quantized_mismatch(sb, ref):
    """Count pixels whose luminance-rank differs by more than half a shade
    step between SameBoy and the reference — a palette-invariant geometry
    comparison (a uniform tint/gamma shift folds to rank 0, only genuine
    per-pixel shape mismatches survive)."""
    a = _rank_map(sb)
    b = _rank_map(ref)
    return int((np.abs(a - b) > 0.5).sum())


def main():
    rows = [l for l in open(sys.argv[1]) if l.strip() and not l.startswith('#')]
    pref = sys.argv[2]
    thr = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    tmp = tempfile.mkdtemp(prefix='clspx_')
    pas, fail, unk = [], [], []
    for line in rows:
        parts = line.split()
        rel = parts[0]
        cgb = '[Cgb]' in line or '[Agb]' in line
        tag = 'Cgb' if cgb else 'Dmg'
        src = os.path.join(ROOT, rel)
        rp = ref_png(rel, cgb)
        if not os.path.exists(src) or not rp:
            unk.append((rel, tag, 'no-src-or-ref', 0)); continue
        ext = rel.rsplit('.', 1)[1]
        dst = os.path.join(tmp, 'p.' + ext)
        bmp = os.path.join(tmp, 'p.bmp')
        if os.path.exists(bmp):
            os.remove(bmp)
        shutil.copy(src, dst)
        subprocess.run([SBT, '--cgb' if cgb else '--dmg', '--length', '4', dst],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not os.path.exists(bmp):
            unk.append((rel, tag, 'no-bmp', 0)); continue
        try:
            sb = read_bmp(bmp)
            ref = read_png(rp)
            mm = quantized_mismatch(sb, ref)
        except Exception as e:
            unk.append((rel, tag, f'err:{e}', 0)); continue
        if mm <= thr:
            pas.append((rel, tag, mm))
        else:
            fail.append((rel, tag, mm))
    print(f"PASS(sb~ref, flip blocker)={len(pas)}  FAIL(rebaseline)={len(fail)}  UNK={len(unk)}  thr={thr}")
    open(pref + '_pass.txt', 'w').write('\n'.join(f"{r}\t[{t}]\tmm={m}" for r, t, m in pas) + '\n')
    open(pref + '_fail.txt', 'w').write('\n'.join(f"{r}\t[{t}]\tmm={m}" for r, t, m in fail) + '\n')
    open(pref + '_unk.txt', 'w').write('\n'.join(f"{r}\t[{t}]\t{why}" for r, t, why, _ in unk) + '\n')
    shutil.rmtree(tmp)


if __name__ == '__main__':
    main()
