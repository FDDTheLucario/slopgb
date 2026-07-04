# DMG power-on boot-frame read law — SHIPPED (+20) (2026-07-04, #11bl)

The 21 DMG `poweron_*` gbmicrotest rows the #11bj engine-set classification
parked as "the C0 boot-DIV read-frame CHAIN … atomic … a per-read-position
frame = the sub-M-cycle read clock (S7)" are **re-measured and the boot-frame
read law BUILT + SHIPPED flag-gated** — `+20` gbmicrotest DMG rows, **zero
regressions on the full 513-row DMG matrix**, `boot_div` HELD, mooneye 91/91
flag-on AND flag-off, CGB two-bin zero-drift, production byte-identical OFF.

The #11bj "curve-fit / atomic with the +4 DIV" verdict is **corrected** — the
exact twin of the #11bk `hblank_int` correction one frame earlier: the boot
READ frame **decouples** from the counter-pinned `+4` boot DIV (a different
subsystem: the DIV is the timer counter, the boot read is a PPU-domain sample).

## The 20 targets (all `[Dmg]`, all pass flag-OFF, all fail flag-ON base)

`poweron.gb` is a no-verdict testbench (FF82 stays 0 flag-on AND flag-off — out
of scope; it never stores a verdict). The 20 real flip-blockers:

| register | read op | rows |
|---|---|---|
| STAT (FF41) | `LD A,($FF41)` `CP $8x` | stat_006 007 027 070 120 121 141 184 235 |
| OAM (FE35) | `LD A,($FE35)` `CP $FF` | oam_006 070 120 184 234 |
| VRAM (8059) | `LD A,($8059)` `CP $FF` | vram_026 070 140 184 |
| LY (FF44) | `LD A,($FF44)` | ly_120 234 |

Each ROM is a NOP sled (name digits = the machine cycle) then ONE direct read
and a `CP`/verdict. **The ROMs never write any PPU register** — they read the
pristine boot hand-off frame. (The reported FF80/FF81 flip per pass/fail branch;
only FF82 is authoritative — the OCR "got==want==FF" rows `oam_070`/`184`/
`vram_070`/`184` actually want the byte ACCESSIBLE, not FF.)

## The ground truth (dual-traced this session, slopgb `SLOPGB_S5DBG` ↔ want)

Every boot read lands at `frame_count == 2` (the first game frame; the boot
warmup crosses line 144 once). slopgb's tier2 deferred read samples cc+0 (the
M-cycle leading edge); the reads land **exactly 4 dots before a boot mode
transition**, so the cc+0 sample returns the pre-transition value:

```
row         slopgb read (cc+0)          got  want  dot+4 lands
stat_006    ly153 d452  (VBlank m1)      85   84    ly0  d0   line-start m0 + LYC0 coinc
stat_007    ly0   d0    (line-start m0)  84   86    ly0  d4   mode 2
stat_027    ly0   d80   (mode 2)         86   87    ly0  d84  mode 3 (entry 84)
stat_070    ly0   d252  (mode 3)         87   84    ly0  d256 mode 0 (past flip R)
stat_120    ly0   d452  (m0 + LYC0)      84   80    ly1  d0   m0, LYC0 clears (ly1≠0)
stat_235    ly2   d0                     80   82    ly2  d4   mode 2
oam_006     ly153 d452  (accessible)     00   FF    ly0  d0   OAM locked (mode 2 scan)
oam_070     ly0   d252  (blocked FF)     FF   acc.  ly0  d256 OAM released (mode 0)
vram_026    ly0   d76   (accessible 00)  00   FF    ly0  d80  VRAM lock (mode 3, at 80)
vram_070    ly0   d252  (blocked FF)     FF   acc.  ly0  d256 VRAM released (mode 0)
ly_120      ly0   d452  (LY=0)           00   01    ly1  d0   LY=1
```

**ONE uniform law fits STAT ∧ OAM ∧ VRAM ∧ LY:** observe the read at its true
cc+4 position — the current (line, dot) advanced **+4 dots** on the 154×456
grid. slopgb's own `vis_mode()` already has the boundaries at dot 4 (m0→m2),
`mode3_entry_dot()`=84 (m2→m3), the projected flip R (m3→m0) and the line wrap;
the reads sit 4 dots shy of each. The three access widths differ exactly as
`blocking.rs`: OAM locks from the line start (mode 2+3), VRAM from dot 80 (mode
3), STAT shows the dot-0-3 line-start mode-0 hold; the LYC-coincidence bit and
LY both follow `self.ly` (which carries the line-153 LY=0 quirk — `ly_000`/
`stat_000` read line 153 late where `self.ly==0`, want LY 0 / coincidence set).

## The build (`Ppu::boot_read`, `ppu/stat_irq/read_laws.rs`)

`boot_read(addr) -> Option<u8>` recomputes the STAT byte / OAM+VRAM
accessibility / LY at [`boot_shift4`] (the (line,dot)+4 position), consumed by
the deferred read path in `interconnect/cycle.rs`. **Verdict-only** — no
counter, dispatch, or DIV moves. Scoped to:

- `tier2_reclock` + `!is_cgb` — CGB's boot hand-off is a separate frame; CGB
  byte-identical (two-bin zero-drift, `boot_read` returns `None`).
- `frame_count <= 2` — the boot window; `frame_count` is monotonic from
  power-on (never resets, even across LCD off/on), so the arm fires once.
- **`!lcd_regs_written`** — the discriminator that isolates poweron from every
  OTHER early reader. The `poweron_*` ROMs write no PPU register; `lcdon_to_*`/
  `oam_read`/`vram_read`/`sprite`/`win` toggle the LCD (FF40) and the gambatte
  kernel/halt STAT-ISR tests arm a mode interrupt (FF41), so a CPU write to
  FF40-FF4B (tier2 write path, `cycle.rs`) trips the flag and reverts those
  reads to cc+0. Without it the arm regressed 55 frame-≤2 rows (measured); with
  it the full 513-row DMG matrix is **+20 / 0-regressed**.

## The CRUX — separable from the `+4` boot DIV (the session's decision point)

The `+4` boot DIV (`interconnect/boot.rs`, `div += 4` under tier2) is the TIMER
counter phase; `boot_div` (9 legs) + `boot_sclk_align` (2) depend on it. The
boot READ law touches only the PPU sample position of FF41/FF44/OAM/VRAM reads
— a different subsystem. **`tier2_boot_div_passes` stays green with the arm**,
proving separability (the honorable-ship endpoint, not the park endpoint). The
counter-pinned IRQ dispatch is likewise untouched (verdict-only) — mooneye
`intr_2_*` hold (91/91 flag-on, the B=42 counter-pin).

## Gates (all green)

- **gbmicrotest DMG flag-on 425 → 445 (+20, 0 of 513 regressed)** — clean two-bin
  vs the base binary (md5 `f608…`).
- `tier2_boot_div_passes` ✓ (the CRUX) + all **53 → 54 tier2 pins** (new
  `tier2_dmg_poweron_passes`, 20 rows, 3× stable).
- mooneye 91/91 flag-on (`SLOPGB_MOONEYE_RECLOCK=1`) AND flag-off.
- CGB two-bin 291/291 zero-drift (`!is_cgb`); lib 660; clippy `-D warnings` clean;
  full gbtr OFF byte-identical.

## STRETCH

The 8 non-window DMG-OCR singles (`C3-FLIP-CHECKLIST.md` §3b) were NOT attempted
this session (phase-1 endpoint reached cleanly; time/scope). They remain the
next §3b lever.
