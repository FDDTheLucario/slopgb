# VRAM viewer + I/O map + introspection

## VRAM viewer

Interactive (tab/checkbox/radio clicks + hover details) and **resizes with the
window**: each tab's grid renders at the largest integer scale that fits
(`vram::fit_scale`), keeping a 1px tile border at any scale; the grid + frame are
**bounded to the actual map extent** (`windows::vram_geom`, so the BG map doesn't
bleed grid lines into empty space — QA finding). Hit-tests (`tile/oam/bgmap_details`)
take `scale`; `vram::oam_cell` = `10*scale`.

### CGB-attribute-aware

- Per-tile/per-sprite VRAM bank (attr bit 3 over the two-bank `gb.vram()` slice).
- 8×16 OAM (LCDC bit 2: two stacked tiles `tile&!1`/`tile|1`, order swapped on
  Y-flip; tall-aware `vram::oam_cell_h` pitch).
- CGB OBJ/BG palette (attr bits 0-2) vs DMG (OBP bit 4 / BGP); X/Y flip
  (`vram::flip_tile`).
- **Tiles bank-0/1 selector** (DMG-inert).
- BG-map **SCX/SCY box wraps** the 256×256 map (`vram::bgmap_viewport_segments`,
  ≤4 segments); a **BG⇄window toggle** shows the window tilemap (LCDC bit 6) + the
  WX/WY box (`vram::window_region_rect`, `vram::MapOverlay`).

### Palettes tab

CGB palette RAM on CGB/AGB, but the **DMG BGP/OBP0/OBP1 shade mappings** on a DMG
machine (`vram::dmg_palette_rows`/`render_palettes_dmg`, so `rBGP`/`rOBP` are
inspectable).

## Read-only introspection (`&self`, golden-safe)

Lives in `slopgb_core::debug` (std-only, side-effect-free) plus a few `&self`
accessors on `GameBoy`:

- `wave_ram()` — the raw FF30-FF3F bytes (`Apu::wave_ram`→`Wave::ram`), so the I/O
  viewer's `wave (FF3x)` row is stable (the gated read path returns 0xFF / a volatile
  sample byte while ch3 plays).
- `rom_bank()`/`ram_bank()` (`Cartridge::cur_rom_bank`/`cur_ram_bank`) — a distinct
  ROM/RAM cartridge-bank indicator in the debugger regs pane's `ima` line + the I/O
  map, vs the VRAM/WRAM banks `VBK`/`SVBK`. `cur_rom_bank` reuses `read_rom`'s
  `rom_bank_for` so the two can't diverge; `cur_ram_bank` is `None` with no RAM chip →
  shown `--`.
