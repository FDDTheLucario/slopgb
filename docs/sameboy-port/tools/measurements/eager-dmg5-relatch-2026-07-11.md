# The 5 residual DMG STAT-bar rows — the #11ec/#11ed method RE-CRACKS all 5, none is a floor (#11ee)

Base: `finish-port-halfdot @ 8ec11c5` (= #11ed). **CODE SHIPPED** — three
`eager_value`+DMG-scoped mechanisms across `regs.rs` (FF41 retro `rd`, the
mode-2 carry-window seed), `lyc.rs` (`write_lyc_dmg` vblank-carry seed + the
LE-only (0,4) un-block), and `read_laws_exit.rs` (Arm 8 bare-line SCX un-extend).
EV DMG **45 → 38**, CLEAN **+7/−0** both models (5 targets + 2 bonus). All
measurement scaffolding (`SLOPGB_FF41T`/`_LYCT`/`_SCXT` FF41/FF45/FF43-write +
engine-tick + read + flip dumps) was REVERTED; only the fixes + comments remain.

## TL;DR — #11dy's "read-clock / pulse-suppression / floor" verdict is REFUTED

#11dy diagnosed these 5 with the DISCREDITED read-debt/dispatch reasoning (the
same lens that falsely floored the #11ec/#11ed window rows). Applying the CORRECT
method — ROM-binary `cmp -l` → full-trace diff → representable whole-M-cycle
latch → calibrate the eager-frame arm — every one cracks. Each `_1`/`_2` sibling
differs by a whole NOP that shifts a WRITE (the FF41 m2-enable, the FF45 LYC
match, or the FF43 SCX rewrite); the eager cc+0 frame records that write a full
M-cycle before the tier2 cc+4 frame the consuming law was calibrated against.

## The ROM-binary diffs (`cmp -l`) — every pair is a whole NOP

| family | `cmp -l` shift | shifted write |
|---|---|---|
| `late_enable_{1,2}` | 1-byte `00` @0x1064 | `LD A,$20; LDH($41),A` (m2 STAT enable) +4 dots |
| `after_lycint_disable_{1,2,3}` | 1-byte `00` | same (old=LYC) |
| `late_enable_m0disable_{1,2,3}` | 1-byte `00` | same (old=HBLANK) |
| `lycwirq_trigger_ly00_stat50_{1,2,3}` | 1-byte `00` @0x1060 | `XOR A; LDH($0F); LDH($45),A` (LYC:=0 match) +4 |
| `late_scx4_{1,2}` | 1-byte `00` @0x1009 | `LD A,$04; LDH($43),A` (mid-mode-3 SCX:=4) +4 |

## Family 1 — the FF41 m2-enable RETRO pulse-reach (regs.rs 0xFF41)

The DMG retro fires an m2-enable STAT pulse for a write in the pulse M-cycle or
the one after (`dot == 0 || dot == 4`), with a `dot==0 ? data : data|old` lycen.
Traced eager decisive writes (`ly=2`):

| cell | want | eager write | old |
|---|---|---|---|
| `late_enable_1` | fire(2) | dot0 | 00 |
| `late_enable_2` | nofire(0) | dot4 | 00 |
| `after_lycint_2` | nofire(0) | dot0 | 40 (LYC) |
| `after_lycint_3` | nofire(0) | dot4 | 40 |

The window/lycen were calibrated to the tier2 cc+4 frame; the eager write is 4
dots earlier. **Fix: `rd = dot + 4` (the +4 read-debt) into the retro window +
lycen** — `late_enable_2` dot4→rd8 (out of window → no fire ✓); `after_lycint_2`
dot0→rd4 (in window, lycen `data|old`=0x60 → held-LYC suppressed ✓). Recovered
`late_enable_2`, `after_lycint_disable_2`, **+ bonus** `m1/lyc143_late_m2enable_
lycdisable_2`, `m2enable/lyc1_late_m2enable_lycdisable_1`.

## Family 2 — the mid-mode-2 OAM spurious rise (regs.rs, m0disable)

`late_enable_m0disable_2` (old=HBLANK → retro EXCLUDED) fires via the dot-ENGINE,
not retro: for lines 1-143 `mode_for_interrupt == 2` (OAM source high) only across
dots 0-3, then NONE. The eager cc+0 m2-enable at ly2 dot0 (its true cc+4 commit is
dot4 = NONE) makes the engine see a fresh OAM rise at **dot1** → spurious IF
(dispatch ly=2 dot=1). `late_enable_2` (dot4, mfi already NONE) does NOT →
passes. **Fix: a fresh OAM enable in the dots-0-3 window that neither retro nor
the write-trigger fired (`!fire`) seeds the engine line HIGH (STAT blocking) — no
edge.** `m0disable_1` fires its real pulse (enable at ly1 dot452, PREVIOUS-line
carry, `old & OAM == 0` excluded here). Recovered `late_enable_m0disable_2`.

## Family 3 — the line-0 vblank-carry → LYC seamless handoff (lyc.rs)

`lycwirq_trigger_ly00_stat50` writes LYC:=0 to match line 0; want fire at dot8+,
no-fire at dot0/4. Traced eager decisive FF45 write (ly=0): `_1` dot0, `_2` dot4,
`_3` dot8. On line 0 the STAT line is held HIGH by the mode-1 (VBlank) carry
across dots 0-3, then DIPS at dot4 (mfi→2, OAM disabled). `_1` (write dot0)
establishes the LYC=0 match while the line is still high (seamless, sline stays
1). `_2` (write dot4) lands AT the dip → the LYC match re-raises a FRESH edge at
dot5 → spurious IF. **Fix: (a) the vblank-branch `(0,4)` un-block cell in
`write_lyc_dmg` is LE/tier2-only — under eager `_3` (dot8) lands in the VISIBLE
branch and fires naturally, so the vblank branch fully blocks; (b) a matching
LYC write the m1 block suppresses on line 0 seeds the engine line HIGH (seamless
carry).** Recovered `lycwirq_trigger_ly00_stat50_2`.

## Family 4 — the mid-mode-3 SCX render over-extension (read_laws_exit.rs Arm 8)

`late_scx4_2` passes OFF + tier2, fails LE + EV. Traced: BOTH siblings' FF41 reads
are byte-identical; the ONLY discriminator is `scx_write_dot` (0 vs 87). Root:
the eager cc+0 FF43 (SCX:=4) write commits `eff.scx` at dot87 — BEFORE the render's
fine-scroll hunt latches (~dot89) — so the eager render over-discards the NEW
fine-scroll and flips at **258** (vs OFF/tier2 **254**: the true cc+4 write lands
PAST the hunt → the current line keeps its fetch-start length). The FF43
write-commit debt that fixes this in the render is **REFUTED** (`regs.rs`
`stage_write`: `eff.scx` IS the length → the debt breaks the `late_scx_late_disable`
window siblings). **Fix: the verdict-only READ analogue — back out the spurious
`eff.scx&7` extension from the BARE-line exit (Arm 8) when `scx_write_dot != 0`.**
Window aborts own the `scx_write_dot` arm (read_laws_exit.rs ~205), so scoping to
the bare exit leaves them untouched. Recovered `late_scx4_2`.

## Gates (all hold)

| gate | value |
|---|---|
| `golden_fingerprint` (production) | **ok — byte-identical** (43s) |
| EV DMG | **45 → 38** (−7 clean: 5 targets + 2 bonus; 0 new) |
| EV CGB | 295 (unchanged — all arms `!is_cgb`/DMG-scoped) |
| tier2 DMG / CGB | 116 / 291 (unchanged — every change `eager_value`-gated) |
| mooneye OFF / RECLOCK / EAGER | **93 / 93 / 93** (intr_2 / di_timing incl.) |
| clippy `-D warnings` | clean |
| file cap | `regs.rs` 967, `lyc.rs` 464, `read_laws_exit.rs` 678 (all < 1000) |
| pin | `eager_dmg_stat_relatch_passes` (red before → `late_enable_2` shows 2) |

## Do-not-re-chase ledger

- None of the 5 is a floor. Each is a whole-M-cycle NOP that lands a WRITE at a
  representable eager dot, mis-framed against the tier2 cc+4 frame — the same
  #11ec/#11ed shape. #11dy's "read-clock / pulse-suppression / floor" verdict was
  the read-debt-only failure mode (a read-debt moves both siblings equally).
- The FF43 SCX write-commit debt stays REFUTED (`eff.scx` IS the mode-3 length);
  `late_scx4` is fixable ONLY on the read side (bare-exit un-extend), leaving the
  render + the `late_scx_late_disable` window arm untouched.
- Method note: for a `_2` that fires via the dot-ENGINE (not the write-trigger),
  the discriminator is the mfi carry window (mode-2 dots 0-3 / the line-0 vblank
  carry), and the fix seeds the engine line — the STAT-blocking analogue of the
  retro read-debt.
