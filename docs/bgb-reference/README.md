# bgb UI reference (real screenshots)

Functional-parity target for slopgb's debugger/viewer UI. **Every spec line here is
derived from a real bgb screenshot in this directory** — captured from bgb 1.6.4 (the
binary at `~/Downloads/bgb/bgb64.exe`) running *Pokemon Crystal* under wine on this
machine, not from memory. Re-capture protocol is at the bottom. Goal is **functional
1:1, not code/pixel parity**: same panels, same data, same controls, laid out the same
way — re-implemented on slopgb's own software renderer (winit + softbuffer, no GUI deps).

## Image index

| File | Window / region |
|---|---|
| `01-main.png` | Main LCD window (160×144, integer-scaled; here 2×=320×288) |
| `02-debugger.png` | Whole debugger window (1172×786) |
| `dbg-menubar.png` | Debugger menu bar |
| `dbg-disasm.png` | Disassembly pane |
| `dbg-regs.png` | Registers panel + stack viewer |
| `dbg-memory.png` | Memory hex dump pane |
| `03-vram.png` | VRAM viewer — **Tiles** tab |
| `03a-vram-bgmap.png` | VRAM viewer — **BG map** tab |
| `03c-vram-oam.png` | VRAM viewer — **OAM** tab |
| `03d-vram-palettes.png` | VRAM viewer — **Palettes** tab |
| `04-iomap.png` | I/O map window |

## Windows

bgb is multi-window: one always-present **main LCD** window, plus three toggleable tool
windows (**debugger**, **VRAM viewer**, **I/O map**). Each is an independent top-level
window. slopgb must do the same (winit, keyed by `WindowId`).

### 1. Main LCD (`01-main.png`)
Just the emulated 160×144 framebuffer, integer-scaled, aspect locked. Already implemented
(`video.rs`). When the debugger holds a break, its title becomes `bgb (debugging)` and the
LCD freezes on the last rendered line.

### 2. Debugger (`02-debugger.png`)
Menu bar: **File · Search · Run · Debug · Window · Execution profiler** (`dbg-menubar.png`).
Body is four panes:

- **Disassembly** (top-left, `dbg-disasm.png`). One line per instruction:
  `BANK:ADDR  <hex bytes>  <mnemonic operands>   ;<m-cycles>  <running cycle counter>`
  e.g. `ROM0:0101 C3 6E 01    jp  016E              ;4   29`.
  - Address is `BANK:OFFSET` (`ROM0`, then bank no. for >0).
  - Disasm syntax is **no$gmb** flavor: lowercase mnemonics, bare-hex operands (no `$`),
    `jp 016E`, `ld a,(hl)`, etc. (`DisasmSyntax=no$gmb` in `bgb.ini`).
  - Current-PC line is a full-width **blue** bar with white text and a **yellow marker
    arrow (◆)** in the left gutter.
  - Left gutter also holds **breakpoint** dots (red).
  - Data regions render as `db XX` / `db XX,YY` and bgb auto-annotates: cartridge-header
    bytes get comments (`;cart type: MBC3+RAM+BATTERY+TIMER`, `;rom size: 2 MiB`,
    `;ram size: 32 KiB`, `;destination code: …`, `;header check (OK)`, `;global check (okay)`),
    and ASCII runs render as string literals (`"PM_CRYSTAL BYTE"`).
- **Registers** (top-right, `dbg-regs.png`). Two columns of `name= VALUE` (hex):
  - col 1: `af bc de hl sp pc ime ima`  (ime/ima show `.` when clear)
  - col 2: `lcdc stat ly cnt ie if spd rom`
    (`cnt` = current PPU dot within the line/`LY` window counter; `spd` = double-speed flag;
    `rom` = current ROM bank). CPU flags Z/N/H/C are reflected in `af`'s low byte and as
    the small checkbox letters at the panel's right edge.
- **Stack** (right, below registers, `dbg-regs.png`). Rows `HRAM:ADDR  WORD` descending
  from `SP` (16-bit little-endian words); the `SP` row is highlighted.
- **Memory hex dump** (bottom, `dbg-memory.png`). `BANK:ADDR  b0 … b7  b8 … b15  |ASCII|`
  — 16 bytes/row, two 8-byte groups, ASCII gutter (non-printable = `.`).

Colors (from `bgb.ini`, COLORREF = `0x00BBGGRR`): bg white `FFFFFF`, text black `000000`,
current-line blue `0000FF`, breakpoint red `FF0000`, freeze/locked yellow `00FFFF`,
hilight grey `808080`. Font: monospace (`courier new 9`); we use our own embedded
bitmap mono font.

### 3. VRAM viewer (`03*.png`)
Tabbed window: **BG map · Tiles · OAM · Palettes**.

- **Tiles** (`03-vram.png`): every tile in VRAM as a zoomed grid (16 cols; both CGB banks),
  optional grid lines. Right panel: hovered-tile preview, `Tile Number`, `Tile Address`
  (`0:8000` = bank:addr), `guessed palette` (`BG 0`), checkboxes `show paletted` + `Grid`,
  a `stretch N,N` zoom readout.
- **BG map** (`03a-vram-bgmap.png`): the 32×32 tilemap rendered, with the **screen viewport
  rectangle** overlaid at (SCX,SCY). Details panel: `X`/`Y` (map cell), `Tile No.`,
  `Attribute`, `Map address` (9800), `Tile address` (0:8000), `X-flip`/`Y-flip`, `palette`
  (`BG 0`), `Priority`. Checkboxes `Grid`/`scxy`/`pal` (+ stretch). Radios: **Map** = Auto /
  9800 / 9C00; **Tiles** = Auto / 8800 / 8000.
- **OAM** (`03c-vram-oam.png`): 40 sprite cells (8×5), each a preview + X/Y; empty slots draw
  a red diagonal. Details: `X-loc`/`Y-loc`, `Tile No`, `Attribute`, `OAM addr` (FE00),
  `Tile Address`, `X-flip`/`Y-flip`, `Palette` (`OBJ 0`), `Priority`.
- **Palettes** (`03d-vram-palettes.png`): `BG 0…7` and `OBJ 0…7`, each 4 swatches + 16-bit
  CGB color words (DMG: derived from BGP/OBP). Selected color shows `Red`/`Green`/`Blue`
  (5-bit, 0–1F) spinners + a `copy dw` button.

### 4. I/O map (`04-iomap.png`)
A dense grid of every I/O register, value + name, grouped: **LCD** (FF40–FF4B), **various**
(FF70/4F/4D/00/01/02/04/05/06/07/0F/FFFF), **Sound 1–4** (FF10–23), **sound control**
(FF24–26), **MBC** (ROM/RAM bank, mode, enable), **GBC DMA** (FF51–55), **GBC banks**
(VRAM/WRAM), **Timer** (running flag, rate), **CPU mode** (double-speed). Plus decoded
sub-panels: **LCDC (FF40)** and **STAT (FF41)** as labeled checkbox bit-breakdowns, **wave
pattern (FF30–3F)** hex, **internal divider**, **IF/IE** as the five vectors (40 VBlank /
48 LCD / 50 Timer / 58 Serial / 60 Joypad) with enable checkboxes + hit counters, **GBC
pal** (BCPS/BCPD/OCPS/OCPD), and a **Refresh** button.

## Disassembler format (no$gmb syntax, from `disasm-probe-*.png`)

Captured by loading `/tmp/disasm_probe.gb` (built by `gen_disasm_rom.py`, 67 labelled
opcodes at 0x0150) in bgb and reading its debugger. Rules slopgb's disassembler must match:

- **Mnemonics lowercase**, **hex operands uppercase**, no `$`/`0x` prefix.
- Widths zero-padded: a16/d16 → 4 digits (`jp 0150`, `ld hl,1234`), d8/a8 → 2 digits
  (`ld b,12`, `ld (ff00+44),a`), bit index decimal (`bit 7,h`), rst target 2 digits (`rst 38`).
- **8-bit ALU register ops drop `a,`**: `add b`, `sub b`, `and b`, `cp b`, `add (hl)`.
  **Immediate ALU keeps `a,`**: `add a,10`, `cp a,99`. (adc/sbc/xor/or/sub immediate assumed
  `op a,nn` by symmetry — confirm in the C8 visual-diff pass.)
- `ld a,(hl)`, `ld (hl),a`, `ld b,(hl)`; **HL+/HL- use `ldi`/`ldd`**: `ldi (hl),a`,
  `ldi a,(hl)`, `ldd (hl),a`.
- **LDH is spelled `ld (ff00+NN),a` / `ld a,(ff00+NN)`** (literal `ff00+` lowercase), and the
  C-forms `ld (ff00+c),a` / `ld a,(ff00+c)`. bgb appends an inline `;NAME` when ff00+NN is a
  known I/O reg (e.g. `ld (ff00+44),a ;LY`).
- `jp 0150`, `jp nz,0150`, **`jp hl`** (not `jp (hl)`), `call 0150`, `call nz,0150`,
  `ret`, `ret nz`, `reti`, `rst 00`.
- **JR shows the absolute target address** (pc+2+disp), not the displacement: `jr 0172`,
  `jr nz,016E`. (So decode needs `pc`.)
- Signed offsets: `ld hl,sp+03` / `ld hl,sp-02`; `add sp,+05` / `add sp,-02` (sign + 2-hex magnitude).
- Stack/misc: `push bc`, `pop af`, `ld sp,hl`, `ld (1234),sp`, `daa`, `cpl`, `scf`, `ccf`,
  `di`, `ei`, `halt`, `stop`.
- CB: `rlc b`, `rl (hl)`, `swap b`, `srl a`, `bit 7,h`, `res 0,(hl)`, `set 0,b`.
- **Illegal opcodes render the literal text `undefined opcode`** (len 1, 0 cycles) — *not*
  `db XX`. (`db XX` is reserved for the disasm pane's data/header regions, which carry the
  header annotations `;cart type: …`, `;rom size: …`, `;header check (OK)`, etc.)
- Comment column is `;<this-instr M-cycles>  <cumulative M-cycles>`; **conditional branches
  use the not-taken M-cycle count** (`jr nz`=2, `jp nz`=3, `call nz`=3, `ret nz`=2). The
  cumulative counter is a pane-level running sum, not part of per-instruction decode.

## Re-capture protocol (must use real screenshots, never invent)

1. `cp ~/Downloads/bgb/bgb.ini{,.slopbak}` (restore after).
2. In `bgb.ini` set `StartDebug=1`, `DebugWinShowOnStart=1`, `VramWinShowOnStart=1`,
   `IomapWinShowOnStart=1`, and give each `*WinX/Y` explicit coords.
3. `cd ~/Downloads/bgb && WINEDEBUG=-all wine bgb64.exe <rom>` (background).
4. `import -window root /tmp/r.png` (IM7 `-window` rejects numeric ids — capture root),
   then crop to each window's `xdotool getwindowgeometry --shell <id>`. Raise/activate the
   target first (`xdotool windowraise; windowactivate --sync`). VRAM tabs: `xdotool
   mousemove --sync X Y click 1` on the tab.
5. **Context + menu-bar menus DO open under synthetic clicks** (an earlier note here was
   wrong): `xdotool mousemove --sync X Y click 3` opens the pane's right-click menu;
   `click 1` on a menu-bar item opens its dropdown. `import -window root` then crop around
   the click; `xdotool key Escape` dismisses. Captured menus live in
   [`menus/`](menus/).
6. Restore `bgb.ini`, `pkill -9 -f bgb64.exe`.

**XInput crash + the reliable fix:** bgb can crash at startup under wine with `X Error …
X_OpenDevice XI_BadDevice` — wine's winex11 tripping on a flaky real XInput2 device on `:0`
(heavy synthetic `xdotool` input seems to trigger it). The dependable workaround is to run bgb
on a **fresh Xvfb display** (no real input devices to enumerate):

```sh
Xvfb :21 -screen 0 1400x950x24 -ac -nolisten tcp &           # clean virtual display
DISPLAY=:21 WINEDEBUG=-all wine bgb64.exe <rom> &            # bgb, no XInput crash
DISPLAY=:21 xdotool search --name "" …                       # drive it (clicks reach it)
import -display :21 -window root /tmp/r.png                   # screenshot, then crop
```

Captured bgb menus (debugger pane menus, Run/Debug bars, the main-window menu + submenus, and
the negative result that VRAM/I-O-map have no right-click menu) live in [`menus/`](menus/).
