# EAGER read-retime — REFUTED: the cc+0 peek + compensation tower is LOAD-BEARING (it reads the mode VALUE at cc+4 while reading the render STATE at cc+0); a literal true-T (cc+4) sample re-couples them and is strictly worse. The halt rows are an ENGINE IF-fold at halt entry, NOT a read-frame miss (2026-07-09, #11co)

Task (the last structural lever): retime the eager FF41 read to the read's true
T (cc+4, the M-cycle data phase) and DELETE the compensation tower that exists
"only to undo the cc+0 peek" — expecting the halt race + the `_a`/`_b`
read-position collapse to resolve for free. Design (a): sample after
`tick_machine`. Build-measure, RED-tolerant; a crisp refutation is valid.

## Answer — NO on every axis, with a load-bearing structural reason

- **Design (a) does NOT fix the early HALT.** The halt rows read
  **byte-identical `got`** (2, 7) under the cc+0 peek and the cc+4 true-T sample
  — the OCR output is a direct function of where the HALT landed, so identical
  `got` ⟹ identical HALT dot. Zero halt rows recovered.
- **It net-REGRESSES:** EV CGB **361 → 425** (+27 pass / −91 fail), EV DMG **92
  → 152** (+11 / −71). TRUE flip bar **CGB 49 → 98 BUG, DMG 46 → 84 BUG** (each
  ~doubled). Nothing ships.
- **The mechanism (the load-bearing finding):** the cc+0 peek + tower is a
  DECOMPOSITION — it reconstructs the mode **VALUE** at the cc+4 read position
  (`read_pos_hd = 2·dot + 8 hd`, entry-80, line-boundary back-dates) while
  keeping the render **STATE** the mode-3-length arms sample (`render.win_active`
  / `flip_dot` / `n_sprites` / `eff.*`) at the cc+0 (pre-flip) dot. A literal
  cc+4 sample collapses both frames onto cc+4, dragging the render state past the
  mode-3 flip — so the 91 CGB / 71 DMG window+sprite **length** reads mis-fire.
  This is the READ-SIDE confirmation of #11cj's "read and render clocks
  CONFLICT". The tower is NOT vestigial cc+0-undo; it is the very machine that
  DECOUPLES the two frames.
- **The halt rows are not a read-frame problem at all** (ground truth below):
  the eager CPU NEVER enters HALT on `late_m0int_halt_m0stat_scx3_3a` — the ly=1
  mode-0 STAT is already folded into `intf` at the HALT instruction, so it
  dispatches inline. That is an ENGINE IF-fold at halt entry (the #11cm intr_0
  class), upstream of and independent of the FF41 read. #11cn's "the cc+0 FF41
  poll peek tips the halt race" is CORRECTED here.

## Baselines (branch `eager-read-retime` @ `94f91b2`, `CARGO_TARGET_DIR=target/agR2`, env unset = byte-identical)

| bin | rowlist | fail | note |
|---|---|---:|---|
| EV CGB (`SLOPGB_PROBE_EV=1`) | cgb | **361** | the baseline |
| EV DMG | dmg | **92** | |
| tier2 CGB (default probe) | cgb | **291** | held |
| OFF CGB (`SLOPGB_PROBE_OFF=1`) | cgb | 486 | flip-bar reference |
| OFF DMG | dmg | 103 | |
| TRUE flip bar CGB (OFF-pass ∩ EV-fail ∩ SB-pass) | — | **49 BUG** / 42 FLOOR | of 91 |
| TRUE flip bar DMG | — | **46 BUG** / 9 FLOOR | of 55 |

## What was built (env-gated `read_true_t`, byte-identical off — the reproduction vehicle)

Sub-flag `read_true_t`, true only under `eager_value` + `SLOPGB_READ_TRUE_T`
(mirrors the `coherent_dispatch`/`ff0f_le` probe pattern — declared + defaulted,
not serialized). Six sites, all `read_true_t`-gated:

| site | change |
|---|---|
| `interconnect/cycle.rs` `leading_edge_sample` | `0xFF41 if !read_true_t` → FF41 trails to the cc+4 `read_no_tick` (M-cycle data phase) |
| `interconnect/bus.rs` `read`/`read_inc` | `set_read_carried(false)` moves AFTER the trailing read (the ISR carry must survive to it) |
| `ppu/engine.rs` `read_pos_hd` | eager +8hd/+4hd read-debt → 0 (self.dot is already +4 at cc+4) |
| `ppu/stat_irq.rs` `mode3_entry_dot` | 80 → native 84 |
| `ppu/stat_irq.rs` `vis_mode` | glitch back-date `−4` → native `GLITCH_MODE3_START` |
| `ppu/stat_irq/read_laws.rs` `vis_mode_read` | 4 line-boundary back-date arms (CGB line-start/VBlank/line-0 + DMG mirror) gated off |

**Design (a), not (b).** (a) is the direct "is the peek the bug?" test and it is
decisive. (b) — a `GB_display_sync` half-dot resolve without re-advancing — was
NOT built: it cannot help (the SS halt polls are whole-dot `dhalf==0`, so a
half-dot resolve is a no-op there; and it would still read render STATE at the
resolved position, the same coupling that breaks the 91 length rows), and the
deferred-machine half-dot routing is already refuted by #11cj (breaks the +16
ISR reads).

## The HALT trace — the ground truth (single-row `late_m0int_halt_m0stat_scx3_3a`, CGB)

A temporary env-gated `eprintln` at `set_cpu_halted` (tick.rs, reverted — re-add
locally: print `(line,dot)` + `intf`/`ie` on the state change) counted the halt
transitions across the run:

| config | `set_cpu_halted(true)` calls | HALT lands | measurement `got` | verdict |
|---|---|---|---|---|
| **OFF** (pass) | **1** — ly=1 dot **336** (wakes ly=2 dot 260) | ly=1 336 | 0 | ✓ |
| **EV** (fail) | **0** — never halts | (inline dispatch) | **2** | ✗ |
| **design (a)** | **0** — never halts | (inline dispatch) | **2** | ✗ (identical to EV) |

The eager CPU **never enters HALT**: at the HALT instruction ly=1's mode-0 STAT
is already pending in `intf`, so it dispatches inline (the "wake[first]"
halt-entry sample #11cn saw was the ENTRY check, not an actual halt — `set_halted`
was never called). Because the STAT-IF **fold dot** — set by the engine
(`stat_update_tick`), not the FF41 read — decides this, the FF41 read frame is
irrelevant, and design (a) is byte-identical (0 halts, `got=2` both). This is the
#11cm class (glitch/line engine IF timing), NOT the read frame.

## Which compensations "died", which survived, and why they don't die cleanly

Turning the tower off tested each compensation's premise ("does it exist ONLY to
undo cc+0?"):

- **The line-boundary back-date arms + entry-80 + read-debt ARE genuine VALUE-frame
  compensations** — they recovered **27 CGB / 11 DMG** line-boundary rows when the
  cc+4 sample gave the base `vis_mode()` the right frame natively (window
  line-start, lcd_offset, ly0, some m2int_m2stat). So the VALUE half of the tower
  IS "cc+0-undo" and could be replaced by a true-T sample **in isolation**.
- **But they do NOT die independently.** The SAME sample move drags the render
  STATE the mode-3-**length** arms read to cc+4 = past the flip, breaking **91 CGB
  (31 window + 28 sprites + …) / 71 DMG** length reads. `read_pos_hd` itself is
  invariant under the swap (`2·dot_old+8` either way), so the length arms' exit
  comparison is unchanged — what breaks is the `m`-gate and the `render.*`/`eff.*`
  state, now sampled a full M-cycle late. The +27/−91 (CGB) and +11/−71 (DMG)
  trade is the coupling cost.
- **The ISR read-carry (`isr_read_carry_hd`, +4/+2 hd) SURVIVED clean** — it is a
  real sub-M-cycle phase offset, not a cc+0-undo; `intr_2_mode0/mode3`,
  `intr_2_0`, `di_timing`, `intr_0_timing` all stay **B=03** on both models even
  under `read_true_t` (the mooneye ISR path never hits the render-length arms).
- **No compensation exists for the halt rows** — they were never a read-frame
  compensation (engine IF-fold), so the read retime moved them zero.

## Why the true-T read is worse on this clock (the precise mechanism)

The eager clock's correctness rests on a two-frame split the tower implements:

| quantity | frame it must be read at | supplied by |
|---|---|---|
| the FF41 mode VALUE | cc+4 (the read's true T) | `read_pos_hd = 2·dot_cc0 + 8 hd`, entry-80, line-boundary back-dates |
| the render STATE the length arms sample | cc+0 (the peek's dot, pre-flip) | `self.dot`/`self.render.*`/`self.eff.*` AT the pre-tick peek |

The cc+0 peek + tower delivers BOTH: it samples render state at cc+0 and adds +8
hd to place the VALUE at cc+4. A literal cc+4 sample can only deliver ONE frame
(both at cc+4). So true-T is not "impossible" — it is a strictly worse
decomposition that RE-COUPLES the two frames the tower deliberately decoupled.
**The tower is load-bearing and correct; it is NOT vestigial cc+0-undo.**

**This does NOT indict the eager decomposition.** The eager read frame is already
right (cc+0 peek + tower = the SameBoy cc+4 value with the pre-flip render state).
The port should NOT return to a coherent deferred clock on the strength of the
halt rows: those are an engine IF-fold, orthogonal to the read.

## The redirect — the real halt-row lever (for the next session)

The halt `m0stat`/`dec` flip-bar rows (5 CGB + 6 DMG) need the eager STAT-IF FOLD
dot at halt entry to match SameBoy's — the same class as #11cm's `intr_0` fix (an
engine `mode_for_interrupt`/rise-dot gate), NOT a read retime and NOT the tier2
wake-mask port (#11cn, structurally unportable). Trace: the eager
`stat_update_tick` folds ly=1's mode-0 STAT into `intf` early enough that the
HALT-entry `pending()` sees it and dispatches inline; production/tier2 raise it a
dot later (or mask it at entry) and halt. Find the eager arm that raises the
mode-0 IF one M early on the alignment line and gate it like #11cm's
`line_render_done` arm. Do NOT re-retime the read frame; do NOT move the
dispatch (thrice-refuted, #11br/#11bs/#11cl).

## Gate state (all HARD invariants green; `read_true_t` env-gated off = byte-identical)

- `golden_fingerprint` **PASS** (9020 cases match HEAD, 42 s — production
  byte-identical, all gates need `SLOPGB_READ_TRUE_T`).
- tier2 CGB two-bin **291**; EV CGB **361** / EV DMG **92** (env unset).
- `cargo test --test mooneye` **92 passed** flag-off; clippy `-D warnings` clean;
  every touched `.rs` < 1000 (interconnect.rs 674, read_laws.rs 951, engine.rs
  594; the pre-existing cartridge.rs/lib_tests.rs ≥1000 are untouched).
- Baseline eager tripwires (`SLOPGB_EAGER=1`) `intr_2_mode0/mode3/0`, `di_timing`,
  `intr_0_timing` all **B=03** both models.

## Reproduction

```
git checkout eager-read-retime
CARGO_TARGET_DIR=target/agR2 cargo test -p slopgb-core --test gbtr --release --no-run
BIN=$(ls -t target/agR2/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# env unset = byte-identical baseline:
SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 361
# design (a) true-T:
SLOPGB_READ_TRUE_T=1 SLOPGB_ROWLIST=$(pwd)/scratchpad/cgb_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 425
SLOPGB_READ_TRUE_T=1 SLOPGB_ROWLIST=$(pwd)/scratchpad/dmg_rowlist.txt SLOPGB_PROBE_EV=1 $BIN --ignored gambatte::flagon_probe::flagon_probe --nocapture | grep pass=   # 152
# halt-row identical got (the refutation): one-row list, EV vs READ_TRUE_T vs OFF → got=2 / got=2 / pass
grep late_m0int_halt_m0stat_scx3_3a scratchpad/cgb_rowlist.txt > /tmp/halt.txt
# HALT-dot trace: re-add the eprintln at interconnect/tick.rs set_cpu_halted, SLOPGB_HALTDBG=1
```
