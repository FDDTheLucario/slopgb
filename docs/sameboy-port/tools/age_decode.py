"""Decode WHICH rung of an age ladder fails, instead of reading its exit registers.

An age ROM ends in `LD B,B` with a Fibonacci signature on success and an opaque
one otherwise — every failing row in the census reports the same
`B=00 C=6B D=14 E=06 H=98 L=10`, which says nothing about the defect. But the
suite's shared checker compares a MEASURED buffer (WRAM `$C600+`) against an
EXPECTED table (ROM) one byte at a time:

    ld a,(bc) / inc bc / push bc / ld b,a / ld a,(de) / inc de / cp b / ld a,b

At the `cp b` the two values are live (A = expected, B = measured), so a probe
stopped there dumps the whole ladder. The routine is library code shared by the
suite: it sits at `$136F` in most builds and `$1382` in the `ncm` ones, and this
tool finds it by pattern rather than assuming either.

Usage (from the repo root):
    cargo build -p slopgb-core --example probe_statread
    python3 age_decode.py <rom.gb> <dmg|cgb> [frames]
    python3 age_decode.py --all          # every chaseable age row in the census
"""
import os
import re
import subprocess
import sys

PATTERN = bytes.fromhex('0A03C5471A13B878')
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))))
ROOT = os.environ.get('SLOPGB_GBTR_ROOT',
                      os.path.join(REPO, 'test-roms', 'game-boy-test-roms-v7.0'))
PROBE = os.environ.get(
    'PROBE', os.path.join(REPO, 'target', 'debug', 'examples', 'probe_statread'))
CENSUS = os.path.join(REPO, 'docs', 'hardware-state', 'floor-census.tsv')


def checker_pc(rom_bytes):
    i = rom_bytes.find(PATTERN)
    return None if i < 0 else i + 6      # the `cp b`


def decode(rom, model, frames=100):
    pc = checker_pc(open(rom, 'rb').read())
    if pc is None:
        return None, []
    out = subprocess.run([PROBE, rom, hex(pc), model, str(frames)],
                         text=True, stderr=subprocess.PIPE,
                         stdout=subprocess.DEVNULL).stderr
    rungs = []
    for line in out.splitlines():
        m = re.search(r'a=(\S\S) bc=(\S\S)', line)
        if m:
            rungs.append((m.group(1), m.group(2)))   # (expected, measured)
    return pc, rungs


def report(rel, model, rom):
    pc, rungs = decode(rom, 'dmg' if model == 'Dmg' else 'cgb')
    if pc is None:
        print(f"{rel:52s} [{model}] no checker found")
        return
    bad = [(i, e, m) for i, (e, m) in enumerate(rungs) if e != m]
    head = f"{rel:52s} [{model}] {len(rungs):3d} rungs, {len(bad):2d} bad"
    if not bad:
        print(head + "  (passes, or the ladder exited early)")
        return
    detail = ' '.join(f"#{i}:want{e}/got{m}" for i, e, m in bad[:6])
    print(f"{head}  {detail}{' …' if len(bad) > 6 else ''}")


def main():
    if not os.path.exists(PROBE):
        sys.exit(f"probe_statread not found at {PROBE} — cargo build -p slopgb-core "
                 "--example probe_statread, or set PROBE=")
    if sys.argv[1:2] == ['--all']:
        for line in open(CENSUS):
            f = line.rstrip('\n').split('\t')
            if len(f) < 7 or f[1] != 'age' or f[6] != 'PASS':
                continue
            rel, _, model = f[0].rpartition(' [')
            model = model.rstrip(']')
            rom = os.path.join(ROOT, rel)
            if os.path.exists(rom):
                report(rel.replace('age-test-roms/', ''), model, rom)
        return
    rom = sys.argv[1]
    model = sys.argv[2] if len(sys.argv) > 2 else 'dmg'
    report(os.path.basename(rom), 'Dmg' if model == 'dmg' else 'Cgb', rom)


if __name__ == '__main__':
    main()
