"""DMG variant of classify_cgb_regr.py (#11bi recipe, rebuilt #11bj).

Classifies [Dmg] gambatte OCR legs: runs SameBoy --dmg on each ROM, OCRs the
BMP with a per-tile x-shift {0,1} trial (DMG glyphs render +1px vs CGB), and
compares against the DMG want from the filename.

Want-regex, EXTENDED vs the #11bi recipe: explicit `dmg08_out<hex>` first,
then the shared-want form `dmg08_cgb04c_out<hex>` (the #11bi regex missed
those 35 window rows -> they were mis-bucketed as pixel legs).

Usage: python3 classify_dmg.py rowlist.txt outprefix
  rowlist lines: gambatte/<rel>.gbc (trailing ` [Dmg]`/want= columns ignored)
Writes <outprefix>_bug.txt / <outprefix>_floor.txt / <outprefix>_unk.txt.
"""
import struct, subprocess, os, re, shutil, sys, tempfile

RAW = {'0':[0x7F,0x41,0x41,0x41,0x41,0x41,0x7F],'1':[0x08,0x08,0x08,0x08,0x08,0x08,0x08],
 '2':[0x7F,0x01,0x01,0x7F,0x40,0x40,0x7F],'3':[0x7F,0x01,0x01,0x3F,0x01,0x01,0x7F],
 '4':[0x41,0x41,0x41,0x7F,0x01,0x01,0x01],'5':[0x7F,0x40,0x40,0x7E,0x01,0x01,0x7E],
 '6':[0x7F,0x40,0x40,0x7F,0x41,0x41,0x7F],'7':[0x7F,0x01,0x02,0x04,0x08,0x10,0x10],
 '8':[0x3E,0x41,0x41,0x3E,0x41,0x41,0x3E],'9':[0x7F,0x41,0x41,0x7F,0x01,0x01,0x7F],
 'A':[0x08,0x22,0x41,0x7F,0x41,0x41,0x41],'B':[0x7E,0x41,0x41,0x7E,0x41,0x41,0x7E],
 'C':[0x3E,0x41,0x40,0x40,0x40,0x41,0x3E],'D':[0x7E,0x41,0x41,0x41,0x41,0x41,0x7E],
 'E':[0x7F,0x40,0x40,0x7F,0x40,0x40,0x7F],'F':[0x7F,0x40,0x40,0x7F,0x40,0x40,0x40]}

def ocr(p, n):
    d = open(p, 'rb').read()
    off = struct.unpack('<I', d[10:14])[0]
    def px(x, y):
        i = off + (y * 160 + x) * 4
        return (d[i], d[i+1], d[i+2])
    out = ''
    for ti in range(n):
        ch = '?'
        for xs in (0, 1):  # DMG glyphs render +1px vs CGB
            bg = px(ti * 8 + xs, 0)
            rows = []
            for y in range(1, 8):
                b = 0
                for x in range(8):
                    xx = min(ti * 8 + x + xs, 159)
                    b = (b << 1) | (1 if px(xx, y) != bg else 0)
                rows.append(b)
            for c, g in RAW.items():
                if rows == g:
                    ch = c
                    break
            if ch != '?':
                break
        out += ch
    return out

def dmg_want(rel):
    m = re.search(r'dmg08_out([0-9A-Fa-f]+?)(?:_cgb|\.gb)', rel)
    if m:
        return m.group(1).upper()
    m = re.search(r'dmg08_cgb04c_out([0-9A-Fa-f]+)\.gb', rel)
    if m:
        return m.group(1).upper()
    return None

SBT = os.environ.get('SBT', '/tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester')
# Default to the collection in this checkout (this script lives in
# docs/sameboy-port/tools/). The old default pointed at a throwaway worktree; once
# that worktree was pruned every row silently classified as UNK, which reads as a
# clean bar rather than as a measurement that never ran.
_REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))))
ROOT = os.environ.get(
    'SLOPGB_GBTR_ROOT',
    os.path.join(_REPO, 'test-roms', 'game-boy-test-roms-v7.0'),
)
if not os.path.isdir(ROOT):
    sys.exit(f"ROM collection not found at {ROOT} — set SLOPGB_GBTR_ROOT. "
             "Classifying zero rows is a vacuous result, not a bar.")

def main():
    rows = [l.split()[0] for l in open(sys.argv[1]) if l.strip() and not l.startswith('#')]
    pref = sys.argv[2]
    tmp = tempfile.mkdtemp(prefix='clsdmg_')
    bug, floor, unk = [], [], []
    for rel in rows:
        want = dmg_want(rel)
        if not want:
            unk.append((rel, 'noregex', '')); continue
        src = os.path.join(ROOT, rel)
        if not os.path.exists(src):
            unk.append((rel, 'missing', want)); continue
        ext = rel.rsplit('.', 1)[1]
        dst = os.path.join(tmp, 'cls.' + ext)
        bmp = os.path.join(tmp, 'cls.bmp')
        if os.path.exists(bmp):
            os.remove(bmp)
        shutil.copy(src, dst)
        subprocess.run([SBT, '--dmg', '--length', '4', dst],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not os.path.exists(bmp):
            unk.append((rel, 'nobmp', want)); continue
        sb = ocr(bmp, len(want))
        if '?' in sb:
            unk.append((rel, sb, want)); continue
        if sb == want:
            bug.append(rel)
        else:
            floor.append((rel, sb, want))
    print(f"BUG(sb==want, must FIX)={len(bug)}  FLOOR/DIFF(sb!=want, baseline at flip)={len(floor)}  UNK={len(unk)}")
    open(pref + '_bug.txt', 'w').write('\n'.join(bug) + '\n')
    open(pref + '_floor.txt', 'w').write('\n'.join(f"{r}\tsb={s}\twant={w}" for r, s, w in floor) + '\n')
    open(pref + '_unk.txt', 'w').write('\n'.join(f"{r}\tsb={s}\twant={w}" for r, s, w in unk) + '\n')
    shutil.rmtree(tmp)

if __name__ == '__main__':
    main()
