"""Ground-truth mooneye-protocol rows (the `fib:` wants) against SameBoy.

The glyph and pixel classifiers read the screen; these ROMs report in
REGISTERS at `LD B,B`, so neither can reach them and every such row sat
`unknown` in the census. `mooneyerun.c` runs one headless and prints
B,C,D,E,H,L; pass <=> 3,5,8,13,21,34.

Same contract as the other classifiers: `rowlist.txt outprefix`, writing
`<outprefix>_bug/_floor/_unk.txt` (bug = SameBoy passes a row we fail, so it is
ours to fix). Build the runner first — see the README — and point `MOONEYERUN=`
at it (default `/tmp/s7/mooneyerun`).
"""
import os
import re
import subprocess
import sys

RUN = os.environ.get('MOONEYERUN', '/tmp/s7/mooneyerun')
_REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))))
ROOT = os.environ.get('SLOPGB_GBTR_ROOT',
                      os.path.join(_REPO, 'test-roms', 'game-boy-test-roms-v7.0'))
# The runner resolves its boot ROMs relative to the CWD, like hramdump.c.
SBDIR = os.environ.get('SBBUILD', os.path.expanduser('~/.cache/sbbuild'))
CWD = os.path.join(SBDIR, 'SameBoy-1.0.2')

FLAG = {'Dmg': '--dmg', 'Mgb': '--mgb', 'Cgb': '--cgb',
        'Agb': '--agb', 'Sgb': '--sgb', 'Sgb2': '--sgb2'}


def main():
    if not os.path.exists(RUN):
        sys.exit(f"mooneyerun not found at {RUN} — build it (see the README) or set "
                 "MOONEYERUN=. Classifying with a missing runner is a vacuous "
                 "result, not a bar.")
    rows = [l.strip() for l in open(sys.argv[1]) if l.strip()]
    pref = sys.argv[2] if len(sys.argv) > 2 else '/tmp/s7/fib'
    bug, floor, unk = [], [], []
    for line in rows:
        rel, _, model = line.rpartition(' [')
        model = model.rstrip(']')
        rom = os.path.join(ROOT, rel)
        if model not in FLAG or not os.path.exists(rom):
            unk.append((line, '', ''))
            continue
        out = subprocess.run([RUN, FLAG[model], rom], cwd=CWD, text=True,
                             stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             stdout=subprocess.PIPE).stdout
        m = re.search(r'B=(\S+) C=(\S+) D=(\S+) E=(\S+) H=(\S+) L=(\S+) (PASS|NOPASS)', out)
        if not m:
            unk.append((line, '', ''))
        elif m.group(7) == 'PASS':
            bug.append(line)
        else:
            floor.append((line, '/'.join(m.groups()[:6]), 'fib'))
    print(f"BUG(sb passes, must FIX)={len(bug)}  FLOOR(sb fails too)={len(floor)}  UNK={len(unk)}")
    open(pref + '_bug.txt', 'w').write('\n'.join(bug) + '\n')
    open(pref + '_floor.txt', 'w').write(
        '\n'.join(f"{r}\tsb={s}\twant={w}" for r, s, w in floor) + '\n')
    open(pref + '_unk.txt', 'w').write(
        '\n'.join(f"{r}\tsb={s}\twant={w}" for r, s, w in unk) + '\n')


if __name__ == '__main__':
    main()
