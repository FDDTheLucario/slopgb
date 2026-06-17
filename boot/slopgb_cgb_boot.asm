; slopgb CGB boot ROM — original work, written from public hardware
; documentation (Pan Docs / gbdev wiki). 2304 bytes, CGB-class.
;
; Memory map while mapped (see crates/slopgb-core/src/interconnect/boot_rom.rs):
;   $0000-$00FF  this entry/init code (+ the hand-off stub at $00FE)
;   $0100-$01FF  the CART header shows through here (logo + title), not boot ROM
;   $0200-$08FF  continuation (logo tiles, chime, palettes) — later phases
; Writing FF50 bit 0 = 1 unmaps the boot ROM; PC then runs the cart at $0100.

INCLUDE "hardware.inc"

SECTION "entry", ROM0[$0000]
Start:
    ld sp, $FFFE                 ; standard stack

    ; --- silence + reset audio, then enable it (post-boot leaves it on) ---
    ld a, $80
    ldh [rNR52], a               ; APU on (so the later chime can sound)
    xor a
    ldh [rNR51], a               ; no channels routed yet
    ldh [rNR50], a               ; volume 0 for now

    ; --- LCD off so we can safely clear VRAM ---
    xor a
    ldh [rLCDC], a

    ; --- clear VRAM ($8000-$9FFF) ---
    ld hl, $8000
.clearVram:
    xor a
    ld [hl+], a
    bit 5, h                     ; reached $A000 (bit5 of high byte set)?
    jr z, .clearVram

    ; --- clear OAM ($FE00-$FE9F) ---
    ld hl, $FE00
    ld c, $A0
    xor a
.clearOam:
    ld [hl+], a
    dec c
    jr nz, .clearOam

    ; --- a plain monochrome BG palette (compat) ---
    ld a, %11100100
    ldh [rBGP], a
    ldh [rOBP0], a
    ldh [rOBP1], a

    ; --- scroll/window registers to a known state ---
    xor a
    ldh [rSCY], a
    ldh [rSCX], a

    ; --- LCD on: BG enabled, tile data $8000, BG map $9800 ---
    ld a, $91
    ldh [rLCDC], a

HandOff:
    ; A = $11 → bit 0 set disables the boot ROM, and $11 stays in A as the
    ; CGB signature the cart checks. The actual FF50 write lives at $00FE so the
    ; instruction after it is the cart entry at $0100.
    ld a, $11
    jp BootEnd

; The hand-off must be the last instruction before $0100: after `ldh [$50],a`
; the boot ROM is unmapped and PC = $0100 = the cart's entry point.
SECTION "handoff", ROM0[$00FE]
BootEnd:
    ldh [rBOOT], a               ; E0 50 — occupies $00FE-$00FF

; Pad the file out to the full 2304-byte CGB-class size; later phases fill
; $0200-$08FF with real content.
SECTION "tail", ROM0[$08FF]
    db $00
