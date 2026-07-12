# Eager DMG LYC=153 coincidence-rise retime — REFUTED (counter-pinned dispatch wall)

Session goal: crack `m1statwirq_3` (the last gbtr-battery drop blocking the eager
C3 flip) by retiming the reclock `stat_update` engine's DMG LYC=153 coincidence
rise from dot 6 to dot 4 under `eager_value`, per the prior two agents'
localization.

**Verdict: REFUTED — genuine engine wall.** The rise dot is a UNIFORM PPU-side
lever. Moving it recovers `m1statwirq_3` but shuffles the line-153 LYC family
(+5 / −13, EV DMG 46→54). The rise state is **bit-identical** across ROMs that
want opposite rise dots — no representable PPU-engine discriminator exists. The
true discriminator is the CPU M-cycle dispatch phase, which the STAT engine
cannot see. This is the counter-pinned dispatch / coherent-per-T retime
(HALFDOT Part A), not a flag-gated law.

Base: `finish-port-halfdot @ e2c07de`. Tree left byte-identical (no code shipped).

## The localization (VERIFIED)

- ROM: `gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb`, DMG. OFF/SameBoy = `2`
  (PASS), eager = `0` (drop). Reproduced.
- Root of the eager rise dot: `Ppu::ly_for_comparison_line_153_at`
  (`ppu/stat_irq/reclock.rs:913`). DMG (`model <= CGB_C`, `!ds`) uses SameBoy's
  `GB_SLEEP(14,4)` schedule: `ly_for_comparison = 153` first appears at **dot 6**
  (`0..=5 => -1, 6..=7 => 153, 8..=11 => -1, _ => 0`). So the eager engine's
  `lyc_interrupt_line` latches at dot 6 → the `StatUpdate` LYC coincidence edge
  dispatches at `ly=153 dot=6`. Confirmed by the `SLOPGB dispatch` probe:
  eager fires exactly one `dispatch ly=153 dot=6 mfi=1 lycln=1`.
- OFF's gambatte engine (`stat_events_tick`, `stat_irq.rs:538`) fires the
  line-153 LYC event at **dot 4** (`self.line >= 1 && self.dot == 4 &&
  self.lyc_event == self.line`). The 2-dot gap (dot 6 vs dot 4) is a real
  SameBoy-vs-gambatte model difference; both are internally calibrated.

## The retime tried (and why it shuffles)

Change (eager+`!is_cgb`+`!ds`-scoped): back-date the DMG line-153
`ly_for_comparison` window from `6..=7 => 153` to `4..=7 => 153`. Single-ROM:
`m1statwirq_3` eager `0 → 2` (PASS); OFF unchanged. But the full DMG two-bin
(`SLOPGB_PROBE_EV` on the 3524-row gambatte DMG list, `GameBoy::set_eager_value`):

```
before (dot 6): flagon_probe[ON] pass=1605 fail=46
after  (dot 4): flagon_probe[ON] pass=1597 fail=54     (NET −8)
```

RECOVERED (5): `m1statwirq_3`, `lyc153int_m2irq_ifw_1`, plus 3 window `late_wy`
rows (cascade — the line-153 STAT rise reschedules the window-setup ISR).

NEW FAILS (13, all previously SameBoy-passing): `lyc153int_m2irq_2`,
`lyc153int_m2irq_late_retrigger_2`, `lycEnable/lycwirq_trigger_ly00_stat50_3`,
and 10 window `late_wy_*_3` rows.

## The A/B WELD TABLE (the refutation)

The `SLOPGB dispatch` probe augmented to print `eng_stat`/`stat_en`. Every
line-153 LYC family member produces the **identical** engine rise state
`ly=153 dot=4 mfi=1 lycln=1` (all with dot-4 forced; dot-6 is the symmetric
baseline). Grouped by `eng_stat`:

| ROM | eng_stat / stat_en | want frame |
|---|---|---|
| `m1statwirq_3` | **40** (LYC only) | **dot 4** |
| `lycEnable/lycwirq_trigger_ly00_stat50_3` | **40** (LYC only) | **dot 6** |
| `lyc153int_m2irq_ifw_1` | **60** (LYC+OAM) | **dot 4** |
| `lyc153int_m2irq_2` | **60** (LYC+OAM) | **dot 6** |
| `lyc153int_m2irq_late_retrigger_2` | **60** (LYC+OAM) | **dot 6** |
| `lyc153int_m2irq_ifw_2` | **60** (LYC+OAM) | insensitive |
| `lyc153int_m2irq_1` | **60** (LYC+OAM) | insensitive |
| `lyc153int_m2irq_late_retrigger_1` | **60** (LYC+OAM) | insensitive |

Within **each** `eng_stat` group the wants are OPPOSITE at bit-identical rise
state:
- eng_stat=40: `m1statwirq_3` (dot 4) ⊥ `lycwirq_trigger_ly00_stat50_3` (dot 6).
- eng_stat=60: `lyc153int_m2irq_ifw_1` (dot 4) ⊥ `lyc153int_m2irq_2` /
  `late_retrigger_2` (dot 6).

`(line, dot, mfi, lyc_interrupt_line, eng_stat, stat_en)` — the complete engine
rise context — is bit-identical for ROMs demanding opposite rise dots. There is
**no representable PPU-engine discriminator.** The prior agents' hoped separator
("fresh LYC=153 write vs steady-state", the `l153_lyc_write_dot` term) does not
apply: these are all steady-state LYC=153 coincidences (LYC preset, no line-153
FF45 write) — `l153_lyc_write_dot == u16::MAX` for all — and they still split.

Also checked and rejected as discriminators: `mfi` (all 1), `lyc_interrupt_line`
(all 1), the OAM/m2 enable bit (`eng_stat` 40 vs 60 — splits BOTH ways within
each group), halt-vs-running (all four are NOP-sled running-CPU dispatches per
the ROMs, not halt-exit — no `stat_halt_late` phase split).

## The mechanism the coherent rise needs (why it is the wall)

The difference between want-dot-4 and want-dot-6 lives entirely in the **CPU's
M-cycle dispatch phase** at line 153 — the instruction stream / DIV alignment
each ROM reaches line 153 with (the `ifw` variants write FF0F, shifting the
dispatch phase one M-cycle; `lycwirq_trigger_ly00_stat50_3` differs from
`m1statwirq_3` only in CPU code despite identical STAT enables). SameBoy sets
`IF |= STAT` at the exact T-cycle of the dot-6 rise (`GB_STAT_update`) and
`GB_cpu_run` samples IF at the M-cycle's trailing T-cycle in lockstep on ONE
per-T interleaved advance, so a dot-6 rise is caught coherently regardless of
each ROM's phase. slopgb's eager clock advances the PPU whole-dot and folds
`pending_if` per-dot, but the CPU's interrupt-enable sample for an M-cycle is a
fixed cc+4 dispatch that does not re-observe the odd-dot (5/6/7) PPU rises inside
that M-cycle — so a dot-6 rise is seen only at the NEXT M-cycle for some phases
and the current one for others, and no single rise dot (4 or 6) is globally
correct.

The fix is therefore not a rise-dot move and not a flag-gated law: it is the
coherent per-T dispatch/read retime (HALFDOT Part A — the CPU and PPU sharing one
per-T advance so `intf` is sampled at the true T-position the PPU set it). This
is the same counter-pinned-dispatch wall the #11br dispatch-fold, #11cb residual
("counter-pinned dispatch reads, C3-flip"), and #11dq scoped-dispatch-retime
(inert / pair-shuffle) each hit. `m1statwirq_3` is a member of that residual set,
not a separable read-frame row.

## Gate status

- No code shipped. `git diff e2c07de -- crates/` empty (tree byte-identical;
  golden/tier2/mooneye unaffected by construction).
- The retime that was A/B-measured is reverted.

## Do-not-repeat

- The DMG line-153 `ly_for_comparison` dot-6→dot-4 back-date (uniform PPU lever):
  recovers `m1statwirq_3` + `lyc153int_m2irq_ifw_1`, drops `lyc153int_m2irq_2` /
  `late_retrigger_2` / `lycwirq_trigger_ly00_stat50_3` + 10 window `late_wy_*_3`.
  NET −8 EV DMG. It is a shuffle — the family splits at bit-identical rise state.
