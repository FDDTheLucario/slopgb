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
# Output: /tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester
set -euo pipefail
SRCTGZ="$HOME/.cache/yay/sameboy/sameboy-1.0.2.tar.gz"
DIR=/tmp/sbbuild/SameBoy-1.0.2
TESTER="$DIR/build/bin/tester/sameboy_tester"

# Guard keys on the #11ay `fp=` field, not just SBMODE, so a tree patched with
# the pre-#11ay (cfl*2+dc-only) tracers is re-patched rather than skipped.
if [ -x "$TESTER" ] && grep -q 'SBMODE ly=%d cfl=%d dc=%d vis=%d fp=' "$DIR/Core/display.c" 2>/dev/null \
   && grep -q SBDISP "$DIR/Core/sm83_cpu.c" 2>/dev/null \
   && grep -q SBPALR "$DIR/Core/memory.c" 2>/dev/null \
   && grep -q SBWSCX "$DIR/Core/memory.c" 2>/dev/null; then
  echo "tester already built + patched: $TESTER"; exit 0
fi

[ -f "$SRCTGZ" ] || { echo "MISSING $SRCTGZ — adjust path"; exit 1; }
mkdir -p /tmp/sbbuild && cd /tmp/sbbuild
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

# sm83_cpu.c — the per-ISR dispatch (SBDISP, at the vector-PC latch) + the
# halt-wake sample position (SBWAKE), for the #11aq read-position-carry trace.
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
PY

cd "$DIR" && make tester -j"$(nproc)"
echo "BUILT: $TESTER"
