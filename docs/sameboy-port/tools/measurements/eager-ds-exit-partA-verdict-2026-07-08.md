# EAGER DS mode-3 exit (HALFDOT Part A) — the "odd exit" premise is REFUTED; the half-dot lives in the READ, not the exit (2026-07-08, #11cf)

Task (HALFDOT Part A, redirected by #11ce): make the eager DS mode-3 EXIT
half-dot-precise so `flip_dot`/`win_predraw_abort_dot` can land ODD
(`2·flip+1`), separating the `_ds_1`/`_ds_2` pairs. Two mechanisms proposed:
(1) a targeted exit computation sourcing the odd bit from `SCX&1`/the DS 2-dot
grid, or (2) a per-half-dot render advance stamping `flip_dot` at the true
half-dot.

**Result: NEITHER ships. EV CGB two-bin UNCHANGED (421 → 421); DS blockers
cleared = 0. The tree is byte-identical to `25060d1` (golden_fingerprint
`--release` PASS). This map is the whole deliverable — a MEASURED REFRAME of the
lever: the "odd exit" premise is directly refuted by SameBoy's own trace, so
neither an exit stamp (mechanism 1) nor a render-side flip half-dot (mechanism
2) is the separator. The half-dot the pairs need lives in the eager READ's
M-cycle alignment to the PPU dot grid — which #11ce already proved is
irrecoverable from any whole-dot-derived state.**

## The refutation: SameBoy's DS mode boundaries are ALL even-dc (whole-dot)

`SBMODE` on `m0int_m0stat_scx5_ds_2` (`--cgb`, `SB_TRACE=1`), the `dc` =
`display_cycles` 8 MHz half-dot remainder at every mode transition:

| dc | count | parity |
|---:|---:|---:|
| 2 | 40653 | **0 (even)** |
| 4 | 15942 | **0** |
| 6 | 25664 | **0** |
| 8 | 52728 | **0** |

**EVERY mode transition (entry, exit, m0→m2) lands on an EVEN dc = a WHOLE
dot.** This is structural, not incidental: SameBoy runs the PPU as a divisor-2
coroutine (`GB_display_run`, display.c:1615 — `GB_SLEEP(…,N)` costs `N·2`
half-dots), so PPU logic is authored in **4 MHz whole dots** and every event it
raises is on an even half-dot. **SameBoy's DS mode-3 exit is NEVER at an odd
half-dot** — so the task's core lever ("make the EXIT land odd", mechanism 1's
`SCX&1` odd bit AND mechanism 2's odd `flip_dot`) contradicts the physics being
ported. An odd exit is not what separates the pairs.

## Where the DS pair discriminator actually lives — the READ, whole-dot too

The `_ds_1`/`_ds_2` pairs differ by **ONE byte of read-code placement** (asm
diff is a single `.text@` line: `scx_m3_extend_ds` `108c`/`108d`;
`m2int_m0stat_ds` `10da`/`10db`) = **one filler NOP = one M-cycle = 2 PPU dots
in DS**, NOT one half-dot. So the two legs' reads sit **2 dots (4 hd) apart on
the even grid** (`_1` @ `2D`, `_2` @ `2D+4`), straddling the **even** exit at
`2D+2`. Both the reads and the exit are whole-dot — the separation is a
whole-dot problem, and it is inert to every whole-dot lever swept here:

| lever | scope | Δ EV CGB fail | note |
|---|---|---:|---|
| control | — | **421** | `25060d1` |
| read-position `sb_dsa8`/paid-debt term | `eager_value && ds` | **0** | inert (#11ce, proven — `sb_dsa8 & 1 ≡ 0`) |
| line-start mode-2 back-date `dot≥2 → dot≥1` (DS) | `eager_value && ds` | **0** | fail SET md5-identical; `m2int_m0stat_ds_2` still all-zeros |

The line-start bump was the sharpest test: the eager pc-gated trace shows
`m2int_m0stat_ds_2`'s handler read at `10db` landing at `ly=136 dot=1` (native
mode 0, want 2); making `dot≥1` return mode 2 leaves the OCR **unchanged** (still
`got=0`). So the OCR-determining read is **not** the read the pc-gate isolates —
a tooling gap (below) — and the naive whole-dot fix does not reach the verdict.

## Why whole-dot levers are inert though everything is whole-dot

Two independent whole-dot mismatches, neither a single-lever fix:

- **m0→m2 LINE-START boundary (the bulk: `m2int_m0stat_ds`, `m0int_m0stat_ds`,
  `halt/*_m0stat_ds`; want 0 vs 2).** SameBoy's DS mode-2 begins at `dc=8`
  (=dot 4); slopgb's native window matches (mode 0 for dots 0-3). The eager
  cc+0 read is back-dated by a `+2`-dot DS read-debt to emulate the cc+4 view,
  and THAT mapping is where the leg splits — but the debt lever is inert on the
  OCR because the pc-gated read ≠ the stored read.
- **RENDER-LENGTH gap under continuous mid-mode-3 writes (`scx_m3_extend_ds`,
  want 3).** The read lands at `ly=1 dot≈327` (`SCX=0xEE`); slopgb's render
  exits mode 3 at **dot 259** while SameBoy extends to **327+** (a ~68-dot gap —
  the `sub a,b`-driven SCX chain re-arms the fine-scroll hunt every write, which
  slopgb's whole-dot hunt under-tracks). This is a render-model gap, NOT a
  half-dot: no exit stamp closes 68 dots.

## The tooling gap (why no clean fix could be VALIDATED)

The OCR-critical read (whose FF41 value reaches screen tile 0 via
`ld(9800),a`) could not be reliably pinned. Two independent probes disagree:
- pc-gated (`last_pc ∈ {108c/108d, 10da/10db}`) → isolates a read whose verdict
  (mode 0/2) does NOT match the value stored to `0x9800`.
- store-correlated (`CRIT` thread-local latched on each eager DS FF41 read,
  printed at the `0x9800` write) → the "last read before store" lands at
  `ly=143 dot=455` with a value that also mismatches the OCR (`value=02` while
  the OCR digit is `0`).
The tests loop/re-run across frames; the OCR captures at `RUN_DOTS + 1 frame`,
and neither probe isolates that exact frame's read. **A reliable Part-A attack
needs a frame-anchored OCR-read probe first** (latch the read on the OCR-capture
frame only, gated on `gb.cycles()` reaching the probe target).

## Verdict (single, precise)

The eager DS mode-3-exit blockers do **not** yield to a targeted exit stamp
(mechanism 1) or a render-side flip half-dot (mechanism 2): SameBoy's DS exit is
**even-dc (whole-dot)**, so an odd exit is the wrong lever. The pairs split
whole-dot (2 dots / 1 NOP apart) on the even grid, and the residual is (a) the
eager read-position→sample-dot **mapping** on the m0→m2 line-start boundary and
(b) a **render-length model gap** under continuous mid-mode-3 SCX writes —
neither a half-dot phenomenon. The genuinely-odd DS read (a CPU M-cycle
misaligned to the PPU dot grid via `double_speed_alignment`) exists only on
**speed-switch-misaligned** ROMs (lcd_offset/speedchange DS) and is
irrecoverable from `sb_dsa8` (always even, #11ce) — it needs the eager PPU
advanced per-half-dot so the read lands at its true M-cycle-aligned half-dot
against the even (correct) exit. That per-half-dot **READ** placement (not an
exit stamp) is the real, but small-yield, next lever; the bulk (line-start +
render-length) is whole-dot and blocked on the frame-anchored OCR-read tooling,
not on the half-dot at all.

Do NOT re-attempt: a swept odd-exit `flip_dot`/`vis_exit_hd` term (refuted here
by even-dc), a `read_pos_hd` `sb_dsa8`/paid-debt term (refuted #11ce), or a
blanket whole-dot line-start/exit shift (inert, fail-set md5-stable).

## Gate state / flip bar (unchanged — doc-only, tree at `25060d1`)

- No source change committed. golden_fingerprint `--release` PASS;
  mooneye OFF+tier2, tier2 CGB two-bin 291, clippy inherit `25060d1`.
- EV CGB two-bin **421** (unchanged). DS blockers cleared **0**. Flip bar
  **89 SameBoy-PASS blockers** (unchanged).

## Reproduction

- SameBoy dc parity: `SB_TRACE=1
  /tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester --cgb --length 4
  <rom>.gbc 2>&1 | grep SBMODE` → every `dc=` is even.
- EV two-bin: `CARGO_TARGET_DIR=target/ev cargo test -p slopgb-core --test gbtr
  --release --no-run`; `BIN=$(ls -t target/ev/release/deps/gbtr-* | grep -v
  '\.d$' | head -1)`; `SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt
  SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe
  --nocapture | grep pass=` → `pass=2599 fail=421`.
- ROM leg diff: `diff <hwtests>/scx_during_m3/scx_m3_extend_ds_1_*.asm _2_*.asm`
  → a single `.text@108c`/`108d` line (1 byte = 1 NOP = 2 DS dots).
