# bar-19 eager floor — ADVERSARIAL AUDIT: the floor is CRACKED. #11eb's "no slopgb-representable discriminator" is REFUTED — it checked `clock.now()` at the decisive FF41 READ (byte-identical) but MISSED that the LCDC-disable WRITE lands at a different, latched, slopgb-representable dot (`win_predraw_abort_dot` = 102 `_1` / 106 `_2`). The DMG arm-D3 extend threshold `abd + 3 >= wxm` was off-by-one vs the eager read frame; `abd + 4` (eager-scoped) recovers `late_disable_early_scx03_wx11_2` + `late_disable_late_scx03_wx11_2` CLEAN +2/−0. SHIPPED (`eager_value`-gated, golden byte-identical). EV DMG 52→50 (2026-07-11, #11ec)

Base: `finish-port-halfdot @ 0a6d3bd` (= #11eb). **CODE SHIPPED** — one flag-gated
line in `read_laws.rs` arm D3 (999 lines held). The audit scaffolding (a full
CPU+bus `SLOPGB_FULLTRACE` probe on `Bus::read`/`write`/`check_exec` + a
`SLOPGB_D3DUMP`/`SLOPGB_D3K` sweep knob) was all REVERTED; only the fix + its
comment remain.

## TL;DR — the floor is CRACKED, not confirmed

#11eb's central claim: *"at the decisive FF41 read both welded siblings reach
byte-identical `clock.now()=70992, pending=4` → no slopgb-representable quantity
distinguishes the poll-phase weld → bar-19 needs a T-exact CPU core."*

**REFUTED.** #11eb under-checked: it looked only at the READ. A full-trace diff
(every read/write/exec from LCD-on to the OCR capture) shows the siblings diverge
at the LCDC **write**, one full M-cycle before the read — and that write dot is
LATCHED in slopgb's render state as `win_predraw_abort_dot`, which the DMG exit law
already thresholds on. The threshold constant was simply off-by-one for the eager
read frame.

## The ROM-binary diff — a whole NOP, not "sub-M-cycle 1-T"

`cmp -l late_disable_early_scx03_wx11_{1,2}.gbc` = 5 bytes, a **1-byte `00` (NOP)
insertion** at 0x100D that shifts `3E 91 E0 40` (`LD A,$91; LDH ($40),A` — the
LCDC window-disable write) down one byte in `_2`. `late_reenable_{1,2}` = identical
shape (NOP before `LD A,$91; LDH($40),A; LD A,$B1; LDH($40),A`). #11eb/#11ea/the
`read_laws.rs:128` prose all call this a "sub-M-cycle (1-T)" poll shift; it is a
**whole M-cycle (4 T)**, fully representable on slopgb's M-cycle-atomic CPU.

## The full-trace diff — divergence is at the WRITE, re-converges before the READ

`SLOPGB_FULLTRACE` (reverted) dumped `{addr,val,clk,pend,ly,dot,rphd}` on every
read/write + `{pc,op,...}` on every exec, `eager_value`, both siblings. 292940
trace lines each; **18 differing lines, all in one 9-line window** — the LCDC
write:

| | `_1` (want0, PASS) | `_2` (want3, FAIL) |
|---|---|---|
| FF40 write (LCDC=0x91) | `clk=70839 dot=104` | `clk=70843 dot=108` |
| decisive FF41 read | `clk=70992 dot=256 rphd=520 val=A0` | **byte-identical** |

The NOP shifts the write +4 dots; the CPU then re-syncs (a poll loop) so **every
FF41 read is byte-identical (0 diff lines)**. #11eb's `clock.now()==70992` at the
read is TRUE but irrelevant — the discriminator is upstream at the write, which
slopgb DOES represent (dot104 vs dot108) and DOES latch.

## The latched discriminator — `win_predraw_abort_dot`

`window_abort_flags` (`render/window.rs:188`) records `win_predraw_abort_dot =
self.dot` at the LCDC.5-clear. Dumped at the decisive read (`SLOPGB_D3DUMP`,
reverted):

```
_1: ly1 dot252 m=3 rphd=512 exit=Some(512) | abd=102 wxm=110 wxscx=3 ...
_2: ly1 dot252 m=3 rphd=512 exit=Some(512) | abd=106 wxm=110 wxscx=3 ...
```

`abd` DIFFERS (102 vs 106); everything else (wxm=110, fscx=3, rphd=512) is equal.
Both computed `exit=512` (bare) → `512 < 512` false → mode 0 → both OCR "0" → `_2`
fails. The DMG arm-D3 extend condition is `abd + 3 >= wxm` (i.e. abd ≥ 107):
`_1` 102 → bare ✓, `_2` 106 → bare ✗ (wants extend, needs abd ≥ 106).

The **CGB arm-3** already splits this exact pair (`read_laws.rs:569`,
`win_predraw_abort_dot <= 105` — its comment literally reads "`_1` abort104 bare /
`_2` abort108 extend"). The DMG port used a wx-relative form off-by-one from the
eager read frame: the eager cc+0 write records `abd` a full M-cycle before the
tier2 cc+4 read the `+3` was calibrated against, so the eager threshold is `+4`.

## The fix — `abd + (eager ? 4 : 3) >= wxm`

`read_laws.rs` arm D3:
```rust
let extend = abd + if self.eager_value { 4 } else { 3 } >= wxm && !scx_kills_early;
```
`eager_value`-scoped: tier2 keeps `+3` (its deferred read frame is calibrated to
it; `+4` REGRESSES tier2 DMG 116→118 — the read-debt is exactly the +1). Threshold
sweep `SLOPGB_D3K` (reverted) proved `+4` UNIQUE-optimal:

| d3k | EV DMG fail | vs +3 |
|---|---|---|
| 2 | 57 | −5 drops |
| **3** (was) | 52 | — |
| **4** (ship) | **50** | **+2 clean, 0 drops** |
| 5 | 52 | +2/−2 wash |
| 6 | 54 | −4 drops |

Recovered (clean, no want-opposite drop): `late_disable_early_scx03_wx11_2`,
`late_disable_late_scx03_wx11_2` — the two `abd`-threshold welds of the seven #11ea
targets. The other five are genuinely a different lever: `late_reenable{,_wx0f}_2`
(rphd-UP, the reenable arm), `late_wy_FFto2_ly2_scx{2,3}_1` + `late_scx_late_disable_0`
(exit=None, render-side off-arm reconstruction) — untouched here.

## Why #11eb/#11ea missed it

They swept only READ-DEBT levers (`bpre`/`reen`/`on`, an rphd shift). A read-debt
moves BOTH siblings equally → welded (their finding is correct FOR THAT LEVER). The
`abd` threshold is a per-sibling render-state discriminator they never varied —
`abd` differs 102 vs 106. "No discriminator at the read" ≠ "no discriminator";
the write-dot discriminator was already latched and already half-consumed by arm
D3, just mis-thresholded. This is the #11dm/#11do/#11dw pattern (a "floor" overturned
by looking one lever over).

## Gates (all hold — code shipped @ read_laws.rs, 999 lines)

| gate | value |
|---|---|
| `golden_fingerprint` (production, no port_probe) | **ok — byte-identical** (41s) |
| EV DMG steady-state | **52 → 50** (down, +2 clean) |
| EV CGB | 295 (unchanged, `is_cgb`-scoped elsewhere; DMG arm `!is_cgb`) |
| tier2 DMG | 116 (unchanged — `+3` kept off eager) |
| tier2 CGB | 291 (unchanged) |
| mooneye OFF / RECLOCK / EAGER | **93 / 93 / 93** |
| clippy `-D warnings` | clean |
| lib tests (stat_irq) | 97/0 |

## Do-not-re-chase ledger (correct #11eb)

- bar-19 is NOT the eager clock's true floor and does NOT need a T-exact CPU core.
  The `_1`/`_2` weld's discriminator is a WHOLE M-cycle (the inserted NOP), it lands
  the LCDC-disable write at a different, representable, latched dot
  (`win_predraw_abort_dot` 102 vs 106), and the DMG exit law already thresholds on
  it — the `+3` was off-by-one for the eager read frame. `+4` (eager-scoped) ships
  clean. #11eb concluded "no discriminator" from checking `clock.now()` at the READ
  only; the discriminator is at the WRITE.
- The REMAINING 5 of #11ea's 7 (reenable pair, exit=None pair+`late_scx_late_disable`)
  are a DIFFERENT lever (reenable-dot / render-side off-arm), NOT this `abd`
  threshold. This fix does not touch them.
- Method note: ALWAYS diff the COMPLETE access trace (writes + fetches + exec, not
  just the target-addr reads) to find the FIRST divergence. #11eb's "the preceding
  FF41 read stream is identical" was true and misleading — the divergence was in the
  FF40 WRITE stream it did not diff.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/hd8
cargo test -p slopgb-core --test gbtr --release --features port_probe --no-run
BIN=$(ls -t target/hd8/release/deps/gbtr-* | grep -v '\.d$' | head -1)
# EV DMG 50 (was 52):
SLOPGB_REQUIRE_ROMS=1 SLOPGB_PROBE_EV=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['
# tier2 DMG 116 unchanged (no PROBE_EV):
SLOPGB_REQUIRE_ROMS=1 SLOPGB_ROWLIST=$PWD/scratchpad/dmg_rowlist.txt \
  $BIN --ignored --exact gambatte::flagon_probe::flagon_probe --nocapture | grep 'flagon_probe\['
# ROM diff (the NOP): cmp -l .../late_disable_early_scx03_wx11_{1,2}*.gbc  -> 5 bytes, shift at 0x100D
# golden:
SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test gbtr --release golden_fingerprint
# mooneye x3:
for E in "" "SLOPGB_MOONEYE_RECLOCK=1" "SLOPGB_MOONEYE_EAGER=1"; do
  env $E SLOPGB_REQUIRE_ROMS=1 cargo test -p slopgb-core --test mooneye --release 2>&1 | grep 'test result'
done
```
