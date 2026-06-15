#!/usr/bin/env python3
"""Build a tiny GB ROM packed with diverse opcodes at 0x0150 so bgb's debugger
will disassemble each on its own line — giving authoritative no$gmb-syntax text
to validate slopgb's disassembler against. Output: /tmp/disasm_probe.gb.

Run, load in bgb (StartDebug=1), screenshot the disasm pane, compare to the
labelled byte sequence in CASES below."""

import struct, sys

# (bytes, human label) — one representative encoding per interesting form.
CASES = [
    (b"\x00", "nop"),
    (b"\x06\x12", "ld b,12"),
    (b"\x0e\x34", "ld c,34"),
    (b"\x3e\xff", "ld a,ff"),
    (b"\x21\x34\x12", "ld hl,1234"),
    (b"\x01\xbe\xba", "ld bc,babe"),
    (b"\x7e", "ld a,(hl)"),
    (b"\x77", "ld (hl),a"),
    (b"\x46", "ld b,(hl)"),
    (b"\x40", "ld b,b"),
    (b"\x04", "inc b"),
    (b"\x05", "dec b"),
    (b"\x34", "inc (hl)"),
    (b"\x09", "add hl,bc"),
    (b"\x80", "add a,b"),
    (b"\x86", "add a,(hl)"),
    (b"\xc6\x10", "add a,10"),
    (b"\x90", "sub b"),
    (b"\xa0", "and b"),
    (b"\xb8", "cp b"),
    (b"\xfe\x99", "cp 99"),
    (b"\x18\x02", "jr +2"),
    (b"\x20\xfc", "jr nz,-4"),
    (b"\xc3\x50\x01", "jp 0150"),
    (b"\xc2\x50\x01", "jp nz,0150"),
    (b"\xe9", "jp (hl)"),
    (b"\xcd\x50\x01", "call 0150"),
    (b"\xc4\x50\x01", "call nz,0150"),
    (b"\xc9", "ret"),
    (b"\xc0", "ret nz"),
    (b"\xd9", "reti"),
    (b"\xc7", "rst 00"),
    (b"\xff", "rst 38"),
    (b"\xe0\x44", "ldh (44),a"),
    (b"\xf0\x44", "ldh a,(44)"),
    (b"\xe2", "ld (ff00+c),a"),
    (b"\xf2", "ld a,(ff00+c)"),
    (b"\xea\x34\x12", "ld (1234),a"),
    (b"\xfa\x34\x12", "ld a,(1234)"),
    (b"\x08\x34\x12", "ld (1234),sp"),
    (b"\x22", "ld (hl+),a"),
    (b"\x2a", "ld a,(hl+)"),
    (b"\x32", "ld (hl-),a"),
    (b"\xf8\x03", "ld hl,sp+3"),
    (b"\xf9", "ld sp,hl"),
    (b"\xe8\x05", "add sp,5"),
    (b"\xc5", "push bc"),
    (b"\xf1", "pop af"),
    (b"\x07", "rlca"),
    (b"\x17", "rla"),
    (b"\x27", "daa"),
    (b"\x2f", "cpl"),
    (b"\x37", "scf"),
    (b"\x3f", "ccf"),
    (b"\xf3", "di"),
    (b"\xfb", "ei"),
    (b"\x76", "halt"),
    (b"\x10\x00", "stop"),
    (b"\xcb\x7c", "bit 7,h"),
    (b"\xcb\x16", "rl (hl)"),
    (b"\xcb\x30", "swap b"),
    (b"\xcb\x00", "rlc b"),
    (b"\xcb\x3f", "srl a"),
    (b"\xcb\xc0", "set 0,b"),
    (b"\xcb\x86", "res 0,(hl)"),
    (b"\xd3", "db d3 (illegal)"),
    (b"\xdd", "db dd (illegal)"),
]

rom = bytearray(b"\xff" * 0x8000)  # 32 KiB, MBC0
# entry at 0x100: nop; jp 0x150
rom[0x100:0x104] = b"\x00\xc3\x50\x01"
# Nintendo logo (bgb warns if absent; supply real bytes to keep it clean)
LOGO = bytes.fromhex(
    "ceed6666cc0d000b03730083000c000d0008111f8889000edccc6ee6ddddd999"
    "bbbb67636e0eecccdddc999fbbb9333e"
)
rom[0x104:0x104 + len(LOGO)] = LOGO
title = b"DISASMPROBE"
rom[0x134:0x134 + len(title)] = title
rom[0x143] = 0x00  # not CGB
rom[0x147] = 0x00  # ROM ONLY
rom[0x148] = 0x00  # 32 KiB
rom[0x149] = 0x00  # no RAM
# code blob
off = 0x150
for b, _ in CASES:
    rom[off:off + len(b)] = b
    off += len(b)
# header checksum 0x134-0x14C
chk = 0
for i in range(0x134, 0x14D):
    chk = (chk - rom[i] - 1) & 0xFF
rom[0x14D] = chk
# global checksum 0x14E-0x14F (sum of all bytes except these two)
gsum = (sum(rom) - rom[0x14E] - rom[0x14F]) & 0xFFFF
rom[0x14E] = gsum >> 8
rom[0x14F] = gsum & 0xFF

out = "/tmp/disasm_probe.gb"
with open(out, "wb") as f:
    f.write(rom)
print(f"wrote {out} ({len(rom)} bytes), {len(CASES)} cases from 0x150")
print(f"last byte offset 0x{off:04X}")
