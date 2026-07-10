# #11dg — the eager CGB SS accessibility residual: 5 CLEAN re-hosts SHIPPED, the vis_early pairs WELDED to the eager flip-dot, the DS sprite WELDED to `vis_exit_hd`

Task: classify the remaining CGB SS + a few DS **mode-3 accessibility** flip-bar
rows as a clean #11da-style re-host or a Part-A-render weld, and ship any clean
slice. **Result: MIXED per-row. Two clean families SHIPPED (EV CGB 323 → 318,
+5/−0 both models); the `postread_scx3_2` read/write family is WELDED to the
eager `vis_early` flip-dot (net-negative gate); the DS sprite row is WELDED to
`vis_exit_hd`. No render-length move exists for the shipped 5 — they are pure
frame/stamp mis-frames the ported laws already resolve.**

## Method

Dual-trace every target row's accessibility read under `SLOPGB_EAGER` vs
`SLOPGB_TIER2` (`examples/run_gambatte`, `SLOPGB_ACCTRACE` temp probe in
`Ppu::vram/oam/pal_*_blocked` + the interconnect read sites), then full A/B via
`flagon_probe` (`SLOPGB_PROBE_EV`, 3422-row cgb/dmg rowlists). The deciding
question per row: does the eager verdict diverge on a WHOLE-M-cycle stamp/frame
(CLEAN) or on `read_pos_hd < vis_exit_hd` / the `vis_early` flip dot (render-LENGTH
= WELDED)?

## The deciding traces

### `vram_m3/postread_scx3_2` (SS) — WELDED to `vis_early`

```
EAGER  ly=1 dot=256 lawdot=256 rphd=520 vram=BLOCKED vearly=true flipdot=0 scx=3 vis_exit3=Some(512)
TIER2  ly=1 dot=256 lawdot=256 rphd=512 vram=OPEN    vearly=true flipdot=0 scx=3 vis_exit3=Some(512)
```

Both clocks read at the SAME dot 256 with `vis_early=true`. The divergence is a
GATE: `vram_read_blocked` releases on `vis_early` only under `tier2_reclock`
(`|| (tier2 && vis_early)`), so the eager read stays blocked. Looks trivially
clean — BUT the guard tells the real story:

```
postread_scx3_1 (want BLOCKED) EAGER  dot=252 vearly=FALSE   (safe)
postread_scx0/scx5_1 (want BLOCKED)   dot=252 vearly=TRUE    (over-releases)
```

Extending the gate to `(tier2 || eager) && vis_early` recovered `scx3_2` but
DROPPED the `_1` guards on scx0/scx5 (full A/B: CGB +13/−9 = net −4, **DMG
74 → 79 +5**). Root cause: the eager `vis_early` latch dot uses the LE
`early_lead = 3` (bare) — NOT the tier2 collapsed parity residue (`mode0.rs`
`early_lead` case-tower is tier2-scoped, and its residues break `intr_2_*_sprites`
under eager, #11by) — so the anticipated flip dot lands ~3 dots early on scx0/scx5
and collapses the `_1`/`_2` separation. The `vis_early` dot IS the render mode-3
flip position; correcting it needs the eager flip-dot reframe, i.e. the render
length. **WELDED (render-length via `vis_early`).** Same verdict for the OAM read
`oam_access/postread_scx3_2` and the OAM write `postwrite_2_scx3`
(`write_unblocked_early`, identical `tier2 && vis_early` gate).

### `cgbpal_m3/cgbpal_m3end_scx{2,5}_2` (SS) — CLEAN (palette stamp bypass)

```
EAGER  palR_STAMPED ly=1 dot=260 pal_ram_blocked=OPEN palopen=258 scx=2   → returns $FF (STAMP blocks)
TIER2  palR_STAMPED ly=1 dot=260 pal_ram_blocked=OPEN palopen=258 scx=2   → returns pal (stamp BYPASSED)
```

The CGB `pal_access_edge` stamp is a WHOLE-M-cycle block (`event_phase(PalAccess)
= END_PHASE`, `access_lead` cannot disarm it). `tier2` and DS-eager
(`ev_ds_access`) BYPASS it in `interconnect/memory.rs`; SS eager did NOT, so it
re-blocked the read $FF even though the ported `Ppu::pal_ram_blocked` (already
`|| eager_value`-gated: `dot 260 < pal_open_dot 258 + 1`) reads OPEN. A pure
stamp mis-frame — the length-derived anchor `pal_open_dot` is already correct.
**CLEAN.**

### `vram_m3/preread_lcdoffset1_1` (SS) + `preread_ds_lcdoffset1_1` (DS) — CLEAN (STOP-shift law frame)

```
preread_lcdoffset1_1  EAGER dot=83 lawdot=82 vram=BLOCKED   (entry lock d>=83; d=self.dot=83)
                      TIER2 dot=83 lawdot=82 vram=OPEN      (d=law_pos().1=82 < 83)
preread_lcdoffset1_2  EAGER dot=87 lawdot=86 vram=BLOCKED   (guard, law-dot86>=83 → stays blocked)
```

`vram_read_blocked`'s law position was `d = if tier2 { law_pos().1 } else {
self.dot }` — eager took the RAW dot, missing the STOP-shift (`lcd_shift_dots`)
correction that `tier2` AND `pal_ram_blocked` already apply. A whole-dot frame
mis-frame (fixed anchor `80 + late`, no `vis_exit_hd`), and the `_1`/`_2` pair
separates whole-dot on the law frame (82 opens, 86 blocks). **CLEAN.**

### `sprites/space/10spritesPrLine_wx7_m3stat_ds_2` (DS) — WELDED to `vis_exit_hd`

No accessibility trace fires — it is an FF41 STAT-mode read (`m3stat`), decided by
`read_pos_hd < vis_exit_hd(3)` on the DS sprite arm. The eager DS sprite exit is
the parked mid-dot floor (#11da). **WELDED (Part-A render length).**

## Shipped slice (B + E)

| change | file | edit |
|---|---|---|
| **E** palette stamp bypass | `interconnect/memory.rs` FF69/FF6B arm | `!self.ev_ds_access()` → `!self.eager_value` (SS+DS eager route through `pal_ram_blocked`) |
| **B** STOP-shift law frame | `ppu/blocking.rs` `vram_read_blocked` | `d = if tier2 { law_pos } else { dot }` → `if tier2 \|\| eager_value { law_pos } else { dot }` |

Both `eager_value`/`tier2`-gated; production (both flags off) byte-identical.

### Recovered (SameBoy-pass, was EV-fail), full-CGB A/B `+5/−0`:

- `cgbpal_m3/cgbpal_m3end_scx2_2`, `scx3_2`, `scx5_2` (palette, E)
- `vram_m3/preread_lcdoffset1_1` (SS), `preread_ds_lcdoffset1_1` (DS) (STOP-shift, B)

`cgbpal_m3end_scx3_2` is a bonus (not in the target list). Pin
`eager_ss_access_passes` (5 recovered + 5 `_1`/`_2` guards, red-before-green
verified).

## Gates

- `golden_fingerprint` byte-identical (production defaults off).
- EV CGB two-bin **323 → 318**; EV DMG **74 unchanged**; A/B new-fails EMPTY on
  BOTH cgb+dmg rowlists.
- tier2 CGB two-bin **291** (unchanged — E is a no-op under tier2 which already
  bypasses; B's `law_pos` branch already fired under tier2).
- mooneye **92/92** flag-off AND `SLOPGB_MOONEYE_EAGER=1` AND
  `SLOPGB_MOONEYE_RECLOCK=1`.
- eager tripwires both models (run_mooneye `SLOPGB_EAGER=1`): `intr_2_mode0/
  mode3/oam_ok_timing`, `intr_2_mode0_timing_sprites`, `di_timing-GS` — all PASS.
- clippy `-D warnings` clean (default + `port_probe`); every `.rs` < 1000.
- 12 eager + 63 tier2 gbtr pins green.

## Residual (parked, WELDED)

The `postread_scx3_2` read/write vis_early family (3 rows) and the DS sprite
`m3stat` row are the render-length weld — recovering them needs the eager
flip-dot / `vis_exit_hd` reframe (HALFDOT Part A), not a gate flip. This confirms
Part A-render is unavoidable for these accessibility residuals.
