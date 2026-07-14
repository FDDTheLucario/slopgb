; slopgb SGB boot ROM — original work, written from public hardware
; documentation (Pan Docs "SGB Functions" / "SGB Command Packet" / "Power-Up
; Sequence", the gbdev wiki). 256 bytes, DMG-class. NOT derived from, nor
; containing any bytes of, Nintendo's copyrighted sgb_boot.bin / sgb2_boot.bin.
;
; Memory map while mapped (crates/slopgb-core/src/interconnect/boot_rom.rs):
;   $0000-$00FF  the whole boot ROM (a DMG-class boot ROM is 256 bytes)
;   $0100+       the CART shows through here (its header logo at $0104), not
;                boot ROM — so the boot ROM reads the cart header directly.
; Writing FF50 bit 0 = 1 unmaps the boot ROM; PC then runs the cart at $0100.
;
; What it does, in order (the documented SGB power-on sequence):
;   1. hardware init (stack, APU/LCD off, VRAM clear)
;   2. decompress the cart's Nintendo logo ($0104) into VRAM and show it, with a
;      short scroll — the DMG-family boot animation (the SGB plays its chime on
;      the SNES side, not the GB APU, so no GB channel is triggered here:
;      Pan Docs / mooneye boot_hwio-S leaves NR52 with no channel active)
;   3. the SGB header handshake: transfer the 16-byte cart header to the SNES
;      ICD2 as six pulse-coded command packets ($F1,$F3,$F5,$F7,$F9,$FB), each
;      carrying 14 header bytes + a checksum, over P14/P15 of the joypad port
;      (Pan Docs "SGB Command Packet" / "Multiplayer": $00 reset pulse, then
;      128 data bits LSB-first as $10 = "1" / $20 = "0" pulses each re-armed by
;      $30, then a $20 stop pulse)
;   4. install the documented SGB post-boot CPU register + IO state and hand off
;
; Not modelled (deliberate clean-room simplifications, none change the handshake
; or the post-boot state the emulator/tests observe): the anti-piracy logo
; compare (the checksum path is enough to gate a corrupt cart; a bad logo simply
; renders wrong), and frame-exact scroll/chime choreography.

INCLUDE "hardware.inc"

SECTION "entry", ROM0[$0000]
Start:
    ld sp, $FFFE

    ; APU on, channels silent (the SGB boot triggers no GB sound channel).
    ld a, $80
    ldh [rNR52], a
    xor a
    ldh [rNR51], a
    ldh [rNR50], a

    ; LCD off so VRAM is freely writable.
    xor a
    ldh [rLCDC], a

    ; Clear VRAM $8000-$9FFF (odd tile-data bytes stay 0 = the logo's blank
    ; high bitplane; tilemap cells default to tile 0 = blank).
    ld hl, $8000
.clearVram:
    xor a
    ld [hl+], a
    bit 5, h                     ; reached $A000?
    jr z, .clearVram

    ; --- decompress the cart's Nintendo logo ($0104-$0133) into VRAM tiles ---
    ; Each header byte holds two 4-bit columns; each nibble is bit-doubled into
    ; one tile-data byte and written to two rows (the classic DMG logo unpack).
    ; Source in DE, VRAM dest in BC, table lookups via HL.
    ld de, $0104                 ; cart logo, 48 compressed bytes
    ld bc, $8010                 ; VRAM tile 1
.logo:
    ld a, [de]
    ; high nibble -> doubled byte, written to tile rows +0 and +2
    swap a
    and $0F
    call Double
    ld [bc], a
    inc bc
    inc bc
    ld [bc], a
    inc bc
    inc bc
    ; low nibble -> doubled byte, written to tile rows +4 and +6
    ld a, [de]
    and $0F
    call Double
    ld [bc], a
    inc bc
    inc bc
    ld [bc], a
    inc bc
    inc bc
    inc de
    ld a, e
    cp $34                       ; until $0134
    jr nz, .logo

    ; --- BG tilemap: the 24 logo tiles as two centred rows of 12 ---
    ld hl, $9904                 ; row 8, col 4
    ld a, 1
.map1:
    ld [hl+], a
    inc a
    cp 13
    jr nz, .map1                 ; tiles 1..12
    ld hl, $9924                 ; row 9, col 4
.map2:
    ld [hl+], a
    inc a
    cp 25
    jr nz, .map2                 ; tiles 13..24

    ; --- palette, initial scroll offset, LCD on ---
    ld a, $FC                    ; BGP: 11 10 01 00
    ldh [rBGP], a
    ld a, $18                    ; start the logo scrolled up, slide it home
    ldh [rSCY], a
    ld a, $91                    ; LCD on, BG on, tile data $8000, map $9800
    ldh [rLCDC], a

    ; --- short scroll: SCY $18 -> 0, one step per frame ---
.scroll:
.vblOn:
    ldh a, [rLY]
    cp 144
    jr c, .vblOn                 ; wait for VBlank (LY >= 144)
    ldh a, [rSCY]
    dec a
    ldh [rSCY], a
.vblOff:
    ldh a, [rLY]
    cp 144
    jr nc, .vblOff               ; wait for the next frame (LY < 144)
    ldh a, [rSCY]
    or a
    jr nz, .scroll               ; until SCY == 0

    ; --- SGB header handshake: six pulse-coded command packets to the SNES ---
    call SgbHandshake
    jp Handoff                   ; skip the helper bodies below

; ---------------------------------------------------------------------------
; Helpers (reached only by `call`/`jp`, never fallen into).
; ---------------------------------------------------------------------------

; Double: bit-double the low nibble of A into a full byte (bit i -> bits 2i,
; 2i+1) via the 16-entry table. Clobbers HL; the ROM lives in page 0 so the
; table's high byte is always 0.
Double:
    add a, LOW(DoubleTable)
    ld l, a
    ld h, 0
    ld a, [hl]
    ret

; SgbHandshake: transfer the cart header to the SNES as six command packets.
; Packet k (0..5) = [ $F1+2k, header[$0104+14k .. +14], checksum ], 16 bytes,
; sent LSB-first through the P1 pulse protocol.
SgbHandshake:
    ld a, $30
    ldh [rP1], a                 ; idle high — arms the packet receiver
    ld hl, $0104                 ; header source pointer
    ld b, 0                      ; packet index 0..5
.packet:
    xor a
    ldh [rP1], a                 ; $00 reset pulse: open the packet
    ld a, $30
    ldh [rP1], a                 ; re-arm for the first data bit
    ; command byte $F1 + 2*b
    ld a, b
    add a, a
    add a, $F1
    ld e, a
    call SgbSendByte
    ; 14 header bytes, running checksum in D
    ld d, 0
    ld c, 14
.byte:
    ld a, [hl+]
    ld e, a
    add a, d
    ld d, a
    call SgbSendByte
    dec c
    jr nz, .byte
    ; checksum byte
    ld e, d
    call SgbSendByte
    ; stop bit + re-arm for the next packet
    ld a, $20
    ldh [rP1], a
    ld a, $30
    ldh [rP1], a
    inc b
    ld a, b
    cp 6
    jr nz, .packet
    ret

; SgbSendByte: pulse the 8 bits of E out LSB-first ($10 = "1", $20 = "0", each
; followed by $30 idle). Preserves B/C/D/HL; clobbers A and E.
SgbSendByte:
    push bc
    ld c, 8
.bit:
    ld a, $20                    ; assume a "0" bit
    rr e                         ; carry = next bit (LSB first)
    jr nc, .pulse
    ld a, $10                    ; "1" bit
.pulse:
    ldh [rP1], a
    ld a, $30
    ldh [rP1], a
    dec c
    jr nz, .bit
    pop bc
    ret

; DoubleTable[n] = n's 4 bits each doubled (e.g. %1011 -> %11001111).
DoubleTable:
    db $00, $03, $0C, $0F, $30, $33, $3C, $3F
    db $C0, $C3, $CC, $CF, $F0, $F3, $FC, $FF

; Pad up to the hand-off block. This also asserts everything above fits below
; $00EF — rgbasm errors on a negative `ds` if the code overflows.
    ds $00EF - @, $00

; --- documented SGB post-boot state + hand-off (mooneye boot_regs-sgb) ---
; A=01 F=00 BC=0014 DE=0000 HL=C060 SP=FFFE; PC=0100 after the FF50 write. The
; `ldh [rBOOT],a` lands at $00FE-$00FF so PC rolls to $0100 = the cart entry.
Handoff:                         ; occupies $00EF-$00FF
    ld sp, $FFFE
    ld bc, $0014
    ld de, $0000
    ld hl, $C060
    ld a, $01
    or a                         ; A=01, F=00
    ldh [rBOOT], a               ; unmap; PC -> $0100 (cart entry)
