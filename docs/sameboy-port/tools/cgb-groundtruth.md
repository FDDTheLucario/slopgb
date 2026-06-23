# C2 CGB ground-truth — the SameBoy-verdict path for CGB regressions

The goal flagged "303 CGB regr UNTRIAGED — `sb_ocr.py` returns empty on CGB
color". This closes that gap: a CGB-capable SameBoy OCR + the full triage of the
flip's CGB regressions into fix-vs-baseline. **No emulator-code change** — pure
measurement tooling, byte-identical.

## The CGB-OCR path (`/tmp/sb_ocr2.py`)

`sb_ocr.py` keyed on an absolute "dark" threshold, which fails on CGB where the
gambatte hex glyph + its background are arbitrary palette colors. `sb_ocr2.py` is
**palette-agnostic**: every gambatte glyph has an all-blank top row, so each
tile's `(0,0)` pixel is its background; a pixel is "on" iff it differs from that.
OCRs DMG identically and CGB correctly.

**CGB needs `--length 4`, not 2.** SameBoy's `--length` is wall-clock seconds
*including its boot ROM*; the CGB boot is ~2.5× the DMG's, so at `--length 2` the
gambatte result hasn't rendered yet (top row uniform → empty OCR). At `--length
4`/`6` it's stable. The triage script cross-checks 4-vs-6 and trusts only a
stable, non-empty read (→ `UNREADABLE` otherwise; measured 0 unreadable, so the
path is robust).

```sh
sameboy_tester --cgb --length 4 ROM.gbc   # writes ROM.bmp
python3 /tmp/sb_ocr2.py ROM.bmp           # prints the hex digits
```

## The triage (`/tmp/c2_cgb_groundtruth.py`, 2026-06-23 #11e)

Input: the slopgb-tier2 CGB fail rows (`gambatte_flagon_probe` on the CGB regr
list → `FAIL <rel> [Cgb] want=X got=Y`). For each, SameBoy `--cgb` (length 4∩6)
→ `sb`. Classify: **BUG** `sb==want` (slopgb-LE wrong → FIX via the port, NEVER
baseline), **AGREE** `sb==got` (SameBoy matches slopgb-tier2 → genuine floor →
baseline), **DIFF** (neither), **UNREADABLE**.

**Result (293 CGB rows): BUG = 248 · AGREE = 39 · DIFF = 6 · UNREADABLE = 0.**
~85% are real slopgb-LE bugs SameBoy passes (mirrors the DMG ~93%), confirming
the C2 mandate: do NOT baseline them — they need the port (the atomic read-frame
reclock / S6 DS / L2 sprites), not a baseline entry.

- **AGREE — the 39 genuine-floor CGB baseline candidates (C2):** m1(7),
  m0enable(7), lycEnable(7), window(5), lyc153int_m2irq(3), tima(2), serial(2),
  m2enable(2), oam_access(1), miscmstatirq(1), ly0(1), cgbpal_m3(1). Lists:
  `/tmp/c2cgb_agree.txt`. These are CGB rows where SameBoy ALSO fails the gambatte
  reference → baseline with a floor-class note (verify each against the
  gambatte.txt floor-class index before adding).
- **DIFF — 6, need manual review** (`/tmp/c2cgb_diff.txt`): `display_startstate/
  stat*_2` ×3 (SameBoy reads `80`, want `84`, slopgb-tier2 `87` — all differ, a
  post-enable mode-3 sub-value); `lcd_offset/*_lyc99int_m2irq_count` ×2 + `dma/
  hdma_late_disable_scx5_ds` ×1 (all double-speed — the S7 DS frame; the OCR
  reads a partial 2-digit count).
- **BUG — the 248 to FIX (not baseline):** sprites(87, render/L2), window(34),
  speedchange(13, S6 DS), lycEnable(13), halt(12), m2int_m3stat(9), vram_m3(8),
  oam_access(7), lcd_offset(7), enable_display(7), dma(6), m2enable(4), ly0(4), …
  Lists: `/tmp/c2cgb_bug.txt`. Same atomic-reclock / DS / sprite-geometry classes
  as the DMG side (the dispatch dots match SameBoy; the residual is the cc+0 read
  frame — see `stat-irq-trace.md`).

The DMG side's equivalent triage is `/tmp/c2_{agree,bug,diff}.txt` (the prior
`sb_ocr.py` DMG run). Together: C2 now has a SameBoy verdict for every flip
regression on both models — the baseline set is the AGREE rows (+ reviewed DIFF),
NOT the BUG rows. The actual rebaseline still waits on the S5 atomic reclock that
fixes the BUG rows (else the baseline would be regenerated twice).
