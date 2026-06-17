# bgb viewers + debugger enhancement batch (branch `bgb-viewers-debugger`)

Round of QA-driven fixes/features on the VRAM viewer, debugger, symbols, and a
standalone memory window. Each task is one TDD red-green cycle; run
`/rust-diff-review` after each and fix all findings before the next. Commit per
task. Final gate: full-branch `/rust-diff-review` + independent multi-agent
adversarial review.

**Invariants:** core changes golden-safe (debug module `&self`, gbtr fingerprint
byte-identical); no new core deps / no unsafe; every `.rs` < 1000 lines; clippy
`-D warnings`.

## Locked scope decisions (from user)

- **Breakpoints** = bgb-style **double-click in disasm toggles a breakpoint**.
- **rgbds** = selectable disasm syntax, **default rgbds**, bgb kept.
- **Symbols (.sym)** = disasm labels+operands, go-to-by-name, manager labels;
  the **standalone** memory viewer gets a **status bar** with the nearest symbol.
- **BG/WIN** = dedicated BG⇄WIN toggle (Auto stays BG-only) + WX/WY box.
- **Reset to bgb defaults** = an Options action reverting every slopgb-departure
  setting to bgb-faithful (pure-bgb mode).

## Tasks (status)

| # | model | task | deps | status |
|---|---|---|---|---|
| 1 | haiku | `flip_tile(pixels,xflip,yflip)` pure helper | — | **done** |
| 2 | haiku | `bgmap_viewport_segments(...)` wrap geometry | — | **done** |
| 3 | haiku | `window_region_rect(wx,wy,...)` geometry | — | **done** |
| 4 | sonnet | `.sym` parser `parse_sym(text)->SymbolTable` | — | todo |
| 5 | haiku | SymbolTable `name_at` / `nearest_before` | 4 | todo |
| 6 | opus | ANALYSIS: rgbds approach (syntax enum vs structured operands) | — | todo |
| 7 | opus | core `decode` emits rgbds text under `Syntax::Rgbds` | 6 | todo |
| 8 | opus | golden gate: gbtr + mooneye byte-identical post-rgbds | 7 | todo |
| 9 | sonnet | thread `Syntax` → DisasmFmt + Options/Debug toggle (default rgbds) | 7 | todo |
| 10 | sonnet | Tiles tab VRAM bank0/bank1 selector (DMG-inert) | — | **done** |
| 11 | opus | OAM render: per-sprite bank/8x16/obj-pal/dmg-pal/flip | 1 | **done** |
| 12 | opus | BG map render: per-tile palette/bank/flip | 1 | **done** |
| 13 | sonnet | wire viewport wrap into render_bgmap | 2 | **done** |
| 14 | sonnet | BG⇄WIN toggle + window tilemap + WX/WY box | 3 | **done** |
| 15 | sonnet | App held-KeyCode repeat guard (held F7/F3 step once) | — | todo |
| 16 | sonnet | disasm double-click toggles breakpoint | — | todo |
| 17 | sonnet | memory viewer nav: wheel + PgUp/Dn + arrows | — | todo |
| 18 | sonnet | symbols in disasm: labels + operand substitution | 4,12 | todo |
| 19 | sonnet | load `.sym` via path modal (PathPurpose::SymbolFile) | 4 | todo |
| 20 | sonnet | go-to by symbol name (fallback hex) | 4,5 | todo |
| 21 | haiku | symbol name beside manager rows | 5,19 | todo |
| 22 | sonnet | standalone Memory-viewer tool window | 17 | todo |
| 23 | sonnet | memory-window status bar (nearest symbol) | 5,22 | todo |
| 24 | sonnet | Options toggle: memory viewer in own window | 22 | todo |
| 25 | sonnet | Options action: **Reset to bgb defaults** (pure-bgb revert) | 9,14,24 | todo |
| 26 | opus | FINAL: full-branch /rust-diff-review + multi-agent review | all | todo |

Critical path: `6→7→8/9` (rgbds), `4→5→{18,20,21,23}` (symbols), `1→{11,12}`
(attrs), `17→22→{23,24}` (memory window); all converge on `25`/`26`.
