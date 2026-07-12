# EAGER WRITE-side half-dot (`write_pos_hd`) — BUILT + MEASURED as a NET-NEGATIVE SHUFFLE; the m1statwirq_3 drop is an UPSTREAM dispatch drift (the DMG LYC=153 coincidence STAT rise is 4 dots late in the reclock engine), NOT a write-commit frame — the write classifier cannot see the discriminator (2026-07-12, #11dr)

Base: `finish-port-halfdot @ 9233110`. Task (#11dr, build the WRITE-side half-dot
`write_pos_hd`, the symmetric twin of the shipped READ-side `read_pos_hd`, to
recover the counter-pinned dispatch / STAT-write-commit family — primary target
`gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb` [Dmg], eager gives 0, want 2).
**Answer: the write-commit half-dot is REFUTED for m1statwirq_3.** `write_pos_hd`
WAS built and routed through `stat_write_trigger_dmg`; it recovers m1statwirq_3
(eager 0→2) but is a **net −10 pair-shuffle** on the flagon DMG set (EV DMG
38→48, +10 SameBoy-pass drops, 0 recoveries in the flagon set). The root cause of
m1statwirq_3 is NOT the write-commit frame — it is an **upstream dispatch drift**:
the DMG **LYC=153 coincidence STAT rise reaches `intf` 4 dots (1 M-cycle) late**
in the reclock `stat_update_tick` engine (identical writes, delayed rise → the
STAT ISR dispatches 1 M-cycle late → the verdict FF41 write drifts from `ly=0
dot=0` to `ly=0 dot=4`). The write classifier only sees `self.dot`; the
discriminating information (the write's TRUE SameBoy dot) was destroyed upstream,
so no write-debt — scalar or half-dot — can separate the fix from the drops. All
code REVERTED; tree byte-identical (`git diff crates/` empty).

## Baselines reproduced (exact, at 9233110)

| frame | EV CGB | EV DMG |
|---|---:|---:|
| EV (`SLOPGB_PROBE_EV`, flagon_probe) | **287** | **38** |

`run_gambatte` verdicts (first OCR digit): m1statwirq_3 OFF=`2` (pass) / EAGER=`0`
(fail) / TIER2=`0` (fail). The A/B siblings from #11dq's atomicity proof
**already pass at this base** (the #11ec–#11em rom-diff-weld sessions cracked
them): `lyc153_late_m1disable_3` EAGER dmg=E0/cgb=E0 (want E0);
`lycdisable_ff41_2` EAGER dmg=2/cgb=0 (want dmg2/cgb0). Both land at IDENTICAL
(line,dot) OFF vs eager — no write drift. So the write-commit family #11dq named
is RESOLVED; m1statwirq_3 is a different, un-cracked row.

## What was built (reverted)

- `Ppu::write_pos_hd(&self) -> i32` (engine.rs) = `2*dot + dhalf + write_debt`,
  the WRITE twin of `read_pos_hd`. `eager_value`-gated (else `2*dot`, byte-identical).
  Write-debt = SameBoy `GB_CONFLICT_WRITE_CPU` (write commits leading+1T) measured
  as **−6 hd SS / −2 hd DS**: under eager `write_no_tick` runs AFTER `tick_machine`
  advanced the PPU the full M-cycle, so the classifier sees `self.dot` at the
  trailing edge (leading+4 SS); SameBoy commits at leading+1 = −3 dots = −6 hd.
- Routed `stat_write_trigger_dmg` (the m1statwirq classifier): the `self.dot < 4`
  hblank/region tests → `write_pos_hd() < 8` (half-dot), `!(glitch && dot<GLITCH)`
  → `< 2*GLITCH`.
- `SLOPGB_WRDEBT` env sweep knob (Part-C, `probe.rs`).

## §1 — the trace: m1statwirq_3 is an UPSTREAM dispatch drift, not a write frame

Full `Bus::read/write/tick` + dispatch-decision trace (temporary `SLBUS`/`SLDISP`
probes, reverted). The verdict FF41 write:

| config | verdict FF41 write | `stat_write_trigger_dmg` | verdict |
|---|---|---|---|
| OFF  | `ly=0 dot=0` (line-start hblank, `dot<4`) | fire (hblank glitch) | **2** ✓ |
| EAGER| `ly=0 dot=4` (mode 2/3, `dot≥4`)          | no fire               | **0** ✗ |
| TIER2| `ly=0 dot=5`                              | no fire               | **0** ✗ |

The FF41 write code path is **identical** OFF vs eager (`bus.rs::write`; FF41 is
not in the DMG FF0F-only borrow set), so nothing about the write itself differs —
its ABSOLUTE PPU position drifted +4. First divergence (aligned by `cyc`): the
STAT interrupt to vector **0x0048** dispatches at the `0232` NOP-sled fetch under
OFF but the `0233` fetch under eager — **1 M-cycle later**. Cause (dispatch-decision
trace): the STAT IF bit (bit 1) reaches `intf`:

| config | `intf` bit1 raised | PPU pos |
|---|---|---|
| OFF  | cyc 69832 | ly=153 dot=4 |
| EAGER| cyc 69836 | ly=153 dot=8 |

The rise is the **LYC=153 coincidence** (`ly_for_comparison` = 153 from dot 4 of
line 153; the ROM set LYC=153 via an FF45 write at ly=151, IDENTICAL cyc OFF/eager
— NOT a late write commit). At the SAME PPU dot the production `stat_events_tick`
has already raised the STAT bit while the eager/tier2 `stat_update_tick` has not:
the reclock engine's DMG LYC-coincidence rise on line 153 is **4 dots late**. That
delays dispatch 1 M-cycle → the verdict write lands 4 dots late → mis-classified.

**This is a STAT/LYC engine-rise frame, not a write-commit frame.** The write
commits at the same within-M-cycle phase (trailing edge) as OFF; only its absolute
position drifted, from the upstream late rise.

## §2 — `write_pos_hd` sweep: recovers m1statwirq_3 but a −10 shuffle

`stat_write_trigger_dmg` routed through `write_pos_hd`, `SLOPGB_WRDEBT` swept on
the flagon DMG two-bin (`dmg_rowlist.txt`, `SLOPGB_PROBE_EV`):

| WRDEBT (hd) | m1statwirq_3 (run_gambatte) | EV DMG fail |
|---:|:---:|---:|
| 0 (≡ original) | `0` ✗ | **38** |
| −2 | `2` ✓ | 48 |
| −4 | `2` ✓ | 48 |
| −6 | `2` ✓ | 48 |
| −8 | `2` ✓ | 48 |

Any negative debt fixes m1statwirq_3 but **drops 10 SameBoy-pass rows, recovers 0**
in the flagon set (m1statwirq_3 is a `.gb` DMG-only ROM, not in the `.gbc`
rowlist, so it is measured only via `run_gambatte`). The 10 drops are the
STAT-write-enable glitch family:

```
lycEnable/late_ff41_enable_3       m2enable/late_enable_2
m0enable/late_enable_3             m2enable/late_enable_after_lycint_3
m1/m1irq_late_enable_3             m2enable/late_enable_after_lycint_disable_3
m2enable/late_enable_ly0_2         m2enable/late_enable_m1disable_ly0_3
m2enable/late_m1disable_ly0_3      m2enable/lyc1_late_m2enable_lycdisable_2
```

## §3 — THE ATOMICITY PROOF (concrete, this base): two writes at the SAME eager dot, OPPOSITE frames

The clean exhibit — `m1statwirq_3` vs the dropped `m2enable/late_enable_2` (want 0):

| row | OFF verdict write | EAGER verdict write | needs |
|---|---|---|---|
| `m1statwirq_3` (want 2)  | `ly=0 dot=0` | `ly=0 dot=4` | classify AS dot **0** (back-date −4) |
| `late_enable_2` (want 0) | `ly=2 dot=8` | `ly=2 dot=4` | classify AS dot **4** (NO back-date) |

Under eager **both writes land at dot 4**, arriving by OPPOSITE upstream drifts
(m1statwirq_3 +4 from a late STAT rise; late_enable_2 −4 from the eager
read/dispatch frame). They require OPPOSITE classification frames. A write-debt
applied to `self.dot` shifts BOTH equally, so it can never separate them — the
`−N` that back-dates m1statwirq_3 into the hblank window ALSO back-dates
late_enable_2's correct mode-2/3 write into the hblank window (false fire). This
is #11dq §3(b2)'s atomicity, reproduced concretely at 9233110: the discriminator
is the write's TRUE SameBoy dot (0 vs 8), **destroyed by an upstream dispatch
drift the PPU write classifier cannot observe** (it sees only `self.dot`).

## §4 — the SameBoy mechanism the fix actually needs (NOT a write class)

The lever is NOT a per-register `GB_CONFLICT_STAT_*` write split. It is the
**reclock `stat_update_tick` DMG LYC=153 coincidence-rise frame**: the natural
(non-write) LYC=153 coincidence must reach `intf` at PPU dot 4 (production
`stat_events_tick`'s emission), not dot 8. That is an ENGINE-source retime on the
most heavily-baselined code in the port (the `lyc153`/`lycEnable`/`ly0` family
pins it across BOTH models), analogous to the #11cm glitch-line mode-0 engine fix
— a separate, high-shuffle-risk investigation, not this task's write-commit lever.
Fixing the rise makes m1statwirq_3's verdict write land at its true `ly=0 dot=0`
naturally (no drift), so the 10 `late_enable` rows never move (no classifier
touch). The row **also fails pure tier2** (verdict write dot=5), so per
`rom-diff-weld` it has no arm to recalibrate — it needs the new engine-rise law
ported, not a write-frame recalibration.

## Gates (all hold; tree byte-identical)

- `git diff 9233110 -- crates/` **empty** (`write_pos_hd`, the classifier routing,
  the `SLOPGB_WRDEBT` knob, and all `SLBUS`/`SLDISP`/`ff41w` trace probes reverted).
- EV CGB 287 / EV DMG 38 reproduced; interconnect/engine reclock defaults NOT
  flipped; no push; parent branch untouched.

## Reproduction

```sh
export CARGO_TARGET_DIR=target/wrhd
cargo build -p slopgb-core --example run_gambatte --release --features port_probe
BIN=target/wrhd/release/examples/run_gambatte
R=test-roms/game-boy-test-roms-v7.0/gambatte/miscmstatirq/m1statwirq_3_dmg08_out2.gb
$BIN $R dmg                    # OFF  -> 2 (pass)
SLOPGB_EAGER=1 $BIN $R dmg     # EAGER -> 0 (fail); TIER2 also 0
# The upstream drift (re-add the SLBUS/SLDISP probes to bus.rs/speed.rs, reverted):
#   verdict FF41 write: OFF ly=0 dot=0 (fires) vs EAGER ly=0 dot=4 (no fire)
#   STAT dispatch: OFF fetch 0232 vs EAGER 0233 (1 M-cycle late)
#   intf bit1 raised: OFF cyc 69832 (dot 4) vs EAGER cyc 69836 (dot 8) — LYC=153 rise
# write_pos_hd sweep (re-add engine.rs write_pos_hd + stat_write_trigger_dmg routing):
#   SLOPGB_WRDEBT=-6 fixes m1statwirq_3 but EV DMG 38->48 (+10 SameBoy-pass drops)
```
