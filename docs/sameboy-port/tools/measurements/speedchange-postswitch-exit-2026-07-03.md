# #11bi — the POST-SWITCH bare-exit 4-VARIABLE TABLE, BUILT + SHIPPED (2026-07-03)

The #11bh item-7 park (`speedchange-postswitch-law-2026-07-03.md`) resolved.
The census-4 quartet (`speedchange2*_m2int_m3stat_scx2_2`) + 27 bonus legs fixed
in one flag-gated law; ZERO drops (family + lcd_offset/m2int_m3stat/dma guard
probes byte-identical; full two-bin below).

## Method

All **120 speedchange m3stat legs** dual-traced in one 6-agent fan-out
(`scratchpad/spd_measure2.sh` — one script, both emulators per leg):
slopgb `SLOPGB stop`/`SLOPGB leave` (new tracer: leave ly/dot/clk/dsa/dsa7/k)
/`SLOPGB ff41`/`SLOPGB visexit` ↔ SameBoy `SBSTOP/SBACK/SBREAD/SBMODE` (fp).
Parsed to one row per leg (`scratchpad/spd_table.tsv`, `spd_parse.py`); the law
fit OFFLINE against every leg's want as a constraint (want-3 ⇒ `rp < E`,
want-0 ⇒ `rp ≥ E`, rp = 2·read_dot): **120/120 satisfied, zero conflicts**.

## The table (the 4 variables collapse to 2 latches + scx)

`E = C + 2·(SCX&7)` in rp (2·dot) units, per class:

| speed at read | class | C | families pinned |
|---|---|---|---|
| SS | leave k=2, LCD ran through | **506** | speedchange2 base/frame1 (inert — emergent agrees), sc2-ly44-even (s∈{0,2}), speedchange4 (2 leaves) |
| SS | leave k=6, LCD ran through | **510** | speedchange2_nop m2int, sc2-ly44-odd (s=1) |
| SS | leave k=2, LCD re-enabled in DS | **502** | speedchange2 lcdoff/lcdoff_nopx2/nop_lcdoff[_nopx2] — the census quartet |
| SS | leave k=6, LCD re-enabled in DS | **506** | speedchange2 lcdoff_nop/nop_lcdoff_nop |
| DS | last leave k=2 (or never left) | **504** | speedchange v1 (enter-only), speedchange3 plain, speedchange5 |
| DS | last leave k=6 | **508** | speedchange3_nop |

Unified: **SS `C = 504 + leave_k − 4·[lcd_enable_in_ds]`; DS `C = 502 +
leave_k`** (leave_k ∈ {2,6} = the #11bd `sb_dsa8`-branched leave advance;
default 2 when never left). Two decisive negative results vs the park's
4-variable prediction:

- **ISR carry drops OUT**: the carried m2int reads and the polled ly44 reads
  land at identical rp with identical wants per class — one constant serves
  both (the park predicted a carry-dependent term).
- **The DS exit is LINEAR in scx** — no `+2·(SCX&1)` parity term (the parity
  form has NO consistent C across speedchange3 scx1/scx2; measured out).

## The scope discriminator (what the #11bh blanket lacked)

`Ppu::stop_anchor_midframe` — the FIRST switching STOP with the LCD enabled
since the last LCD enable, latched `line < 144`:

- **Mid-frame anchors (law fires)**: every speedchange dance (v1/2/3/4/5 first
  LCD-on STOP at ly68; the lcdoff variants' first LCD-on STOP is the STOP#2
  DECISION at ly0 dot12 — the DS re-enable resets the line counter, so the
  decision instant sits on line 0).
- **VBlank/boot anchors (law excluded — the emergent arm still serves them)**:
  kernel `m2int_m3stat_ds` (STOP ly144 dot240 clk76), lcd_offset offset1/2/3
  (first STOP ly144; offset1-ds's mid-frame ly113 RE-enter is not the FIRST),
  gdma_cycles (ly144) — all measured. The whole tier2 DS/SS suite is
  calibrated on this prologue frame (its constants absorb the switch error;
  #11bc/#11bd), so the law must not re-shift it. The #11bh blanket's 14
  SameBoy-pass drops were exactly VBlank-anchored classes.
- **Excluded by construction**: lcdoff2 (leave with LCD off —
  `note_switch_leave` checks the pause-end LCD), lcdoffds (`lcd_enable_in_ds`
  with no leave — sits exactly on the emergent DS exit, verified).

An LCD enable/disable clears the latches (the frame re-anchors; SameBoy
`double_speed_alignment = 0` at enable — the e-law).

## REPLACE, not fold (the second build iteration)

In scope the law REPLACES the emergent `2·flip + 2` exit for BOTH directions.
A fold cannot express it: `speedchange4_ly44_m3_nop_m3stat_scx3_2` reads
rp 512 native-0 with law exit 512 (→ 0, pass) while the emergent m==0 hold is
518 — fold-max keeps the over-hold, so the first (fold, m==3-only) build left
it failing. Family probe confirmed the replace build: **+31/−0** (26 remaining
family fails all pre-existing non-m3stat: tima/nr52/stat_N + the conceded
`speedchange2_nop_m2int_m3stat_scx1_1`, the VBlank-anchored pre-seeded #11bd
rebaseline joiner — out of scope by the anchor, unchanged).

## Where it lives

- `ppu/mod.rs`: `stop_anchor_set/midframe`, `stop_leave_lcd_on`,
  `stop_leave_k`, `lcd_enable_in_ds` + `note_switch_stop/leave` (tier2-only
  writers).
- `interconnect.rs stop()`: anchor latch at the STOP decision, leave latch
  after the k-advance; permanent `SLOPGB leave` tracer (S5DBG-gated).
- `ppu/regs.rs write_lcdc`: enable/disable clears + `lcd_enable_in_ds`.
- `ppu/stat_irq.rs vis_exit_hd` Arm 8: the two in-scope replacements.
- Pin: `tier2_speedchange_postswitch_exit_passes` (5 rows, probed 3×).

## Gates (this commit)

51 tier2 pins ×3 · mooneye 91/91 flag-on AND flag-off · lib 660 · clippy `-D`
clean · full gbtr OFF green (production byte-identical) · family probe
+31/−0 · lcd_offset/m2int_m3stat/dma guard probes byte-identical · full
two-bin: see below.

## Full two-bin (final tree)

ON **291** / OFF 486 (base 323 → 291: **32 fixed, 0 new** — name-level diff vs
`scratchpad/on_11bh_final3.txt`; ON list archived
`scratchpad/on_11bi{,_n}.txt`). The 32nd fix is out-of-family:
`oamdma/oamdma_late_speedchange_stat_1` (a mid-frame speed dance reading
FF41 — in law scope by construction). Full gbtr OFF 236/0 (the +1 = the new
pin). **CENSUS: 4 → 0. The C3 flip bar is met.**
