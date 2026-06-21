# SameBoy parity — validated finding + roadmap (2026-06-21)

**Goal (user): pass every test SameBoy passes, no exceptions — cycle-accurate to SameBoy's level.**

## The finding that reverses the "irreducible floor" verdict

Built SameBoy's headless **tester** (`make tester` from the 1.0.2 source) and a gap-finder
(OCR SameBoy's framebuffer with the gambatte glyph font, compare to each ROM's `_outXX`). Result:

- **SameBoy PASSES ~420 of our ~839 baselined-failing gambatte rows** (we fail them). Families:
  `m2int_m3stat`, `m0int_m3stat`, `cgbpal_m3`, `scx_during_m3`, `vramw`, `postread/postwrite/preread`,
  `speedchange`, `sprite_late_enable`, `lcd_offset`, `dma/*_cycles`.
- Harness validated: of 60 rows **we** pass, SameBoy passes 56 (~7% frame/boot noise).
- **SameBoy passes BOTH `m2int_m3stat` (out3) AND mooneye `intr_2_mode0_timing`** simultaneously.

**Therefore the "class-A/B floor is a cross-oracle contradiction / irreducible" conclusion in
`ppu-subdot-ladder.md` is WRONG.** It is not a contradiction — it is our timing model being too coarse.
SameBoy resolves it with a **cycle-exact (T-cycle) timing model**. The half-dot/event-phase scaffold
(S0+S1, the `event_phase`/`lead_eighths` machinery) is a correct but insufficient approximation.

## The precise diagnosis (why our model fails, e.g. the kernel pair)

`m2int_m3stat_1` (wants out3) and `m0int_m3stat_2` (wants out0) BOTH read FF41 at the **identical dot
256** with identical PPU state in our model → we give both the same value (collapse). SameBoy
distinguishes them: on its T-cycle-exact grid the two reads land in **different M-cycles** (the mode-0
ISR read is one M-cycle later, after the mode-3→0 boundary → mode 0). The gap is the combination of:
1. **Read sampling phase** — SameBoy's deferred-commit (`sm83_cpu.c` `cycle_read`: sample-then-defer)
   samples the bus at the M-cycle LEADING edge (cc+0), 4 dots (1 in DS) before our tick-then-access cc+4
   end view. (Probed: a uniform leading-edge single-speed FF41 read lifts m2int=3 but regresses
   m0int→3 — necessary but not sufficient alone.)
2. **Mode-0 STAT-IRQ dispatch dot** — when the mode-0 IRQ fires (→ when the m0int ISR runs → its read
   M-cycle). Ours fires too early, collapsing m0int's read onto m2int's dot.
3. **Mode-boundary sub-dot position** (`display.c` exit stagger: STAT&3→0, VRAM/OAM unblock at X,
   palette re-block X+3, unblock X+5) and the per-read accessibility back-dating.

These interlock — fixing one without the others swaps rows (the empirically-measured +N/−N).

## The path = port SameBoy's cycle-exact timing core (the major rewrite)

Design workflow `wf_c6945378` (9 agents, both source trees) → **incremental-port, medium feasibility**.
Stages (each full-gbtr+golden+mooneye+mealybug gated, revert-on-regression; SameBoy gap-finder as the
targeting/progress metric):
- **Foundation**: deferred-commit CPU reads (sample-then-defer) + a decoupled cycle-exact
  `visible_mode`/`mode_for_interrupt` (`display.c:1782-1799`) + per-read accessibility back-dating.
  NOTE: this is NOT net-zero against our current boundaries — it requires shifting the boundary/dispatch
  dots to SameBoy's frame too, so the foundation + first boundary set land together (atomic-ish).
- **S2** accessibility read decouple (cgbpal/vramw/oam_access, ~37).
- **S3** unify single+double-speed STAT/access onto one back-dated model, RETIRING the
  `event_phase`/`lead_eighths`/`ACCESS_PHASE` edge stamps — MUST reproduce the existing INC-DS-1(+43) /
  task6(+84) trades.
- **S4** STAT-IRQ-line edge model (`GB_STAT_update`-equivalent rising-edge IF + the line-0 "OAM IRQ 1T
  before STAT" blip) — MUTATING (changes IF → frames); ~123 rows.
- **S5** residual write-timing (speedchange/hdma cycle rows) — port SameBoy's `cycle_write` conflict map;
  our `stage_write` is a hand-fit equivalent. SameBoy passes mealybug, so its write model is the correct
  re-derivation target (NOT unrecapturable — derive from SameBoy's source).

**Honest scope:** this is a multi-session re-architecture of the timing core to SameBoy's cycle-exact
level. It is FEASIBLE (SameBoy's model demonstrably passes everything incl. mealybug + mooneye), but it
is large and interlocking — not a row-by-row patch. The whole core (CPU bus timing + PPU mode/access/STAT
+ write timing) moves to one consistent cycle-exact frame.

## Tooling (rebuildable; was in /tmp this session)
- SameBoy tester: `cd <SameBoy-src> && make tester` → `build/bin/tester/sameboy_tester --cgb --length 10 <rom>` (dumps `<rom>.bmp`).
- `sb_ocr.py` (OCR a SameBoy BMP with the gambatte glyphs), `sb_gaps.py` (gap-finder over the baselines),
  `sb_gap_list.txt` (the 456-row gap list). Re-run after each stage; the gap count is the progress metric.
