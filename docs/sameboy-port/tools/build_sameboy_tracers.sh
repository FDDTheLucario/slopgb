#!/usr/bin/env bash
# build_sameboy_tracers.sh — reconstruct the SameBoy 1.0.2 ground-truth tester
# with the slopgb-port tracers (SBMODE / SBREAD ff41+ff0f / SBLEVEL / STAT_IRQ),
# from the pinned yay-cache tarball. Idempotent; survives /tmp wipes (tmpfiles).
#
# The tracers are gated on the SB_TRACE env var (zero overhead unset). They print
# the PPU half-dot position at every visible-mode change (SBMODE), FF41/FF0F read
# (SBREAD, after sync_ppu — the cc+0 read position), STAT line edge (SBLEVEL) and
# IF|=2 dispatch (STAT_IRQ). cfl = cycles_for_line (DOTS); dc = display_cycles
# (8MHz HALF-dots); the true half-dot line position = cfl*2 + dc (display.c:1584).
#
# Usage:  docs/sameboy-port/tools/build_sameboy_tracers.sh
# Output: /tmp/sbbuild/SameBoy-1.0.2/build/bin/tester/sameboy_tester
set -euo pipefail
SRCTGZ="$HOME/.cache/yay/sameboy/sameboy-1.0.2.tar.gz"
DIR=/tmp/sbbuild/SameBoy-1.0.2
TESTER="$DIR/build/bin/tester/sameboy_tester"

if [ -x "$TESTER" ] && grep -q SBMODE "$DIR/Core/display.c" 2>/dev/null; then
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
        if(m!=pm||gb->current_line!=pl){ fprintf(stderr,"SBMODE ly=%d cfl=%d dc=%d vis=%d\\n",
          gb->current_line, gb->cycles_for_line, gb->display_cycles, m); pm=m; pl=gb->current_line; } } }
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
                  if(trc) fprintf(stderr,"SBREAD ff41 ly=%d cfl=%d dc=%d mode=%d\\n",
                    gb->current_line, gb->cycles_for_line, gb->display_cycles, gb->io_registers[GB_IO_STAT]&3); }
                return gb->io_registers[GB_IO_STAT] | 0x80;""")
    open(d+"/Core/memory.c","w").write(mem)
    print("patched memory.c")
PY

cd "$DIR" && make tester -j"$(nproc)"
echo "BUILT: $TESTER"
