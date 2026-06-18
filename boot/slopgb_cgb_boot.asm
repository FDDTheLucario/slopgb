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

; CGB compatibility palette tables (factual data, generated — see boot/README.md
; + cgb_palette_extract). Included first so its DEFs are visible to the code.
SECTION "paldata", ROM0[$05E0]
INCLUDE "cgb_palettes.inc"

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

    ; --- the two-tone boot chime ---
    call PlayChime

    ; --- assign the DMG game its CGB compatibility palette, then hand off ---
    call ApplyGamePalette
    ld a, $11                    ; CGB signature + FF50 bit0 (disable boot)
    jp BootEnd                   ; $00FE: ldh [rBOOT],a -> PC = cart $0100

; Pick + install the DMG game's compatibility palette (the reference CGB boot
; ROM's title-checksum scheme; data in cgb_palettes.inc). CGB carts keep their
; own palettes. The cart header shows through at $0100-$01FF while booting.
ApplyGamePalette:
    ld a, [$0143]                ; CGB flag
    bit 7, a
    jr z, .dmg
    ldh [rKEY0], a               ; CGB cart: KEY0 = flag, no compat palette
    ret
.dmg:
    ; LCD off so every CGB palette write lands (mode-3 blocks palette RAM); done
    ; before the lookup so it can't clobber the computed set index in A.
    xor a
    ldh [rLCDC], a
    ; manual override: a held d-pad direction (+ optional A/B) forces a preset
    call ReadCombo               ; C = held-combo byte
    ld a, c
    or a
    jr z, .auto                  ; nothing held -> automatic colorization
    ld hl, CgbCombos
    ld b, CGB_COMBO_COUNT
.combo:
    ld a, [hl+]                  ; combo code
    cp c
    jr z, .comboHit
    inc hl                       ; skip set index
    dec b
    jr nz, .combo
    ; no combo matched -> automatic colorization
.auto:
    ; colorize only Nintendo-licensed carts (else the default palette)
    ld a, [$014B]                ; old licensee
    cp $01
    jr z, .lookup                ; $01 = Nintendo (old)
    cp $33
    jr nz, .default
    ld a, [$0144]                ; new licensee "01" = Nintendo
    cp $30
    jr nz, .default
    ld a, [$0145]
    cp $31
    jr nz, .default
.lookup:
    ; title checksum = sum of the 16 title bytes $0134-$0143
    ld hl, $0134
    ld b, 0
    ld c, 16
.sum:
    ld a, [hl+]
    add b
    ld b, a
    dec c
    jr nz, .sum
    ; scan rules: checksum, 4th-letter ($00 = wildcard), set index (3 bytes)
    ld a, [$0137]
    ld d, a                      ; D = 4th title letter
    ld hl, CgbRules
    ld c, CGB_RULE_COUNT
.scan:
    ld a, [hl+]                  ; rule checksum
    cp b
    jr nz, .skip
    ld a, [hl]                   ; rule 4th letter
    or a
    jr z, .hit                   ; $00 = wildcard
    cp d
    jr z, .hit
.skip:
    inc hl                       ; past letter
    inc hl                       ; past set index
    dec c
    jr nz, .scan
.default:
    ld a, CGB_DEFAULT_SET
    jr .install
.hit:
    inc hl                       ; -> set index
    ld a, [hl]
    jr .install
.comboHit:
    ld a, [hl]                   ; combo set index
.install:
    ; install while still in CGB mode (the data ports reject writes once locked)
    call InstallSet
    ld a, $04
    ldh [rKEY0], a               ; lock DMG-compat (bit 2)
    ld a, $01
    ldh [rOPRI], a               ; DMG object priority
    ret

; Install palette set A: its 3 bytes (BG, OBJ0, OBJ1 palette indices) pick
; palettes from CgbPalettes into BG palette 0 and OBJ palettes 0 and 1.
InstallSet:
    ld c, a
    add a, a
    add a, c                     ; A = set * 3
    ld c, a
    ld b, 0
    ld hl, CgbSets
    add hl, bc                   ; HL = CgbSets + set*3
    ld a, [hl+]
    ldh [$FF80], a               ; BG palette index
    ld a, [hl+]
    ldh [$FF81], a               ; OBJ0 palette index
    ld a, [hl]
    ldh [$FF82], a               ; OBJ1 palette index
    ldh a, [$FF80]
    call PalSrc
    ld a, $80                    ; BG palette 0, colour index 0, auto-increment
    ldh [rBGPI], a
    ld c, LOW(rBGPD)
    call Copy8ToC
    ldh a, [$FF81]
    call PalSrc
    ld a, $80                    ; OBJ palette 0
    ldh [rOBPI], a
    ld c, LOW(rOBPD)
    call Copy8ToC
    ldh a, [$FF82]
    call PalSrc
    ld a, $88                    ; OBJ palette 1 (colour byte 8, auto-increment)
    ldh [rOBPI], a
    ld c, LOW(rOBPD)
    call Copy8ToC
    ret

; A = palette index -> HL = CgbPalettes + A*8 (one palette = 4 BGR555 colours).
PalSrc:
    ld l, a
    ld h, 0
    add hl, hl
    add hl, hl
    add hl, hl
    ld de, CgbPalettes
    add hl, de
    ret

; Copy 8 bytes from HL to the FF00-page data port in C (BGPD/OBPD, auto-inc).
Copy8ToC:
    ld b, 8
.cp:
    ld a, [hl+]
    ldh [c], a
    dec b
    jr nz, .cp
    ret

; Read the joypad into C: d-pad in the high nibble (Up $40, Left $20, Down $80,
; Right $10), A=$01/B=$02 in the low nibble; a held button reads as 1.
ReadCombo:
    ld a, $20
    ldh [rP1], a                 ; select d-pad
    ldh a, [rP1]
    ldh a, [rP1]                 ; let the lines settle
    cpl                          ; pressed = 1
    and $0F
    swap a                       ; d-pad -> high nibble
    ld b, a
    ld a, $10
    ldh [rP1], a                 ; select buttons
    ldh a, [rP1]
    ldh a, [rP1]
    cpl
    and $0F                      ; A=$01, B=$02 in the low nibble
    or b
    ld c, a                      ; C = combo byte
    ld a, $30
    ldh [rP1], a                 ; deselect both
    ret

; The CGB boot chime ("di-ding"), bit-for-bit the same as the reference ROM:
; square channel 1, two rising tones (freq $783 then $7C1, an octave apart) two
; frames apart, sharing the envelope $F3 (vol 15, decrease, period 3). The APU
; register values are the reference boot ROM's exact writes.
PlayChime:
    ld a, $80
    ldh [rNR52], a               ; APU on
    ldh [rNR11], a               ; NR11 = $80: 50% duty, length 0
    ld a, $F3
    ldh [rNR12], a               ; NR12 = $F3: envelope vol 15, decrease, period 3
    ldh [rNR51], a               ; NR51 = $F3: panning (matches reference)
    ld a, $77
    ldh [rNR50], a               ; NR50 = $77: master volume max, both sides
    ; tone 1: frequency $783
    ld a, $83
    ldh [rNR13], a
    ld a, $87
    ldh [rNR14], a               ; trigger + freq hi (7)
    call WaitFrame               ; the reference fires the two tones two steps apart
    call WaitFrame
    ; tone 2: frequency $7C1
    ld a, $C1
    ldh [rNR13], a
    ld a, $87
    ldh [rNR14], a               ; trigger + freq hi (7)
    ld b, 30                      ; let the envelope ring out
.ring:
    call WaitFrame
    dec b
    jr nz, .ring
    ret

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

SECTION "logo", ROM0[$0470]
LogoTiles:
    INCBIN "logo.2bpp"
