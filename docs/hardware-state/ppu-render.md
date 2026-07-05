# PPU — window, fetch, OAM scan, mealybug, OAM bug

## Dot-serial OAM scan

`ppu/mod.rs` §Dot-serial OAM scan.

- Entry `i` is latched + evaluated at dot `2i+3` (gbctr; gambatte OamReader — `scan_latch_dot` anchoring pinned by gambatte oamdma/late_sp* + sprites/late_sizechange* per-slot races).
- Per-entry LCDC.2 sampling.
- While OAM DMA owns OAM (running, or halt/stop-frozen) the scan latches `$FF` — a disabled sprite (`Ppu::oam_dma_active`, edges = gambatte startOamDma/endOamDma: the first byte's cycle still latches real OAM; the disconnect outlives the last copy by one M-cycle).

Parked: chasing the residual late_sp `_ds` out3 rows (half-dot, cc-granular races compounded with the frozen-ds mode-0 flip lead) or strikethrough's 7-px residue (an undocumented glitch-sprite, see `smallsuites.rs`) with **whole-dot** timing — don't chase either; whole-dot granularity can't resolve them.

## Window machine

`ppu/render.rs`.

- WX comparator runs every dot, including the 8-dot prefill. Match position by WX value:

  | WX value | Match dot/column |
  |---|---|
  | WX 0-7 | pause-aware dot `WX+6` (sprite stalls shift it via `pos_dot`) |
  | WX >= 8 | `lx == WX-7` |
  | WX <= 166 | (upper bound; above this never matches) |

- Rising-edge only (`win_match_prev`); checked **before** the same-dot sprite trigger (window start wins).
- `win_line` = gambatte winYPos (`0xFF` at frame start, `++` per activation, so same-line retriggers draw the next row).
- LCDC.5 off mid-line aborts at the eff commit, with the BG resuming on the live column `(scx+x+1-cgb)/8` (`window_abort`).
- WX=166 on DMG never starts in-line — instead a 2-dot freeze + carryover into the next line's mode-3 start (`win_start_pending`, window drawn from col 1).
- A WX match while drawing injects one color-0 pixel when it lands on a window tile boundary (mealybug "reactivation").
- WX=0 start adds `SCX&7+1` discards.

### WY sampling

- Discrete weMaster sampling at dots 450/454 (+1 DMG) and line-0 dot 2.
- Plus a live compare against `wy2`, which lags the write per model:

  | Model | `wy2` write lag |
  |---|---|
  | DMG | 2 dots |
  | CGB | 6 dots |
  | ds (double-speed) | 5 dots |

- WX commits to the pipeline 1 dot later than the palette strobe (`stage_write` FF4B dots+1, pinned by m3_wx_4/5/6_change).

## Mode-3 fetch grid

`ppu/render.rs` `fetcher_step`.

- Every fetch VRAM access samples `eff` clean at its read dot on **both** families.
- LCDC.1 gates sprite pixels at the mix as well as the fetch (m3_lcdc_obj_en_change).
- Sprites with OAM X 0-7 fetch during the pause-aware prefill walk (`prefill_pos`), freezing the SCX hunt (gambatte spx0/spx1); penalty math unchanged (mooneye tables frozen).
- The BG fetcher free-runs through every sprite stall (prefill included), with the line's first push waiting for the pause-aware startup walk (`push_allowed`), keeping pixel 0 on its stall-shifted dot.

Parked: the rising-late CGB LCDC fetch view — tried and rejected. See the mealybug note below: it fits most `_cgb_c` photo columns but contradicts hardware-captured gambatte bgtiledata spx0B rows. Current law samples `eff` clean at the read dot on both families instead.

## Mealybug ppu state

Status of the `m3_*` ppu_state tests:

| Status | Tests |
|---|---|
| Pixel-perfect (both legs) | m3_bgp_change, m3_scx_low_3_bits, m3_window_timing, m3_window_timing_wx_0, m3_lcdc_win_en_change_multiple, m3_wx_4_change_sprites |
| Pixel-perfect, [Dmg]-only | m3_wx_4_change, m3_wx_5_change, m3_wx_6_change |
| Pixel-perfect [Dmg] legs | m3_lcdc_tile_sel_change, m3_lcdc_tile_sel_win_change, m3_lcdc_bg_map_change, m3_lcdc_win_map_change, m3_scx_high_5_bits, m3_bgp_change_sprites, m3_obp0_change |

Remaining (not yet pixel-perfect) legs are mostly:
- [Cgb] fetch-law residue — see the parked rising-late CGB LCDC fetch view above and the baseline comments (`_cgb_c` photo columns vs hardware-captured gambatte bgtiledata spx0B rows).
- Small [Dmg] scy / bg_en / obj_en single-pixel residue.
- The obj_size pair.
- Sub-dot LCDC-write races: win_en_change_multiple_wx, m2_win_en_toggle [Dmg].

## DMG OAM corruption bug

- Implemented via `Ppu::oam_bug` + `Bus::tick_addr` / `read_inc`.
- DMG-family only; suppressed while halted / during OAM DMA.
- Window + patterns are CRC-calibrated against blargg `oam_bug/` — all green **except** 7-timing_effect, a defective single build that self-destructs on real hardware too (see the baseline note in `tests/gbtr/blargg.rs`).

## Post-boot VRAM (boot logo)

- Post-boot VRAM holds the boot logo *tile data* (incl. the (R) tile `$19`; `install_boot_logo_vram`).
- Do: leave the DMG logo tile-**map** rows uninstalled — the pinned gambatte reference PNGs predate initial-VRAM modelling (see the doc comment), and gambatte's `_blank` halt ROMs are judged on the top tile row only.

## Frame skip and CGB boot palettes

- The first frame after an LCD enable is presented blank (`Ppu::frame_skip`, Pan Docs LCDC.7 / SameBoy frame-skip) — frame-compare harnesses must sample >=2 vblanks after the ROM's re-enable.
- CGB DMG-compat boot palettes are the real boot-ROM *defaults* (BG table != OBJ table, `interconnect.rs`).
- Do: leave the Nintendo-licensee title-hash table deliberately unmodelled.

## PPU interrupt raising

- The PPU raises STAT/VBlank IRQs via `Ppu::write`'s return value (single drain).
- When adding a PPU register path, OR the returned IF bits into `intf` like the existing interconnect call sites.
