#!/usr/bin/env bash
# build_sameboy_tracers.sh — reconstruct the SameBoy 1.0.2 ground-truth tester
# with the slopgb-port tracers (SBMODE / SBREAD ff41+ff0f / SBLEVEL / STAT_IRQ),
# from the pinned yay-cache tarball. Idempotent; survives /tmp wipes (tmpfiles).
#
# The tracers are gated on the SB_TRACE env var (zero overhead unset). They print
# the PPU half-dot position at every visible-mode change (SBMODE), FF41/FF0F read
# (SBREAD, after sync_ppu — the cc+0 read position), STAT line edge (SBLEVEL) and
# IF|=2 dispatch (STAT_IRQ). cfl = cycles_for_line (DOTS); dc = display_cycles
# (8MHz HALF-dots).
#
# `fp=` (#11ay) is the ABSOLUTE MONOTONIC 8MHz clock:
# `fp = absolute_debugger_ticks - display_cycles` (the half-dots the display
# coroutine has actually consumed). USE `fp`, NOT `cfl*2+dc`, as the time axis
# when comparing reads/flips across a mode transition or a line boundary:
# `cfl*2+dc` RESETS per line (`display.c` cfl=0) and is CONSERVED across the
# mode-0 flip (cfl+=k while a GB_SLEEP drives dc -=2k), so it maps two genuinely
# different instants to the same number and has repeatedly produced FALSE
# "co-temporal" verdicts (#11i/#11ap/#11ax, all refuted by `fp` in #11ay —
# `measurements/c2-absclock-cotemporal-refuted-2026-07-01.md`).
#
# Usage:  docs/sameboy-port/tools/build_sameboy_tracers.sh
# Output: $SBBUILD/SameBoy-1.0.2/build/bin/tester/sameboy_tester
#         (persistent cache by default; override SBBUILD=. The old /tmp/sbbuild
#          location was wiped between sessions — the whole classify protocol
#          depends on this binary, so it must survive.)
set -euo pipefail
SRCTGZ="$HOME/.cache/yay/sameboy/sameboy-1.0.2.tar.gz"
SBBUILD="${SBBUILD:-$HOME/.cache/sbbuild}"
DIR="$SBBUILD/SameBoy-1.0.2"
TESTER="$DIR/build/bin/tester/sameboy_tester"

# Guard keys on the #11ay `fp=` field, not just SBMODE, so a tree patched with
# the pre-#11ay (cfl*2+dc-only) tracers is re-patched rather than skipped.
if [ -x "$TESTER" ] && grep -q 'SBMODE ly=%d cfl=%d dc=%d vis=%d fp=' "$DIR/Core/display.c" 2>/dev/null \
   && grep -q SBDISP "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q SBPALR "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBWSCX "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBWLCDC "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBSTOP "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q 'SBWAKE.*fp=' "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q 'SBDISP.*fp=' "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q SBWHDMA "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBSPD "$DIR/Core/timing.c" 2>/dev/null \
   && grep -q SBWRITE "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBACK "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q 'SBREAD ff0f.*fp=' "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBIF "$DIR/Core/display.c" 2>/dev/null; then
  echo "tester already built + patched: $TESTER"; exit 0
fi

[ -f "$SRCTGZ" ] || { echo "MISSING $SRCTGZ — adjust path"; exit 1; }
mkdir -p "$SBBUILD" && cd "$SBBUILD"
[ -d "$DIR/Core" ] || tar xzf "$SRCTGZ"

python3 - "$DIR" <<'PY'
import sys
d=sys.argv[1]
disp=open(d+"/Core/display.c").read()
if "SBMODE" not in disp:
    disp=disp.replace(
"""void GB_STAT_update(GB_gameboy_t *gb)
{
    if (!(gb->io_registers[GB_IO_LCDC] & GB_LCDC_ENABLE)) return;
    if (GB_is_dma_active(gb) && (gb->io_registers[GB_IO_STAT] & 3) == 2) {""",
"""void GB_STAT_update(GB_gameboy_t *gb)
{
    if (!(gb->io_registers[GB_IO_LCDC] & GB_LCDC_ENABLE)) return;
    { static int trc=-1,pm=-1,pl=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
      if(trc){ int m=gb->io_registers[GB_IO_STAT]&3;
        if(m!=pm||gb->current_line!=pl){ fprintf(stderr,"SBMODE ly=%d cfl=%d dc=%d vis=%d fp=%lld\\n",
          gb->current_line, gb->cycles_for_line, gb->display_cycles, m, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); pm=m; pl=gb->current_line; } } }
    if (GB_is_dma_active(gb) && (gb->io_registers[GB_IO_STAT] & 3) == 2) {""")
    disp=disp.replace(
"""    if (gb->stat_interrupt_line && !previous_interrupt_line) {
        gb->io_registers[GB_IO_IF] |= 2;
    }
}""",
"""    { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
      if(trc && gb->stat_interrupt_line != previous_interrupt_line)
        fprintf(stderr,"SBLEVEL ly=%d cfl=%d dc=%d %d->%d mfi=%d lyc_line=%d stat=%02x\\n",
          gb->current_line, gb->cycles_for_line, gb->display_cycles, previous_interrupt_line,
          gb->stat_interrupt_line, (int8_t)gb->mode_for_interrupt, gb->lyc_interrupt_line,
          gb->io_registers[GB_IO_STAT]); }
    if (gb->stat_interrupt_line && !previous_interrupt_line) {
        { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
          if(trc) fprintf(stderr,"SBTRACE STAT_IRQ ly=%d cfl=%d dc=%d mfi=%d stat=%02x lyc_line=%d\\n",
            gb->current_line, gb->cycles_for_line, gb->display_cycles,
            (int8_t)gb->mode_for_interrupt, gb->io_registers[GB_IO_STAT], gb->lyc_interrupt_line); }
        gb->io_registers[GB_IO_IF] |= 2;
    }
}""")
    open(d+"/Core/display.c","w").write(disp)
    print("patched display.c")

mem=open(d+"/Core/memory.c").read()
if "SBREAD ff41" not in mem:
    mem=mem.replace(
"""            case GB_IO_IF:
                return gb->io_registers[GB_IO_IF] | 0xE0;
            case GB_IO_TAC:
                return gb->io_registers[GB_IO_TAC] | 0xF8;
            case GB_IO_STAT:
                return gb->io_registers[GB_IO_STAT] | 0x80;""",
"""            case GB_IO_IF:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBREAD ff0f ly=%d cfl=%d dc=%d if=%02x\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->io_registers[GB_IO_IF]&0x1f); }
                return gb->io_registers[GB_IO_IF] | 0xE0;
            case GB_IO_TAC:
                return gb->io_registers[GB_IO_TAC] | 0xF8;
            case GB_IO_STAT:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBREAD ff41 ly=%d cfl=%d dc=%d mode=%d fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->io_registers[GB_IO_STAT]&3, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                return gb->io_registers[GB_IO_STAT] | 0x80;""")
    open(d+"/Core/memory.c","w").write(mem)
    print("patched memory.c")

# C2 #11ax: CGB palette (BGPD/OBPD = FF69/FF6B) access tracer — SBPALR (read)
# + SBPALW (write) print cgb_palettes_blocked at every access, the ground truth
# for the cgbpal_m3 / enable_display late-cgbpw palette-lock lcd_offset inversion.
mem2=open(d+"/Core/memory.c").read()
if "SBPALR" not in mem2:
    # read side (read_high_memory): the accessible-return path
    mem2=mem2.replace(
"""            case GB_IO_BGPD:
            case GB_IO_OBPD:
            {
                if (!gb->cgb_mode && gb->boot_rom_finished) {
                    return 0xFF;
                }
                if (gb->cgb_palettes_blocked) {
                    return 0xFF;
                }""",
"""            case GB_IO_BGPD:
            case GB_IO_OBPD:
            {
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBPALR ly=%d cfl=%d dc=%d blocked=%d fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->cgb_palettes_blocked, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                if (!gb->cgb_mode && gb->boot_rom_finished) {
                    return 0xFF;
                }
                if (gb->cgb_palettes_blocked) {
                    return 0xFF;
                }""")
    # write side (write_high_memory): just before the blocked branch
    mem2=mem2.replace(
"""                uint8_t index_reg = (addr & 0xFF) - 1;
                if (gb->cgb_palettes_blocked) {
                    if (gb->io_registers[index_reg] & 0x80) {""",
"""                uint8_t index_reg = (addr & 0xFF) - 1;
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBPALW ly=%d cfl=%d dc=%d blocked=%d val=%02x\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->cgb_palettes_blocked, value); }
                if (gb->cgb_palettes_blocked) {
                    if (gb->io_registers[index_reg] & 0x80) {""")
    open(d+"/Core/memory.c","w").write(mem2)
    print("patched memory.c (SBPALR/SBPALW)")

# C2 #11ay: SCX (FF43) write tracer — SBWSCX prints the write's fp + value, the
# ground truth for the late_scx4 / scx_during_m3 render-LENGTH families (the SCX
# write's timing vs the fine-scroll drop decides the mode-3 penalty).
mem3=open(d+"/Core/memory.c").read()
if "SBWSCX" not in mem3:
    mem3=mem3.replace(
"""            case GB_IO_IF:
            case GB_IO_SCX:
            case GB_IO_SCY:
            case GB_IO_BGP:
            case GB_IO_OBP0:
            case GB_IO_OBP1:
            case GB_IO_SB:
            case GB_IO_PSWX:
            case GB_IO_PSWY:
            case GB_IO_PSW:
            case GB_IO_PGB:
                gb->io_registers[addr & 0xFF] = value;
                return;""",
"""            case GB_IO_IF:
            case GB_IO_SCX:
            case GB_IO_SCY:
            case GB_IO_BGP:
            case GB_IO_OBP0:
            case GB_IO_OBP1:
            case GB_IO_SB:
            case GB_IO_PSWX:
            case GB_IO_PSWY:
            case GB_IO_PSW:
            case GB_IO_PGB:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc && (addr&0xFF)==GB_IO_SCX) fprintf(stderr,"SBWSCX ly=%d cfl=%d dc=%d val=%02x fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[addr & 0xFF] = value;
                return;""")
    open(d+"/Core/memory.c","w").write(mem3)
    print("patched memory.c (SBWSCX)")

# #11bd: the LCD-phase-origin tracers — SBWLCDC (every FF40 write: old/new value,
# double_speed_alignment, speed, fp), SBWKEY1 (FF4D arm), SBSTOP (the STOP
# speed-switch decision point: direction, alignment&7, interrupt_pending, fp) and
# SBSPD (the actual cgb_double_speed flip instants in GB_advance_cycles, fp).
# Together they pin WHERE the lcd_offset ROMs' CPU<->PPU phase establishes
# (enable-vs-switch sequencing) for the item-1 phase representation.
mem4=open(d+"/Core/memory.c").read()
if "SBWLCDC" not in mem4:
    mem4=mem4.replace(
"""                gb->io_registers[GB_IO_LCDC] = value;
                gb->wy_check_scheduled = true;
                return;""",
"""                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWLCDC ly=%d cfl=%d dc=%d old=%02x new=%02x dsa=%d ds=%d fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles,
                    gb->io_registers[GB_IO_LCDC], value, gb->double_speed_alignment,
                    gb->cgb_double_speed, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[GB_IO_LCDC] = value;
                gb->wy_check_scheduled = true;
                return;""")
    mem4=mem4.replace(
"""            case GB_IO_KEY1:
                if (!gb->cgb_mode) {
                    return;
                }
                gb->io_registers[GB_IO_KEY1] = value;
                return;""",
"""            case GB_IO_KEY1:
                if (!gb->cgb_mode) {
                    return;
                }
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWKEY1 ly=%d cfl=%d dc=%d val=%02x fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value,
                    (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[GB_IO_KEY1] = value;
                return;""")
    open(d+"/Core/memory.c","w").write(mem4)
    print("patched memory.c (SBWLCDC/SBWKEY1)")

tim=open(d+"/Core/timing.c").read()
if "SBSPD" not in tim:
    tim=tim.replace(
"""        if (gb->speed_switch_countdown == cycles) {
            gb->cgb_double_speed ^= true;
            gb->speed_switch_countdown = 0;
        }""",
"""        if (gb->speed_switch_countdown == cycles) {
            gb->cgb_double_speed ^= true;
            { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
              if(trc) fprintf(stderr,"SBSPD ly=%d cfl=%d dc=%d ds=%d dsa=%d fp=%lld\\n",
                gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->cgb_double_speed,
                gb->double_speed_alignment, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
            gb->speed_switch_countdown = 0;
        }""")
    tim=tim.replace(
"""            GB_advance_cycles(gb, old_cycles);
            gb->cgb_double_speed ^= true;
        }""",
"""            GB_advance_cycles(gb, old_cycles);
            gb->cgb_double_speed ^= true;
            { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
              if(trc) fprintf(stderr,"SBSPD ly=%d cfl=%d dc=%d ds=%d dsa=%d fp=%lld\\n",
                gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->cgb_double_speed,
                gb->double_speed_alignment, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
        }""")
    open(d+"/Core/timing.c","w").write(tim)
    print("patched timing.c (SBSPD)")

# sm83_cpu.c — the per-ISR dispatch (SBDISP, at the vector-PC latch) + the
# halt-wake sample position (SBWAKE), for the #11aq read-position-carry trace.
cpu0=open(d+"/Core/sm83_cpu.c").read()
if "SBSTOP" not in cpu0:
    cpu0=cpu0.replace(
"""    if (speed_switch) {
        flush_pending_cycles(gb);
        
        if (gb->io_registers[GB_IO_LCDC] & GB_LCDC_ENABLE && gb->cgb_double_speed) {""",
"""    if (speed_switch) {
        flush_pending_cycles(gb);
        { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
          if(trc) fprintf(stderr,"SBSTOP ly=%d cfl=%d dc=%d from_ds=%d dsa=%d dsa7=%d ip=%d fp=%lld\\n",
            gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->cgb_double_speed,
            gb->double_speed_alignment, gb->double_speed_alignment & 7, interrupt_pending,
            (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
        if (gb->io_registers[GB_IO_LCDC] & GB_LCDC_ENABLE && gb->cgb_double_speed) {""")
    open(d+"/Core/sm83_cpu.c","w").write(cpu0)
    print("patched sm83_cpu.c (SBSTOP)")

cpu=open(d+"/Core/sm83_cpu.c").read()
if "SBDISP" not in cpu:
    cpu=cpu.replace(
"""            gb->io_registers[GB_IO_IF] &= ~(1 << interrupt_bit);
            gb->pc = interrupt_bit * 8 + 0x40;""",
"""            gb->io_registers[GB_IO_IF] &= ~(1 << interrupt_bit);
            gb->pc = interrupt_bit * 8 + 0x40;
            { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
              if(trc) fprintf(stderr,"SBDISP ly=%d cfl=%d dc=%d bit=%d stat=%02x mfi=%d\\n",
                gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_bit,
                gb->io_registers[GB_IO_STAT]&0x7f, (int8_t)gb->mode_for_interrupt); }""")
    cpu=cpu.replace(
"""    if (gb->halted) {
        GB_advance_cycles(gb, (GB_is_cgb(gb) || gb->just_halted) ? 4 : 2);
    }""",
"""    if (gb->halted) {
        { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
          if(trc && interrupt_queue) fprintf(stderr,"SBWAKE ly=%d cfl=%d dc=%d iq=%02x stat=%02x\\n",
            gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_queue,
            gb->io_registers[GB_IO_STAT]&0x7f); }
        GB_advance_cycles(gb, (GB_is_cgb(gb) || gb->just_halted) ? 4 : 2);
    }""")
    open(d+"/Core/sm83_cpu.c","w").write(cpu)
    print("patched sm83_cpu.c")

# #11bf — upgrade SBWAKE/SBDISP with the absolute `fp=` clock (the #11ay axis):
# SBWAKE prints at the halt-loop iq sample (nonzero iq = the wake decision
# instant); SBDISP at the vector-PC latch. Keyed on the fp-less old format so a
# pre-#11bf tree is re-patched in place.
cpu=open(d+"/Core/sm83_cpu.c").read()
if 'SBWAKE ly=%d cfl=%d dc=%d iq=%02x stat=%02x\\n' in cpu:
    cpu=cpu.replace(
'"SBWAKE ly=%d cfl=%d dc=%d iq=%02x stat=%02x\\n",\n            gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_queue,\n            gb->io_registers[GB_IO_STAT]&0x7f); }',
'"SBWAKE ly=%d cfl=%d dc=%d iq=%02x stat=%02x jh=%d fp=%lld\\n",\n            gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_queue,\n            gb->io_registers[GB_IO_STAT]&0x7f, gb->just_halted, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }')
    open(d+"/Core/sm83_cpu.c","w").write(cpu)
    print("patched sm83_cpu.c (SBWAKE fp)")
# #11bf item 2a — SBWHDMA: FF51-FF55 writes + the HDMA run's block boundaries,
# all with the absolute `fp=` clock (the hdma_late_* write-instant vs
# hblank-transfer-trigger race + the gdma_cycles S6 commit position).
mem=open(d+"/Core/memory.c").read()
if "SBWHDMA" not in mem:
    mem=mem.replace(
"""            case GB_IO_HDMA5:
                if (!gb->cgb_mode) return;
                gb->hdma_steps_left = (value & 0x7F) + 1;""",
"""            case GB_IO_HDMA5:
                if (!gb->cgb_mode) return;
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWHDMA w55 ly=%d cfl=%d dc=%d val=%02x src=%04x dst=%04x fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value,
                    gb->hdma_current_src, gb->hdma_current_dest,
                    (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->hdma_steps_left = (value & 0x7F) + 1;""")
    for reg,name in (("GB_IO_HDMA1","w51"),("GB_IO_HDMA2","w52"),("GB_IO_HDMA3","w53"),("GB_IO_HDMA4","w54")):
        mem=mem.replace(
"""            case %s:
                if (gb->cgb_mode) {""" % reg,
"""            case %s:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWHDMA %s ly=%%d cfl=%%d dc=%%d val=%%02x fp=%%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value,
                    (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                if (gb->cgb_mode) {""" % (reg, name))
    mem=mem.replace(
"""    gb->addr_for_hdma_conflict = 0xFFFF;
    uint16_t vram_base = gb->cgb_vram_bank? 0x2000 : 0;
    gb->hdma_in_progress = true;""",
"""    gb->addr_for_hdma_conflict = 0xFFFF;
    uint16_t vram_base = gb->cgb_vram_bank? 0x2000 : 0;
    { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
      if(trc) fprintf(stderr,"SBWHDMA run ly=%d cfl=%d dc=%d src=%04x dst=%04x steps=%d fp=%lld\\n",
        gb->current_line, gb->cycles_for_line, gb->display_cycles,
        gb->hdma_current_src, gb->hdma_current_dest, gb->hdma_steps_left,
        (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
    gb->hdma_in_progress = true;""")
    mem=mem.replace(
"""    gb->hdma_in_progress = false; // TODO: timing? (affects VRAM reads)""",
"""    { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
      if(trc) fprintf(stderr,"SBWHDMA end ly=%d cfl=%d dc=%d src=%04x dst=%04x steps=%d fp=%lld\\n",
        gb->current_line, gb->cycles_for_line, gb->display_cycles,
        gb->hdma_current_src, gb->hdma_current_dest, gb->hdma_steps_left,
        (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
    gb->hdma_in_progress = false; // TODO: timing? (affects VRAM reads)""")
    open(d+"/Core/memory.c","w").write(mem)
    print("patched memory.c (SBWHDMA)")

cpu=open(d+"/Core/sm83_cpu.c").read()
if 'SBDISP ly=%d cfl=%d dc=%d bit=%d stat=%02x mfi=%d\\n' in cpu:
    cpu=cpu.replace(
'"SBDISP ly=%d cfl=%d dc=%d bit=%d stat=%02x mfi=%d\\n",\n                gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_bit,\n                gb->io_registers[GB_IO_STAT]&0x7f, (int8_t)gb->mode_for_interrupt); }',
'"SBDISP ly=%d cfl=%d dc=%d bit=%d stat=%02x mfi=%d fp=%lld\\n",\n                gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_bit,\n                gb->io_registers[GB_IO_STAT]&0x7f, (int8_t)gb->mode_for_interrupt, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }')
    open(d+"/Core/sm83_cpu.c","w").write(cpu)
    print("patched sm83_cpu.c (SBDISP fp)")
PY

# #11bg -- SBWRITE: FF41 (STAT) + FF45 (LYC) write instants with lyfc + fp (synced:
# both writes run after the GB_display_sync of the IO-write prelude).
python3 - "$DIR" <<'PYW'
import sys
d=sys.argv[1]
p=d+"/Core/memory.c"
mem=open(p).read()
if "SBWRITE" not in mem:
    old_stat="""            case GB_IO_STAT:
                gb->io_registers[GB_IO_STAT] &= 7;"""
    new_stat="""            case GB_IO_STAT:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWRITE ff41 ly=%d cfl=%d dc=%d val=%02x lyfc=%d fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value,
                    (int)(int16_t)gb->ly_for_comparison, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[GB_IO_STAT] &= 7;"""
    assert old_stat in mem, "STAT write anchor missing"
    mem=mem.replace(old_stat,new_stat,1)
    old_lyc="""            case GB_IO_LYC:
                /* TODO: Probably completely wrong in double speed mode */"""
    new_lyc="""            case GB_IO_LYC:
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBWRITE ff45 ly=%d cfl=%d dc=%d val=%02x lyfc=%d fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, value,
                    (int)(int16_t)gb->ly_for_comparison, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                /* TODO: Probably completely wrong in double speed mode */"""
    assert old_lyc in mem, "LYC write anchor missing"
    mem=mem.replace(old_lyc,new_lyc,1)
    open(p,"w").write(mem)
    print("patched SBWRITE")
PYW

# #11bh -- SBREAD ff0f fp upgrade: the FF0F read gets the absolute fp clock
# (keyed on the fp-less old format so a pre-#11bh tree is re-patched in place).
python3 - "$DIR" <<'PYF'
import sys
d=sys.argv[1]
p=d+"/Core/memory.c"
mem=open(p).read()
if 'SBREAD ff0f ly=%d cfl=%d dc=%d if=%02x\\n' in mem:
    mem=mem.replace(
'"SBREAD ff0f ly=%d cfl=%d dc=%d if=%02x\\n",\n                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->io_registers[GB_IO_IF]&0x1f); }',
'"SBREAD ff0f ly=%d cfl=%d dc=%d if=%02x fp=%lld\\n",\n                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->io_registers[GB_IO_IF]&0x1f, (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }')
    open(p,"w").write(mem)
    print("patched SBREAD ff0f fp")
PYF

# #11bh -- SBACK: the dispatch IF-acknowledge instant (sm83_cpu.c vector pick:
# pending_cycles-2 flush has JUST run, so fp here is SYNCED to the exact ack
# instant, unlike SBDISP whose fp context is the post-latch print). Prints the
# pre-clear IF + IE. SBIF: every IF|=2 raise in display.c with SYNCED fp
# (coroutine context, the SBMODE precedent): "su" = GB_STAT_update:572,
# "m1oam" = the ly144 dot-2 STAT&0x20 raise (:2175), "vbl"/"vbloam" = the
# vblank-entry IF|=1 + IF|=2 pair (:2190-2192).
python3 - "$DIR" <<'PYA'
import sys
d=sys.argv[1]
p=d+"/Core/sm83_cpu.c"
cpu=open(p).read()
if "SBACK" not in cpu:
    old="""            gb->pending_cycles = 2;
            gb->io_registers[GB_IO_IF] &= ~(1 << interrupt_bit);"""
    new="""            gb->pending_cycles = 2;
            { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
              if(trc) fprintf(stderr,"SBACK ly=%d cfl=%d dc=%d bit=%d if=%02x ie=%02x fp=%lld\\n",
                gb->current_line, gb->cycles_for_line, gb->display_cycles, interrupt_bit,
                gb->io_registers[GB_IO_IF]&0x1f, gb->interrupt_enable&0x1f,
                (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
            gb->io_registers[GB_IO_IF] &= ~(1 << interrupt_bit);"""
    assert old in cpu, "SBACK anchor missing"
    cpu=cpu.replace(old,new,1)
    open(p,"w").write(cpu)
    print("patched SBACK")
p=d+"/Core/display.c"
disp=open(p).read()
if "SBIF" not in disp:
    old="""            (int8_t)gb->mode_for_interrupt, gb->io_registers[GB_IO_STAT], gb->lyc_interrupt_line); }
        gb->io_registers[GB_IO_IF] |= 2;"""
    new="""            (int8_t)gb->mode_for_interrupt, gb->io_registers[GB_IO_STAT], gb->lyc_interrupt_line); }
        { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
          if(trc) fprintf(stderr,"SBIF su ly=%d cfl=%d dc=%d mfi=%d lyc_line=%d stat=%02x if=%02x fp=%lld\\n",
            gb->current_line, gb->cycles_for_line, gb->display_cycles, (int8_t)gb->mode_for_interrupt,
            gb->lyc_interrupt_line, gb->io_registers[GB_IO_STAT]&0x7f, gb->io_registers[GB_IO_IF]&0x1f,
            (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
        gb->io_registers[GB_IO_IF] |= 2;"""
    assert old in disp, "SBIF su anchor missing"
    disp=disp.replace(old,new,1)
    old="""            if (gb->current_line == LINES && !gb->stat_interrupt_line && (gb->io_registers[GB_IO_STAT] & 0x20)) {
                gb->io_registers[GB_IO_IF] |= 2;
            }"""
    new="""            if (gb->current_line == LINES && !gb->stat_interrupt_line && (gb->io_registers[GB_IO_STAT] & 0x20)) {
                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBIF m1oam ly=%d cfl=%d dc=%d stat=%02x if=%02x fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles,
                    gb->io_registers[GB_IO_STAT]&0x7f, gb->io_registers[GB_IO_IF]&0x1f,
                    (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[GB_IO_IF] |= 2;
            }"""
    assert old in disp, "SBIF m1oam anchor missing"
    disp=disp.replace(old,new,1)
    old="""                gb->io_registers[GB_IO_IF] |= 1;
                if (!gb->stat_interrupt_line && (gb->io_registers[GB_IO_STAT] & 0x20)) {
                    gb->io_registers[GB_IO_IF] |= 2;
                }"""
    new="""                { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                  if(trc) fprintf(stderr,"SBIF vbl ly=%d cfl=%d dc=%d stat=%02x if=%02x fp=%lld\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles,
                    gb->io_registers[GB_IO_STAT]&0x7f, gb->io_registers[GB_IO_IF]&0x1f,
                    (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                gb->io_registers[GB_IO_IF] |= 1;
                if (!gb->stat_interrupt_line && (gb->io_registers[GB_IO_STAT] & 0x20)) {
                    { static int trc=-1; if(trc<0) trc=getenv("SB_TRACE")?1:0;
                      if(trc) fprintf(stderr,"SBIF vbloam ly=%d cfl=%d dc=%d stat=%02x if=%02x fp=%lld\\n",
                        gb->current_line, gb->cycles_for_line, gb->display_cycles,
                        gb->io_registers[GB_IO_STAT]&0x7f, gb->io_registers[GB_IO_IF]&0x1f,
                        (long long)((long long)gb->absolute_debugger_ticks - gb->display_cycles)); }
                    gb->io_registers[GB_IO_IF] |= 2;
                }"""
    assert old in disp, "SBIF vbl anchor missing"
    disp=disp.replace(old,new,1)
    open(p,"w").write(disp)
    print("patched SBIF (su/m1oam/vbl/vbloam)")
PYA

cd "$DIR" && make tester -j"$(nproc)"
echo "BUILT: $TESTER"
