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

    ; --- all 8 CGB BG palettes start: index0 white, indices 1..3 black (the
    ; logo letters start dark; the colored wipe lights each palette's hue in turn)
    ld a, $80                    ; auto-increment from index 0
    ldh [rBGPI], a
    ld c, 8                      ; 8 palettes
.palOuter:
    ld a, $FF                    ; index0 white lo ($7FFF)
    ldh [rBGPD], a
    ld a, $7F
    ldh [rBGPD], a
    xor a                        ; indices 1..3 = black
    ld b, 6
.palInner:
    ldh [rBGPD], a
    dec b
    jr nz, .palInner
    dec c
    jr nz, .palOuter

    ; --- build the BG tilemap (bank 0): the 11x2 logo centred at col 4, row 8 ---
    ld hl, $9800 + 8*32 + 4
    ld d, 1                      ; first logo tile index
    ld c, LOGO_ROWS
.maprow:
    ld b, LOGO_COLS
.mapcol:
    ld a, d
    ld [hl+], a
    inc d
    dec b
    jr nz, .mapcol
    push bc
    ld bc, 32 - LOGO_COLS
    add hl, bc
    pop bc
    dec c
    jr nz, .maprow

    ; --- BG attribute map (bank 1): each logo column uses palette = column index
    ; (0..7, the rightmost columns capped at 7) so colour can wipe across them ---
    ld a, 1
    ldh [rVBK], a
    ld hl, $9800 + 8*32 + 4
    ld c, LOGO_ROWS
.attrrow:
    ld b, 0                      ; column counter -> palette index
.attrcol:
    ld a, b
    cp 8
    jr c, .attrok
    ld a, 7                      ; cap at palette 7
.attrok:
    ld [hl+], a
    inc b
    ld a, b
    cp LOGO_COLS
    jr nz, .attrcol
    push bc
    ld bc, 32 - LOGO_COLS
    add hl, bc
    pop bc
    dec c
    jr nz, .attrrow
    xor a
    ldh [rVBK], a                ; back to bank 0

    ; --- LCD on: BG enabled, $8000 tile data, $9800 BG map ---
    ld a, $91
    ldh [rLCDC], a

    ; --- CGB colored wipe: light each palette's hue left-to-right ---
    ld c, 0                      ; palette index 0..7
.wipe:
    call SetHue                  ; palette C := Hues[C]
    ld b, 8                      ; ~8 frames per column band
.wipeWait:
    call WaitFrame
    dec b
    jr nz, .wipeWait
    inc c
    ld a, c
    cp 8
    jr nz, .wipe

    ; --- P3: hold on the colored logo (chime + hand-off come later) ---
.hold:
    jr .hold

; Set CGB BG palette C's letter colours (indices 1..3) to Hues[C].
SetHue:
    ld a, c
    add a, a
    ld e, a                      ; E = C*2 (index into 2-byte Hues table)
    ld d, 0
    ld hl, Hues
    add hl, de                   ; HL -> Hues[C]
    ld a, c
    add a, a
    add a, a
    add a, a                     ; A = C*8 (palette base)
    add a, 2                     ; +2 -> palette index 1
    or $80                       ; auto-increment
    ldh [rBGPI], a
    ld a, [hl+]                  ; colour lo
    ld d, a
    ld e, [hl]                   ; colour hi
    ; write the colour to indices 1,2,3
    ld b, 3
.sh:
    ld a, d
    ldh [rBGPD], a
    ld a, e
    ldh [rBGPD], a
    dec b
    jr nz, .sh
    ret

; Wait for one frame (one rising edge of v-blank, LY 143 -> 144).
WaitFrame:
.notVbl:
    ldh a, [rLY]
    cp 144
    jr nz, .notVbl
.inVbl:
    ldh a, [rLY]
    cp 144
    jr z, .inVbl
    ret

; 8 rainbow hues (BGR555 little-endian): red, orange, yellow, green, cyan,
; blue, indigo, magenta — the colour the wipe paints the logo letters.
Hues:
    dw $001F, $01FF, $03FF, $03E0, $7FE0, $7C00, $7C0F, $7C1F

SECTION "logo", ROM0[$0300]
LogoTiles:
    INCBIN "logo.2bpp"

SECTION "tail", ROM0[$08FF]
    db $00
