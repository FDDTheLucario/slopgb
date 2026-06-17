; slopgb CGB boot ROM — original work, written from public hardware
; documentation (Pan Docs / gbdev wiki). 2304 bytes, CGB-class.
;
; Memory map while mapped (crates/slopgb-core/src/interconnect/boot_rom.rs):
;   $0000-$00FF  entry/init (+ the FF50 hand-off stub at $00FE)
;   $0100-$01FF  the CART header shows through here (logo + title), not boot ROM
;   $0200-$08FF  main routine + logo tiles + (later) chime/palette data
; Writing FF50 bit 0 = 1 unmaps the boot ROM; PC then runs the cart at $0100.

INCLUDE "hardware.inc"

DEF LOGO_COLS    EQU 11        ; the slopgb logo is 11x2 tiles (88x16 px)
DEF LOGO_ROWS    EQU 2
DEF LOGO_TILES   EQU LOGO_COLS * LOGO_ROWS

SECTION "entry", ROM0[$0000]
Start:
    ld sp, $FFFE

    ; APU on (the chime, a later phase, needs it); channels silent for now.
    ld a, $80
    ldh [rNR52], a
    xor a
    ldh [rNR51], a
    ldh [rNR50], a

    ; LCD off so VRAM is freely writable.
    xor a
    ldh [rLCDC], a

    ; Clear VRAM bank 0 ($8000-$9FFF). Tile 0 ends up all-zero = blank.
    ld a, 0
    ldh [rVBK], a
    ld hl, $8000
.clearVram:
    xor a
    ld [hl+], a
    bit 5, h                     ; reached $A000?
    jr z, .clearVram

    ; Clear OAM.
    ld hl, $FE00
    ld c, $A0
    xor a
.clearOam:
    ld [hl+], a
    dec c
    jr nz, .clearOam

    jp Main

; The hand-off must be the last instruction before $0100: after `ldh [$50],a`
; the boot ROM is unmapped and PC = $0100 = the cart's entry point. A = $11 has
; bit 0 set (disables boot) and is the CGB signature the cart reads.
SECTION "handoff", ROM0[$00FE]
BootEnd:
    ldh [rBOOT], a               ; E0 50 — occupies $00FE-$00FF

SECTION "main", ROM0[$0200]
Main:
    ; --- copy the slopgb logo tiles to VRAM, starting at tile 1 ($8010) ---
    ld de, LogoTiles
    ld hl, $8010                 ; tile 1
    ld bc, LOGO_TILES * 16
.copyLogo:
    ld a, [de]
    ld [hl+], a
    inc de
    dec bc
    ld a, b
    or c
    jr nz, .copyLogo

    ; --- CGB BG palette 0: index0 = white, indices 1..3 = black (BGR555), so the
    ; logo letters show dark whichever shade rgbgfx assigned them ---
    ld a, $80                    ; auto-increment, index 0
    ldh [rBGPI], a
    ld a, $FF                    ; index0 white lo ($7FFF)
    ldh [rBGPD], a
    ld a, $7F                    ; index0 white hi
    ldh [rBGPD], a
    xor a                        ; indices 1..3 = black ($0000)
    ld b, 6
.bgpClear:
    ldh [rBGPD], a
    dec b
    jr nz, .bgpClear

    ; --- build the BG tilemap: place the 11x2 logo centred ---
    ; top-left at col (20-11)/2 = 4, row (18-2)/2 = 8 -> $9800 + 8*32 + 4.
    ld hl, $9800 + 8*32 + 4
    ld d, 1                      ; first logo tile index
    ld c, LOGO_ROWS
.row:
    ld b, LOGO_COLS
.col:
    ld a, d
    ld [hl+], a
    inc d
    dec b
    jr nz, .col
    ; advance hl to the same column on the next tile row (+32 - LOGO_COLS)
    push bc
    ld bc, 32 - LOGO_COLS
    add hl, bc
    pop bc
    dec c
    jr nz, .row

    ; --- LCD on: BG enabled, $8000 tile data, $9800 BG map ---
    ld a, $91
    ldh [rLCDC], a

    ; --- P2: hold on the logo (animation + chime + hand-off come later) ---
.hold:
    jr .hold

SECTION "logo", ROM0[$0300]
LogoTiles:
    INCBIN "logo.2bpp"

SECTION "tail", ROM0[$08FF]
    db $00
