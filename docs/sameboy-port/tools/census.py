"""Build the floor census from a gbtr harness dump.

The gbtr battery is green only in the sense that every failure sits in a
known-failure baseline. This joins that baseline into one table that says, per
row, what we produce, what the ROM wants, what SameBoy produces, and how
trustworthy the want is — so the floor can be attacked by evidence instead of
by prose.

Input: the TSV written by `SLOPGB_GBTR_CENSUS=<path> cargo test -p slopgb-core
--test gbtr` (`suite \t key \t verdict \t detail`).

Output: `docs/hardware-state/floor-census.tsv`, one row per FAILING case:

    key suite cluster model ours want sameboy provenance bucket evidence

`sameboy` comes from the existing classifiers, which run SameBoy 1.0.2 and OCR
its framebuffer; rows no classifier covers stay `unknown`. `bucket` and
`evidence` are left blank here — adjudication is a human/Pan-Docs step, and a
machine-guessed verdict in those columns would be indistinguishable from a
cited one.

Usage: python3 census.py <dump.tsv> [-o docs/hardware-state/floor-census.tsv]
"""
import argparse
import os
import re
import subprocess
import sys
import tempfile

TOOLS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(TOOLS))

# Suites the battery actually ratchets. blargg and smallsuites label their rows
# per sub-suite (`blargg/oam_bug`, `smallsuites/rtc3test`), so the match is on
# the leading component. The harness dump also carries rows from harness.rs's
# own unit tests (suite "demo"): those are not cases and must not be counted
# into the floor.
SUITES = {
    'gambatte', 'wilbertpol', 'mealybug', 'gbmicrotest', 'age',
    'same-suite', 'acid', 'blargg', 'mooneye2022', 'smallsuites',
}

# --- provenance -------------------------------------------------------------
#
# How much a row's *expectation* is worth. Only `hw-*` rows are safe to treat
# as ground truth; `weak`/`defect` rows are candidates for exemption, never for
# chasing.

def provenance(rel, model, detail):
    """Where this row's expected value came from, most specific first."""
    if 'age-test-roms/' in rel:
        # age encodes the verified silicon in the stem: dmgC, cgbBCE, ncmBC.
        m = re.search(r'-(dmg[A-E]+|cgb[A-E]+|ncm[A-E]+)', rel)
        return f"hw-age:{m.group(1)}" if m else 'hw-age'
    if 'mealybug-tearoom-tests/' in rel:
        return 'hw-photo:mealybug-png'
    if rel.startswith('gambatte/'):
        # The captured unit is in the filename next to the expected value.
        if 'dmg08_cgb04c_out' in rel:
            return 'hw-capture:dmg08+cgb04c'
        if 'cgb04c_out' in rel and model == 'Cgb':
            return 'hw-capture:cgb04c'
        if 'dmg08_out' in rel and model == 'Dmg':
            return 'hw-capture:dmg08'
        if '.png' in detail or 'frame' in detail.lower():
            return 'hw-capture:png'
        return 'gambatte:untagged'
    if 'mooneye-test-suite-wilbertpol/' in rel:
        if 'madness' in rel:
            return 'defect:build-sensitive'
        return 'weak:wilbertpol-2016'
    if 'gbmicrotest/' in rel:
        if 'dma_basic' in rel:
            return 'defect:self-overwriting-result'
        return 'hw-verdict:gbmicrotest-hram'
    if 'oam_bug' in rel and '7-timing' in rel:
        return 'defect:single-build'
    if 'mts-' in rel or 'mooneye-test-suite/' in rel:
        return 'hw-mooneye'
    if rel.startswith(('dmg-acid2/', 'cgb-acid2/', 'cgb-acid-hell/')):
        return 'hw-capture:png'
    if rel.startswith('same-suite/'):
        # SameBoy's own suite: using SameBoy as the oracle here would be
        # circular, so these are adjudicated on upstream evidence only (the
        # NR43 LFSR tables upstream documents as revision-specific).
        return 'upstream:same-suite'
    return 'unknown'


# --- value extraction -------------------------------------------------------

_WANT_SHOWS = re.compile(r'want "([^"]*)", screen shows "([^"]*)"')
_PIXELS = re.compile(r'(\d+) pixel\(?s?\)? differ')
_REGS = re.compile(r'regs at breakpoint ([^,]*), want Fibonacci (\S+)')
_AUDIO = re.compile(r'expected (\w+), got (\w+ \w+) over')
_HRAM = re.compile(r'actual \$FF80=(\S+), expected \$FF81=(\S+)')


def ours_want(detail):
    """(ours, want) parsed out of a failure message, best effort.

    Four protocols produce the floor: gambatte's hex glyph screen, the
    frame-vs-PNG comparators, the Fibonacci register signature, and the
    audio activity checks.
    """
    m = _WANT_SHOWS.search(detail)
    if m:
        # The harness reports the whole 20-glyph screen but only compares the
        # leading `len(want)` digits, so trim to the compared span — otherwise
        # every row looks wrong by 16 trailing zeros it was never judged on.
        want, screen = m.group(1), m.group(2)
        return screen[:len(want)], want
    m = _PIXELS.search(detail)
    if m:
        return f"{m.group(1)}px-diff", 'reference-png'
    m = _REGS.search(detail)
    if m:
        return m.group(1).replace(' ', ','), f"fib:{m.group(2)}"
    m = _AUDIO.search(detail)
    if m:
        return m.group(2).replace(' ', '-'), m.group(1)
    m = _HRAM.search(detail)
    if m:
        # gbmicrotest reports its verdict in HRAM: $FF80 actual, $FF81 expected.
        return m.group(1), m.group(2)
    if ('no LD B,B' in detail or 'timeout' in detail.lower()
            or 'condition not reached' in detail):
        return 'timeout', 'protocol-exit'
    return '', ''


# --- SameBoy oracle ---------------------------------------------------------

def _run_classifier(script, rows, workdir, tag, suffixes):
    """Run one classifier over `rows`; return {rel: 'PASS'|'FAIL'|'unknown'}.

    The two OCR classifiers name their outputs `_bug` (SameBoy matched the
    ROM's want, so a row we fail there is our bug) / `_floor` (SameBoy missed
    it too); the pixel classifier calls the same two `_pass` / `_fail`. Both
    mean the same thing, so the caller passes the suffix map.
    """
    if not rows:
        return {}
    listfile = os.path.join(workdir, f'{tag}_rows.txt')
    with open(listfile, 'w') as f:
        f.write('\n'.join(rows) + '\n')
    pref = os.path.join(workdir, tag)
    try:
        subprocess.run(
            [sys.executable, os.path.join(TOOLS, script), listfile, pref],
            check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True,
        )
    except (subprocess.CalledProcessError, OSError) as e:
        # A classifier that cannot run must leave rows `unknown`, never
        # silently mark them SameBoy-FAIL — an unmeasured row would otherwise
        # look exemptible.
        print(f"  {tag}: classifier unavailable ({e}) — rows stay unknown",
              file=sys.stderr)
        return {}
    out = {}
    for suffix, verdict in suffixes:
        path = pref + suffix
        if not os.path.exists(path):
            continue
        for line in open(path):
            line = line.strip()
            if not line:
                continue
            f = line.split('\t')
            # The pixel classifier echoes the model back as a second field;
            # without it the Dmg and Cgb legs of one ROM would collide.
            m = re.match(r'\[(\w+)\]$', f[1]) if len(f) > 1 else None
            out[(f[0], m.group(1)) if m else f[0]] = verdict
    return out


# Suffix -> verdict, per classifier flavour.
_OCR_SUFFIXES = (('_bug.txt', 'PASS'), ('_floor.txt', 'FAIL'),
                 ('_unk.txt', 'unknown'))
_PIXEL_SUFFIXES = (('_pass.txt', 'PASS'), ('_fail.txt', 'FAIL'),
                   ('_unk.txt', 'unknown'))


def sameboy_verdicts(rows, workdir):
    """{(rel, model): verdict} for every row a classifier can measure."""
    cgb, dmg, pixel = [], [], []
    for rel, model, detail in rows:
        # The pixel classifier resolves reference PNGs for the gambatte and
        # mealybug trees; acid ships its references the same way.
        is_png = '.png' in detail or 'pixel(s) differ' in detail
        if is_png:
            pixel.append(f"{rel} [{model}]")
        elif model == 'Cgb' and 'cgb04c_out' in rel:
            cgb.append(rel)
        elif model == 'Dmg' and 'dmg08' in rel:
            dmg.append(rel)
    print(f"  SameBoy: {len(cgb)} cgb, {len(dmg)} dmg, {len(pixel)} pixel rows",
          file=sys.stderr)
    by_model = {
        'Cgb': _run_classifier('classify_cgb_regr.py', cgb, workdir, 'cgb',
                               _OCR_SUFFIXES),
        'Dmg': _run_classifier('classify_dmg.py', dmg, workdir, 'dmg',
                               _OCR_SUFFIXES),
    }
    pix = _run_classifier('classify_pixel.py', pixel, workdir, 'pixel',
                          _PIXEL_SUFFIXES)
    out = {}
    for rel, model, _ in rows:
        v = by_model.get(model, {}).get(rel) or pix.get((rel, model))
        out[(rel, model)] = v or 'unknown'
    return out


# --- adjudication -----------------------------------------------------------
#
# Clusters where two hardware-captured suites demand opposite dots, documented
# in the baseline headers. These can never be chased one-sidedly: satisfying
# either side regresses the other, so they stay baselined until the pair is
# re-derived jointly. Matched as substrings of the collection-relative path.
CONFLICT_MARKS = (
    # single-speed scx2/scx5 m2-dispatch-chained reads vs the gbmicrotest
    # hblank_int_scx*_if DMG-hardware FF0F races that pin the same dots
    ('gambatte/m2int_m0irq/', 'scx2'), ('gambatte/m2int_m0irq/', 'scx5'),
    ('gambatte/m2int_m3stat/', 'scx2'), ('gambatte/m2int_m3stat/', 'scx5'),
    ('gambatte/oam_access/', 'scx2'), ('gambatte/oam_access/', 'scx5'),
    ('gambatte/vram_m3/', 'scx2'), ('gambatte/vram_m3/', 'scx5'),
    # gambatte's hardware-captured bgtiledata/bgtilemap spx0B columns vs the
    # mealybug _cgb_c photos: a rising-late LCDC view fits one and breaks the
    # other (see docs/hardware-state/ppu-render.md).
    ('gambatte/bgtiledata/', 'spx0B'), ('gambatte/bgtilemap/', 'spx0B'),
)


def adjudicate(rel, prov, sb):
    """(bucket, evidence) for one row.

    Hardware provenance decides whether a row is worth chasing; SameBoy only
    decides whether fixing it ties us with SameBoy or puts us ahead of it.
    """
    for prefix, mark in CONFLICT_MARKS:
        if rel.startswith(prefix) and mark in rel:
            return 'CONFLICT', 'two hardware-captured suites pin opposite dots'
    if prov.startswith('defect:'):
        return 'JUNK', f'defective asset ({prov.split(":", 1)[1]})'
    if prov.startswith('weak:'):
        # 2016-era chains with no capture provenance, already contradicting
        # age (2022, CPU-CGB-04-verified) and gbmicrotest at the same dots.
        # No classifier reads their 0xED protocol, so SameBoy cannot vouch
        # either way — and a row nobody measured must not be dropped on
        # provenance alone. Exemptible only once Pan Docs is cited against the
        # specific expectation.
        if sb != 'FAIL':
            return 'JUNK', 'weak provenance, SameBoy unmeasured — NOT exemptible yet'
        return 'JUNK', 'no capture provenance; contradicts hardware-verified suites'
    if prov.startswith('upstream:'):
        return 'CONFLICT', 'upstream-documented revision/unit-specific behavior'
    if sb == 'PASS':
        return 'BUG', 'SameBoy reproduces the captured value; we do not'
    if sb == 'FAIL':
        return 'EXCEED', 'hardware-captured want that SameBoy also misses'
    return 'BUG', 'hardware-captured want; SameBoy verdict unmeasured'


# --- main -------------------------------------------------------------------

def parse_dump(path):
    """Failing cases from the harness dump as (suite, rel, model, detail)."""
    rows = []
    for line in open(path):
        parts = line.rstrip('\n').split('\t')
        if len(parts) < 3:
            continue
        suite, key, verdict = parts[0], parts[1], parts[2]
        detail = parts[3] if len(parts) > 3 else ''
        if verdict != 'fail' or suite.split('/')[0] not in SUITES:
            continue
        m = re.match(r'^(.*) \[(\w+)\]$', key)
        if not m:
            print(f"  unparseable key, skipped: {key}", file=sys.stderr)
            continue
        rows.append((suite, m.group(1), m.group(2), detail))
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dump')
    ap.add_argument('-o', '--out', default=os.path.join(
        REPO, 'docs', 'hardware-state', 'floor-census.tsv'))
    ap.add_argument('--no-sameboy', action='store_true',
                    help='skip the SameBoy oracle (every row stays unknown)')
    args = ap.parse_args()

    rows = parse_dump(args.dump)
    print(f"{len(rows)} failing cases in {args.dump}", file=sys.stderr)
    if not rows:
        sys.exit("no failing rows parsed — a census of nothing is not a bar")

    triples = [(rel, model, detail) for _, rel, model, detail in rows]
    if args.no_sameboy:
        sb = {}
    else:
        with tempfile.TemporaryDirectory(prefix='census_') as wd:
            sb = sameboy_verdicts(triples, wd)

    header = ['key', 'suite', 'cluster', 'model', 'ours', 'want', 'sameboy',
              'provenance', 'bucket', 'evidence']
    lines = ['\t'.join(header)]
    for suite, rel, model, detail in sorted(rows):
        ours, want = ours_want(detail)
        # Suites that already label per sub-suite (blargg, smallsuites) are
        # their own cluster; the flat ones cluster by the ROM's first dir,
        # which is what the baselines' prose groups by (`dma/`, `window/`, …).
        parts = rel.split('/')
        cluster = suite if '/' in suite else (
            f"{suite}/{parts[1]}" if len(parts) > 2 else suite)
        verdict = sb.get((rel, model), 'unknown')
        prov = provenance(rel, model, detail)
        bucket, evidence = adjudicate(rel, prov, verdict)
        lines.append('\t'.join([
            f"{rel} [{model}]", suite, cluster, model, ours, want,
            verdict, prov, bucket, evidence,
        ]))
    with open(args.out, 'w') as f:
        f.write('\n'.join(lines) + '\n')

    n_sb = sum(1 for v in sb.values() if v != 'unknown')
    print(f"wrote {args.out}: {len(rows)} rows, {n_sb} with a SameBoy verdict",
          file=sys.stderr)
    tally = {}
    for line in lines[1:]:
        tally[line.split('\t')[8]] = tally.get(line.split('\t')[8], 0) + 1
    for bucket in ('BUG', 'EXCEED', 'CONFLICT', 'JUNK'):
        print(f"  {bucket:9} {tally.get(bucket, 0)}", file=sys.stderr)
    # A JUNK row SameBoy passes would mean exempting a test we simply fail.
    bad = [l.split('\t')[0] for l in lines[1:]
           if l.split('\t')[8] == 'JUNK' and l.split('\t')[6] == 'PASS']
    if bad:
        sys.exit("JUNK rows that SameBoy PASSES (never exempt these):\n  "
                 + '\n  '.join(bad))


if __name__ == '__main__':
    main()
