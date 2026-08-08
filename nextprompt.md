slopgb — next task: keep differencing the PPU read frame against SameBoy

## Repo state (verified 2026-08-06)

`main` @ `e58d2df7`, clean tree, no open branch.

gbtr **221/221** with **319** baselined floor cases (349 at the start of this
run); mooneye **93/93** suite tests (439/439 rom×model); core lib **913**;
frontend **676**; clippy + fmt clean; `golden_fingerprint` recaptured after each
lift, every drift confined to the rows that moved.

`docs/hardware-state/floor-census.tsv` is current: 320 rows, **291** with a
SameBoy verdict — up from 78 at the start of the run (the CGB classifier was
writing where nobody read, `bb755f63`; the mooneye-protocol rows had no gate at
all, `f71dace0`). Chaseable = SameBoy PASS + we fail, **150 rows**:

| cluster | chaseable | | cluster | chaseable |
|---|---|---|---|---|
| `wilbertpol/acceptance` | 30 | | `window`, `lycEnable` | 6, 6 |
| `dma` | 15 | | `sprites`, `m2enable` | 4, 4 |
| `mealybug/ppu` | 8 | | `bgtilemap`, `bgtiledata` | 4, 4 |
| `lcd_offset` | 8 | | `age/stat-mode-window` | 4 |
| `enable_display` | 8 | | `age/oam`, `age/vram` | 4, 3 |
| `scx_during_m3` | 7 | | `speedchange`, `m1` | 3, 3 |
| `halt` | 7 | | rest | the tail |

## READ FIRST

- `docs/hardware-state/ppu-timing.md` § **"The FF41 read frame"** — the law this
  run landed (`flip - 5`, both models), its two surviving scopes (carried reads
  and post-STOP shifted frames, both re-measured) and how it was derived.
- The floor-class index header in `tests/gbtr/baselines/gambatte.txt`.
- `docs/sameboy-port/tools/README.md` — the SameBoy ground-truth rig.

## THE METHOD THAT WORKED (repeat it)

Do not sweep a constant. Difference the read against SameBoy:

1. `docs/sameboy-port/tools/build_sameboy_tracers.sh` (cached at
   `~/.cache/sbbuild`), then `SB_TRACE=1 sameboy_tester --cgb --length 4 <rom>`.
   `SBMODE` = visible mode change, `SBREAD ff41` = the read instant. **Difference
   `fp=` (absolute 8 MHz), never `cfl`/`dc`** — those reset per line and gave a
   1-dot phantom jitter here.
2. Anchor both sides on the same event (mode-3 entry works; the line-start event
   carries a 4-dot ambiguity). The anchor cancels out of the resulting
   inequality, so a wrong anchor is survivable but a mixed one is not.
3. Our side: `cargo run -p slopgb-core --example probe_statread -- <rom> <pc>`
   prints the value the read actually latched (register A), which is the only
   thing that matters — a post-step `debug_read(0xFF41)` is a DIFFERENT read one
   or two M-cycles later and disagreed with A on exactly the failing rows.
4. Find the read PC by `cmp -l` on the `_1`/`_2` ROM pair: they differ by one
   inserted NOP, so the read instruction moves by one byte between rungs.
5. A/B with `SLOPGB_GBTR_CENSUS=<file>` per variant and diff the two dumps per
   row; the suite's own pass/fail summary hides which rows traded.

gambatte builds the same way and is worth instrumenting for a second opinion
(`LCD::getStat`, `m0TimeOfCurrentLine`) but it is NOT the oracle: it passed
these rows with an m0 time 2 dots off SameBoy's edge and its own `cc + 2`
read lead cancelling it.

## NEW: 54 rows just became chaseable

The mooneye-protocol rows (`fib:` wants — wilbertpol + age) report in REGISTERS,
so no screen classifier could reach them and all 62 sat `unknown`.
`mooneyerun.c` + `classify_fib.py` now gate them (wired into `census.py`):
**54 of the 62 are SameBoy-PASS**, which is what took the census to 291
verdicts and the chaseable population to 150. Untouched ground, and the biggest
single block is 24 rows of `intr_2_mode0_timing_sprites_scx{1,2,3,4}_nops`
across six models plus 18 age rows (`oam-read/write`, `stat-mode*`,
`vram-read`, `spsw-mode0`).

The age ladders are the better half of that block: hardware-captured, no
cross-oracle trade, and SameBoy passes them. **They are now decoded** —
`docs/sameboy-port/tools/age_decode.py --all` names the failing rung of every
one (the table and how to read it: `docs/hardware-state/test-status.md`, "The
age ladders — decoded"). The headline: `vram-read-dmgC` [Dmg],
`vram-read-ncmBCE` [Cgb] and `oam-read-dmgC-cgbBC` [Dmg] share ONE fingerprint
(rungs 10/26/43/59 want `$FF`, get real data), several rows are down to 2-3 bad
rungs. What a rung VALUE means is still open, and the obvious reading (`$FF` =
blocked, so we unblock early) is refuted — our VRAM reads never return the
`01`/`09` the buffer holds, so it stores something computed. Decode the
measurement phase (the `$C600` fill is batched well after the reads) before
aiming a fix. Two edges are already eliminated by sweep: `eager_access_released`
6 → 8/10 is 0/−9 and the DMG VRAM lock 80 → 76 is 0/−11, both pinned two-sided
by mooneye `lcdon_timing-GS`, gbmicrotest and the gambatte access rows.

That gate has already paid once: it showed the DMG-family
`hblank_ly_scx_timing_variant_nops` legs were chaseable, which led to measuring
the polled read edge on DMG and collapsing the model split (+6).

Read the two caveats before acting on the rest: the wilbertpol `intr_2_mode0`
family is the documented cross-oracle swap (siding with gambatte forfeits
gbmicrotest rows — see the baseline header), and `census.py` now WARNS that
these rows are bucketed JUNK yet SameBoy passes them, which means they must
never be exempted. But "SameBoy passes it" is new information for all 54, and
the age rows in particular are hardware-captured expectations with no such
trade attached.

## Already differenced this session — do NOT re-sweep these

| family | verdict | evidence |
|---|---|---|
| `lcd_offset/*_m0stat_count_*` | **floor** — the per-offset brackets contradict at whole-dot resolution (`K > 12` vs `K <= 10`); swept `over` 4/6/8 = +0/−1, +2/−1, +2/−3 | ppu-timing.md "The shifted (post-STOP) frame's mode-0 edge" |
| `lcd_offset/*_ly_count_*` | **floor** — fails on LY, not STAT: we drop LY=153 at 153:4, SameBoy holds it 8 dots in. Widening: +6/−18 (drops hardware age rows) | ppu-timing.md "Line 153's LY hold in a shifted frame" |
| `halt/late_m0*_halt_m0stat_scx3_2b` [Dmg] | **floor** — the mirror line-wrap OAM back-date is +2/−61; rows at the same dot 452 want both answers (class H halt-wake phase) | ppu-timing.md "The DMG line-wrap OAM entry" |
| `enable_display/ly0_late_scx7_m3stat_scx1_2` | **open, localized** — a render-length row: our fine-scroll hunt latches a late SCX write one M-cycle earlier than SameBoy's on the LCD-enable line. A one-dot hunt-start delay is refuted (+0/−1) | ppu-render.md "the fine-scroll hunt latches one M-cycle early" |

## TASK

Best next lever: the **fine-scroll hunt latch position** above. It is measured,
localized to `render.rs`'s live position comparator, and SameBoy's threshold is
pinned to the M-cycle by a rung pair that reads at the same absolute instant.
It drives every line's mode-3 length, so it needs a full-corpus A/B — that is
the work, not the diagnosis.

**The pixel rows are re-gated — start from this list, not from the census's
`sameboy` column.** `classify_pixel.py` compares luminance RANKS, so it verifies
geometry, not colour, and reports PASS on rows SameBoy plainly misses.
`docs/sameboy-port/tools/pixel_gate.py` applies the same metric to OUR frame and
splits the 18 chaseable gambatte pixel rows:

- **class F, not chaseable (2)**: `scx_during_m3_spx2`, `bgtiledata_spx09_ds_4`
  — colour-only misses on an unwritten CGB palette entry (power-on garbage the
  reference asset captured; SameBoy misses them too).
- **genuine geometry (16)**, smallest first: the `bgtilemap_spx0{8,9}_ds_{1,4}`
  and `bgtiledata_spx09_ds_{1,3}` 8-px rows, `scx_attrib_during_m3_spx2_ds`
  (raw 16 / geo **2**), `scx_during_m3_spx2_ds` (geo 6), then the big ones.
  Two worth knowing: `window/on_screen/late_wx_ds_2` looks like 22880 px raw but
  is a **160 px** geometry core, and `scx_0360c0/scx_during_m3_ds_2` is the
  reverse — 160 raw but 11516 rank, so its shade STRUCTURE differs.

Note every remaining one is `_ds` except the two class-F rows, so the pixel vein
now runs into the same double-speed floor as everything else.

The pixel-reference vein just paid and is not exhausted: diff the frame per
row (`cargo run -p slopgb-core --example dump_gambatte_frame`, then compare
against the sibling `_cgb04c.png`) and read WHERE the pixels differ — the
`bgtilemap_spx09` rows put their whole error in one tile column, which named the
mechanism immediately. Still open in that family: `bgtiledata_spx*_ds`,
`bgtilemap_spx0{8,9}_ds`, `scx_during_m3` (7).

`dma`'s remaining 15 are the HDMA-seam / speed-switch families (class B), a
different mechanism from anything here.

Gate every row through both references before investing in it — a row SameBoy
also fails is class G and is not chaseable.

## The chaseable population, mapped (start here)

150 rows are SameBoy-PASS. Categorising them against the verdicts this run
recorded leaves only **19 single-speed rows in clusters with no verdict yet** —
everything else is either already scored as a floor or is `_ds` (class A, the
double-speed sub-dot floor, 50 rows):

| cluster | SS | ds | note |
|---|---|---|---|
| `mealybug/ppu` | **8** | 0 | hardware photos — the strongest oracle, and the family already moved this run (+4 from the map-delay fix) |
| `dma`, `enable_display`, `window`, `same-suite/apu` | 2 each | 1/6/3/0 | |
| `acid`, `lycEnable`, `scx_during_m3` | 1 each | 0/5/6 | |
| the rest | 0 | 27 | all `_ds` |

So **mealybug is the biggest tractable block left**: `m3_bgp_change_sprites`
[Cgb], `m3_lcdc_obj_en_change{,_variant}` [Dmg], `m3_lcdc_obj_size_change`
[Cgb], `m3_lcdc_obj_size_change_scx` [Cgb]/[Dmg],
`m3_lcdc_win_en_change_multiple_wx` [Dmg], `m3_obp0_change` [Cgb] — 32 to 678
px each, so use `pixel_gate.py` first (geometry vs colour) and then diff the
differing columns like the `bgtilemap_spx09` fix.

Everything else single-speed is one or two rows in a cluster whose siblings are
`_ds`, i.e. likely the same class-A floor seen from the single-speed side.

## The prefill sprite path — diagnosed, needs a port not a patch

Three rows implicate it (`scx_during_m3_spx2`, `m3_lcdc_obj_en_change{,_variant}`
[Dmg]). For the obj_en rows the cause is now traced: the sprite IS fetched
during the prefill walk and lands in the FIFO with colour 1, then OUR MIXER
drops it because the draw-time OBJ-enable gate reads low at the pop — the drop
dots match the missing pixels exactly.

Read out of SameBoy's source (`display.c:704-730`): it gates at the pop against
the LIVE LCDC, exactly as we do, but pops the OAM FIFO on EVERY pixel —
discarded and off-screen included — with `position_in_line` advancing through
the prefill. We hold `lx` and index the sprite FIFO by `slot = screen - lx`.
Both patches were measured and rejected: the shift in the discard branch is
0/0 (`discard == 0` on these lines), the shift through the prefill walk is
**0/−11**. Taking SameBoy's gate means taking its position accounting too —
a port of the prefill walk, scored against the whole sprite corpus, not a
local fix. Do not attempt it as a patch.

## Localized, one measurement from a fix

`miscmstatirq/lycstatwirq_trigger_ly00_10_50_1` **[Cgb]** (its [Dmg] leg is
fixed). The two rungs stage in the engine's two-phase view at line 0 dot **5**
(must not fire) and dot **9** (must fire), identical in every other field
(`pre_high=false`, `mfi=NONE`, `lyc_interrupt_line=true`). So a fresh LYC-source
enable is silent for ~4 dots after the line-0 LYC event and fires beyond it.
The raise path is still unidentified — the `k >= 4` continuity arm is REFUTED
(suppressing there, windows `dot < 12` and `< 16`, changes nothing). And the
family pulls both ways: `_1` over-fires while `lcdoffset3_2` /
`ds_lcdoffset1_2` under-fire, so no single window serves all three.

## Scored this run, floors confirmed (don't re-sweep)

| row/family | lever tried | score |
|---|---|---|
| `halt/m1int_ly_2` [Cgb] | the CGB `getLyReg` +1 window (6 dots / 4 dots / line-144-only) | +6/−49, +6/−47, +1/−9 |
| the age ladders | `eager_access_released` 6 → 8/10 | 0/−9 |
| the age ladders | DMG VRAM read lock dot 80 → 76 | 0/−11 |

`m1int_ly_2`'s ladder pins the dot exactly (reads at 448/452/456 wanting
90/91/91), so the law is not in doubt — rows at the SAME dot simply want both
answers, which is the class-H sub-dot signature.

## Best next target with its kernel already decoded

`dma/hdma_pc_7ffe` + `dma/late_gdma_pc_7ffe_1` put the `ldh (FF55),a` trigger at
`$7FFE`, so the CPU's next opcode fetch comes from `$8000` — the VRAM the
transfer is writing — and their `_2` sibling passes, bracketing what that fetch
sees. Trace the fetch, not the DMA length (`docs/hardware-state/dma.md`).

## Also differenced, no lift (measured, don't re-sweep)

`dma/hdma_late_m3halt_m2unhalt_ly_*` — the six-rung ladder misses only at rung 4
because our halt-deferred HBlank block retires ~36 dots earlier in the line than
SameBoy's, whose two adjacent reads straddle the line 2→3 boundary. Floor class
B; the numbers are in `docs/hardware-state/dma.md`.

**Rig note:** run `sameboy_tester` at `--length 4`. Shorter runs can cut its boot
ROM (`Boot ROM did not finish`), which looks exactly like a SameBoy failure and
manufactures false floors — it cost a wrong verdict here before the `.log` was
checked.

## Constraints

- **Zero regressions.** Growing a baseline is a regression.
- Verify in order: unit tests → the affected suite → full gbtr +
  `golden_fingerprint` → mooneye → frontend.
- Recapture golden with `SLOPGB_GOLDEN=capture` only after reading the drift
  list and confirming every drifted case is in the cluster you touched.
- Standing repo law: no new deps, no unsafe, files <1000 lines, SSH-signed
  commits (`export SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket`),
  `/rust-diff-review` per iteration.
- One `CARGO_TARGET_DIR` per concurrent run; never `pkill` a build sharing one.

## Last session

| commit | law | rows |
|---|---|---|
| `c007bdd5` | a polled CGB FF41 read sees mode 0 from `flip - 5`, not `flip - 3` | +10 |
| `bb755f63` | the CGB classifier now writes where `census.py` reads | +167 measured |
| `4a3f540b` | three `lcd_offset`/`enable_display` families differenced: two measured floors, one localized render bug | 0 |
| `27b63ab9` | the CGB line-start dispatch-ack widening applies except on line 0 (CGB decouples its line-0 emission to dot 4) | +4 |
| `d3daeaff` | a mid-mode-3 LCDC write reaches the fetcher's MAP-select bits one dot after its data-select bit, CGB single speed | +8 |
| `f71dace0` | the mooneye-protocol rows are gateable at last; 54 of 62 unknowns are SameBoy-PASS | census |
| (this commit) | the polled bare-line `flip - 5` read edge holds on DMG too — the model split was a misread | +6 |
