# Stage 3 (Part C) — the palette/accessibility re-derivation on the deferred frame (2026-07-02)

Follows the Part-C law collapse (`vis_exit_hd`, same session). Method: slopgb
`SLOPGB_S5DBG` pal/oamw/vramw tracers ↔ ROM hardware wants (cgb04c) ↔
SameBoy SBMODE/SBREAD fp where needed. Result: **two-bin 411 → 397 (−14/+0,
zero SameBoy-pass drops)** across four slices.

## 1. CGB palette-RAM boundaries (`pal_ram_blocked`) — 9 fixed

**Entry lock = 84 (the mode-3 anchor itself), NOT `84 + 3`.** The
`*_m3start_2` triplet (SameBoy-pass blockers) accesses at ly1 dot84 and wants
BLOCKED (read FF / write dropped) while the `_1` legs at dot80 land — the
previous `PAL_M3START_OPEN` grace sacrificed all three to serve the
STOP-shifted rows. The +3 grace belongs ONLY to shifted ROMs
(`*_m3start_lcdoffset1_1` law-dot-85 access must stay open — the shifted poll
lands +3 dots per +1-dot machine advance, the #11bd poll-quantum law, so the
shifted law frame under-corrects; boundary 87 there). The #11bd item-5b
`frame_skip` first-frame arm (84) is subsumed — 84 is now every unshifted
frame's base.

**Exit release = pipe_end + 1 dot SS / + 0 DS** (`pal_open_dot`, recorded at
`advance_lx` lx==160). The full m3end constraint table (write dot → read dot,
pipe = 256 + SCX&7):

| row | wr | rd | pipe | want | fit (+1 SS / +0 DS) |
|---|---|---|---|---|---|
| m3end_1 | 248 | 256 | 256 | 7 blocked | 256 < 257 ✓ |
| m3end_2 | 252 | 260 | 256 | 0 open | 260 ≥ 257 ✓ |
| scx2_1/2 | 248/252 | 256/260 | 258 | 7/0 | 256<259 / 260≥259 ✓ |
| scx3_2 | 252 | 260 | 259 | 0 open | 260 ≥ 260 ✓ — **Δ=+2 REFUTED here** |
| scx5_1/2 | 252/256 | 260/264 | 261 | 7/0 | 260<262 / 264≥262 ✓ |
| ds_1/2 | 250/252 | 254/256 | 256 | 7/0 | 254<256 / 256≥256 ✓ (Δ_DS=0) |
| scx5_ds_1/2 | 256/258 | 260/262 | 261 | 7/0 | 260<261 / 262≥261 ✓ |

The whole-M-cycle `pal_access_edge` straddle stamp (`memory.rs`) is BYPASSED
under tier2 for FF69/6B reads — it is the cc+4 eager-frame device; the
deferred cc+0 read is resolved to its exact half-dot before sampling, and the
stamp re-blocked reads landing legitimately past the unblock (`scx2_2` read
260 with flip 258 in its span). Write side untouched. Bonus: `m3end_3`
(rebaseline-class) converges to hardware.

## 2. The M0Access straddle stamps under tier2 — 4 fixed

Same disease, OAM/VRAM: tier2 bypasses the m0_access straddle stamp for the
three READ sites + the OAM write; the DS line-END release (`ds_lineend_open`,
`254 + SCX&7`) extends to OAM WRITES (`postwrite_ds_2` write@254 lands, `_1`
@252 dropped; `postwrite_scx1_ds` 256/254). Fixed: `oam_access/postread_scx5_ds_2`
+ `postwrite_ds_2` + `postwrite_scx1_ds_2` + `vram_m3/postread_scx5_ds_2`.

**Two guards found by measured drops (both healed):**
- VRAM READ stamp bypass must EXCLUDE `vram_wr_recent` (a readback within 8
  dots of a same-line VRAM write keeps the straddle view — the #11as
  co-temporality; without the guard `vramw_m3end_scx5_ds_2` SameBoy-pass
  dropped).
- The VRAM WRITE stamp stays on BOTH paths (`vramw_m3end_scx5_ds_4` dropped
  with the bypass).

## 3. The DS bare exit re-expressed EMERGENT — 1 fixed

`vis_exit_hd` arm 8 DS: `2*flip − 2 + 2*(SCX&1)` anchored to the render's
recorded/projected flip — algebraically identical to the #11ar closed form
`508 + 2*(SCX&7) + 2*(SCX&1)` on steady lines (flip = 255 + SCX&7), while a
mid-line SCX rewrite that re-arms the fine-scroll hunt extends the exit with
the render. Fixes `scx_m3_extend_ds_1` (dual-traced: SameBoy reads hd 668
mode3 / 672 mode0 = slopgb-frame exit ∈ (660, 664]; slopgb's projected flip
lands it exactly). `m2int/m0int_m3stat` families all green after (31 + 4
rows).

## Gates at commit

36 pins · lib 660 · mooneye flag-on 91/91 · clippy clean · full two-bin
411→397 (−14/+0) · gbtr OFF 221/0.

## Remaining stage-3 residue (traced, deferred)

- window 5: `late_wy_ds_2`-class (win_active + native m0 read@262 want 3 = the
  WINDOW hold direction, exit 263+SCX&7+ds — the never-shipped vis-HOLD
  twin of arm 2); `late_enable_ly0_ds_2` (line-0/first-line arm-1 exclusion
  over-broad: late-ENABLE first lines take the steady exit, late-WY first
  lines extend later — needs a trigger-source discriminator);
  `late_wx_scx5_1`; `late_disable_spx10_wx0f_2` (ns=1 — sprite-laden,
  excluded from every arm); wxA5/A6 (non-FF41 registers).
- oam 2 / vram 2: `prewrite_lcdoffset*_1` + `preread/prewrite_lcdoffset2_1`
  (shifted-frame entry locks, gambatte-ref class).
