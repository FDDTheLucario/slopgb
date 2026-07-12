# EAGER `ly_lyc_153_write-{GS,C}` ×6 — the coherent discriminated line-153 STAT-delivery retime SHIPPED: all 6 rom×model cases RECOVERED (full pass, not partial). The #11ei "AIRTIGHT Part-A / welded-to-dispatch" verdict is OVERTURNED — the delivery IS representable and tunable on the eager frame, keyed on the enable/disable discriminator #11dv's uniform back-date ignored (2026-07-12, #11ek)

Base: `finish-port-halfdot @ 2193724`. Flag-gated (`eager_value`), golden
byte-identical. All gates hold; commit `<HASH>`.

## Result — 6/6 recovered

| row | model | OFF | eager pre-fix | eager post-fix |
|---|---|:--:|:--:|:--:|
| `ly_lyc_153_write-GS` | Dmg / Mgb / Sgb / Sgb2 | PASS | FAIL | **PASS** |
| `ly_lyc_153_write-C`  | Cgb / Agb              | PASS | FAIL | **PASS** |

The stored per-round interrupt counts (`C014..C017`) now equal the OFF/SameBoy
reference `0,1,1,0` on every model.

## The ROM (per #11ei disassembly, confirmed)

Multi-round STAT-interrupt-COUNT test; ISR at `0x0048` = `inc b; reti` (B counts
STAT ints). Each round writes FF45 near the line-152→153 cutoff, then reads its
count and stores it:

- **C014/C015 = DISABLE family** (LYC=153 held, write 153→F0): does a late
  disable still let the held LY=153 coincidence fire? Reference `C014=0, C015=1`.
  Code: `LDH (45),A ; LD A,B ; LD (C0xx),A` — B read the M-cycle AFTER the write.
- **C016/C017 = ENABLE family** (LYC=F0 held, write F0→153): does a late fresh
  enable trigger? Reference `C016=1, C017=0`. Code inserts an `LDH A,(41)` +
  store between the write and `LD A,B`, so the count read is LATER than the
  disable family's.

## Root cause (eager frame, measured per-round via ISR/write trace)

The line-153 LYC-coincidence STAT IF is raised by the reclock engine at the
dots-6-7 `ly_for_comparison==153` window (CGB-C/DMG SS; Agb 4-11). On the eager
dispatch frame the CPU's interrupt-count read (`LD A,B`) straddles that window
differently per round:

| round | write commit dot | count-read boundary | dot-6 fire | OFF | eager pre-fix |
|---|:--:|:--:|:--:|:--:|:--:|
| C015 disable (CGB) | 1 | dot ~4 | too LATE | 1 | **0** |
| C016 enable (CGB)  | 1 | delayed | in time | 1 | 1 |
| C017 enable (CGB)  | 5 | delayed | too EARLY | 0 | **1** |

Disable wants the coincidence delivered **2 dots earlier**; enable wants it
**suppressed** (fresh write in the window is SameBoy's "side-effect" no-trigger
zone) — **opposite corrections**, keyed on the WRITE, exactly the discriminator
#11dv's uniform whole-dot back-date (+17 CGB shuffle) ignored.

## The fix — a DISCRIMINATED retime keyed on `l153_lyc_write_dot`

New field `Ppu::l153_lyc_write_dot` (u16, `MAX` = "no write this line", reset
each `start_line`, set at the FF45 write on line 153 CGB/eager) — the fresh-write
signal that separates a just-written LYC from a steady-state LYC=153 (the field
gate is what keeps the four gambatte two-bins at zero drop; an ungated version
regressed EV CGB +64 by suppressing every steady-state LYC=153).

1. **DISABLE early-deliver** (`reclock.rs`, CGB): at line-153 dot 3, when a held
   `lyc_event == 153` with `lyc != 153` and a write happened this line, raise
   `IF |= STAT` and `force_level(true)` (fires once; suppresses the dots-6-7
   re-edge). Delivers the held-153 coincidence at the eager-frame dot the CPU
   dispatch observes. → C015.
2. **ENABLE suppress** (`reclock.rs`, CGB): at line-153 dots ≥6 with `lyc == 153`
   and `l153_lyc_write_dot >= 5` (the write committed IN the window), zero
   `lyc_interrupt_line` and cancel the deferred `lyc_if_delay` (Agb re-delivers at
   dot 9 via that path). A write BEFORE the window (C016) still fires. → C017.
3. **DMG held-153 hold** (`lyc.rs::write_lyc_dmg`, eager): a disable of a held
   LYC=153 (`old == 153 && value != 153`) landing at line-153 dots 4-7 does NOT
   update the delayed `lyc_event` copy, so the held 153 fires the natural dots-6-7
   delivery (the DMG write commits later than CGB's, so dot 6 already precedes the
   count read — no early-deliver needed). Scoped to the held-153 disable so it
   does NOT spuriously fire `lycEnable/lyc0_ff45_disable` (the +1 EV DMG drop the
   un-scoped form caused, fixed). → GS C015 on all 4 DMG-family models.

The #11ei `step_dot` `lyc_event` dots-5-7 protection extension was BUILT and
found NOT load-bearing (the DIS/EN + DMG-hold arms subsume it) — dropped.

## Gates (all hold)

| gate | baseline | with fix |
|---|:--:|:--:|
| `golden_fingerprint` | byte-identical | **byte-identical** |
| EV CGB (`SLOPGB_PROBE_EV` cgb_rowlist fail) | 287 | **287** |
| EV DMG (dmg_rowlist fail) | 38 | **38** |
| tier2 CGB (cgb_rowlist fail) | 291 | **291** |
| tier2 DMG (dmg_rowlist fail) | 116 | **116** |
| mooneye OFF / RECLOCK / EAGER | 93 / 93 / 93 | **93 / 93 / 93** |
| lib unit tests | 762 | **762** |
| frontend bin tests | 508 | **508** |
| wilbertpol_matrix (OFF) | pass | **pass** |
| clippy `-D warnings` | clean | **clean** |

Zero gambatte-OCR drops on either rowlist under either clock. The eager arms are
`eager_value`-gated (tier2 byte-identical), and production (`eager_value` false)
never runs them → golden byte-identical.

## Red-before-green pin

`tests/gbtr/wilbertpol.rs::eager_ly_lyc_153_write_delivery` boots the six cases
with `harness::boot_eager` and asserts the Fibonacci verdict. Verified RED on the
base (all six `B=48 C=BE`) and GREEN with the fix.

## Method note (overturning #11ei)

#11ei declared the full fix "AIRTIGHT Part-A, welded to the CPU read/dispatch
frame" because its only lever was a UNIFORM delivery-dot back-date (which the
enable/disable symmetry defeats — it fixes one half, breaks the other). The
`rom-diff-weld` lesson held: the discriminator (`l153_lyc_write_dot` +
enable-vs-disable direction) separates the rounds. A per-round DISCRIMINATED
early-deliver / suppress on the eager frame — NOT a uniform sweep — recovers all
six. The delivery dot IS representable and tunable; the block was the missing
write discriminator, not a dispatch weld.
