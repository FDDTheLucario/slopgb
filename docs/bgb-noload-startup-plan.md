# bgb no-ROM startup — TDD plan

Goal: slopgb starts with **no ROM** on the command line (remove the CLI execution
dependency), matching bgb which boots to a blank LCD window with no ROM. A ROM is
then loaded via drag-drop / **Load ROM...** modal / **Recent ROMs**. The blank
screen shows bgb's pale-green LCD-off colour.

Frontend-only (`crates/slopgb`); core untouched (golden-safe). Deps stay
winit/softbuffer/cpal only; no file >1000 lines; clippy `-D` clean; every
behaviour gets a red-first unit test.

## Ground truth (captured, never invented)

- bgb 1.6.4 launched under wine on Xvfb :23 with **no ROM** → a 320×288 (2×) LCD
  window titled `bgb`, filled with a solid pale-green LCD-off colour.
  Capture: `/tmp/bgb-norom-crop.png`; sampled centre pixel `srgb(232,252,204)` =
  `#E8FCCC`.
- bgb's active palette read straight from `bgb.ini` (`Color0..3`, stored **BGR**):
  `CCFCE8 90D4AC 708C54 382C14` → RGB `E8FCCC ACD490 548C70 142C38` (the named
  "BGB 0.3" scheme). `Color0` reversed = `E8FCCC` = the captured pixel exactly.
- bgb does **not** run the CPU with no ROM (the LCD just shows the off colour), so
  slopgb gates emulation off until a ROM loads (no garbage NOP slide).

## Tasks

| id | model | task | test |
|----|-------|------|------|
| 1 | haiku | `cli::Options.rom` → `Option<PathBuf>`; no-positional parse → `Run{rom:None}`; keep rejecting 2nd positional + unknown flags; USAGE shows ROM optional | `parse([])` Ok(Run) rom None; `parse([game.gb])` rom Some; `parse([a,b])`/`parse([--frob])` still Err |
| 2 | sonnet | reorder `SCHEMES` so `[0]="BGB 0.3"` = `[0xE8FCCC,0xACD490,0x548C70,0x142C38]` (slopgb default), grayscale → `[1]`; fix stale "index 0 = grayscale" comment | `SCHEMES[0].colors[0]==0xE8FCCC`; `Settings::default().dmg_palette==SCHEMES[0].colors`; scheme-cycle/defaults tests stay green |
| 3 | sonnet | `Session::blank(model)` builds a valid blank 32 KiB ROM-only `GameBoy`; title `""`; no battery; `quick_state` None | `Session::blank(Dmg)` constructs; title empty; `save_data()` None; `flush_save()` no-op |
| 4 | sonnet | `App.rom_loaded:bool`; `main` builds blank `Session` when `opts.rom` None (else loads, errors still exit); `apply_palette()` at startup + after each rebuild | App rom_loaded false/true per path; `apply_palette` pushes `settings.dmg_palette` to gb |
| 5 | haiku | pure `should_idle(paused,broken,rom_loaded)` → `about_to_wait` emulates 0 frames when `!rom_loaded` | false only when running+rom_loaded; true when `!rom_loaded` |
| 6 | haiku | pure `window_title(rom_loaded,title,state)`; no ROM → bare `"slopgb"` | `window_title(false,..)=="slopgb"`; `(true,"poke",running)=="poke — slopgb"` |
| 7 | sonnet | `App.blank_frame:Box<[u32;SCREEN_PIXELS]>` = `dmg_palette[0]`; `redraw` blits it when `!rom_loaded`; refill on palette change | all px == `dmg_palette[0]` (0xE8FCCC); refilled after palette change |
| 8 | sonnet | `load_dropped` (Load ROM modal + Recent) sets `rom_loaded=true` + resync on success; failure leaves blank state | App(blank,false) + `load_dropped(temp rom)` → true; bad path → still false |

Critical path: 1,3 → 4 → 5/6/7/8 (2 & 7 carry the palette).
