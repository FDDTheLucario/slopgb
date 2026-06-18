; slopgb CGB boot ROM — original work, written from public hardware
; documentation (Pan Docs / gbdev wiki). 2304 bytes, CGB-class.
;
; Memory map while mapped (crates/slopgb-core/src/interconnect/boot_rom.rs):
;   $0000-$00FF  entry/init (+ the FF50 hand-off stub at $00FE)
;   $0100-$01FF  the CART header shows through here (logo + title), not boot ROM
;   $0200-$08FF  main routine + logo tiles + (later) chime/palette data
; Writing FF50 bit 0 = 1 unmaps the boot ROM; PC then runs the cart at $0100.

INCLUDE "hardware.inc"

DEF LOGO_COLS    EQU 11        ; the SLOPGB wordmark is 11x3 tiles (88x24 px)
DEF LOGO_ROWS    EQU 3
DEF LOGO_TILES   EQU LOGO_COLS * LOGO_ROWS
DEF LOGO_LOW_BYTES  EQU 13 * 16    ; first 13 tiles (fit the $0029 boot gap)
DEF LOGO_HIGH_BYTES EQU LOGO_TILES * 16 - LOGO_LOW_BYTES

; CGB compatibility palette tables (factual data, generated — see boot/README.md
; + cgb_palette_extract). Included first so its DEFs are visible to the code.
SECTION "paldata", ROM0
INCLUDE "cgb_palettes.inc"

; The cart header shows through $0100-$01FF while booting; reserve it so the
; linker never places boot code/data there (it would be shadowed at runtime).
SECTION "cartgap", ROM0[$0100]
    ds $100, $00

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
    ; --- copy the SLOPGB logo tiles to VRAM (tile 1, $8010). The logo data is
    ; split across two ROM regions to fit the budget, so copy both parts. ---
    ld de, LogoTilesLow
    ld hl, $8010                 ; tile 1
    ld bc, LOGO_LOW_BYTES
    call CopyBytes
    ld de, LogoTilesHigh
    ld bc, LOGO_HIGH_BYTES
    call CopyBytes               ; HL continues into the next tiles

    ; --- decompress the cart's Nintendo logo into tiles 32+ ($8200) ---
    call DecompressNintendo

    ; --- all 8 BG palettes: index0 white, indices 1..3 black; the per-palette
    ; live letter colour (R,G,B, 0..31 each) is tracked in WRAM at $C000 so the
    ; animation can interpolate it. Start every letter colour black. ---
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
    ld hl, $C000                 ; live RGB per palette (8 x R,G,B), start black
    ld b, 24
    xor a
.clrRgb:
    ld [hl+], a
    dec b
    jr nz, .clrRgb

    ; --- build the BG tilemap (bank 0): the 11x2 logo centred at col 4, row 8 ---
    ld hl, $9800 + 6*32 + 4
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

    ; --- place the Nintendo logo: 2 rows x 12 tiles (34..57) at row 11 ---
    ld hl, $9800 + 13*32 + 4
    ld d, 34                     ; first Nintendo tile
    ld c, 2
.ntmRow:
    ld b, 12
.ntmCol:
    ld a, d
    ld [hl+], a
    inc d
    dec b
    jr nz, .ntmCol
    push bc
    ld bc, 32 - 12
    add hl, bc
    pop bc
    dec c
    jr nz, .ntmRow

    ; --- BG attribute map (bank 1) ---
    ld a, 1
    ldh [rVBK], a
    ; slopgb columns use palette = column index, capped at 6 (the animated band);
    ; palette 7 is reserved for the static-black Nintendo logo.
    ld hl, $9800 + 6*32 + 4
    ld c, LOGO_ROWS
.attrrow:
    ld b, 0                      ; column counter -> palette index
.attrcol:
    ld a, b
    cp 7
    jr c, .attrok
    ld a, 6                      ; cap at palette 6
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
    ; Nintendo logo rows -> palette 7 (static black)
    ld hl, $9800 + 13*32 + 4
    ld c, 2
.nattrRow:
    ld b, 12
.nattrCol:
    ld a, 7
    ld [hl+], a
    dec b
    jr nz, .nattrCol
    push bc
    ld bc, 32 - 12
    add hl, bc
    pop bc
    dec c
    jr nz, .nattrRow
    xor a
    ldh [rVBK], a                ; back to bank 0

    ; --- LCD on: BG enabled, $8000 tile data, $9800 BG map ---
    ld a, $91
    ldh [rLCDC], a

    ; brief hold on just the Nintendo logo before the wordmark wipes in (the
    ; reference shows 'Nintendo' for ~28 frames first)
    ld b, 28
    call DelayFrames

    ; --- Phase 1: reveal the letters left-to-right, each column its rainbow hue
    ld c, 0                      ; palette / column 0..7
.reveal:
    push bc
    ld a, c
    add a, a
    add a, c                     ; A = c*3
    ld e, a
    ld d, 0
    ld hl, HueRGB
    add hl, de                   ; HL -> HueRGB[c]
    ld a, c
    add a, a
    add a, c
    ld e, a
    ld d, $C0                    ; DE = $C000 + c*3 (live colour)
    ld a, [hl+]
    ld [de], a
    inc de
    ld a, [hl+]
    ld [de], a
    inc de
    ld a, [hl]
    ld [de], a
    ld b, c
    call CombineWrite            ; push palette c's colour to BG palette c
    pop bc
    push bc
    ld b, 4                      ; ~4 frames per column
.revWait:
    call WaitFrame
    dec b
    jr nz, .revWait
    pop bc
    inc c
    ld a, c
    cp 7                         ; animate palettes 0..6 (7 is the static logo)
    jr nz, .reveal

    ; --- Phase 2: settle the rainbow into solid blue ---
    ld hl, BlueRGB
    call FadeAll

    ; --- the two-tone boot chime (during the blue hold) ---
    call PlayChime

    ; hold on the blue logo a little longer (the reference dwells ~60 frames)
    ld b, 20
    call DelayFrames

    ; --- Phase 3: fade the logo out to white ---
    ld hl, WhiteRGB
    call FadeAll

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

; Copy BC bytes from DE to HL.
CopyBytes:
    ld a, [de]
    ld [hl+], a
    inc de
    dec bc
    ld a, b
    or c
    jr nz, CopyBytes
    ret

; Decompress the cart's 48-byte Nintendo logo ($0104) into tiles at $8200 by
; doubling each nibble's bits (the standard logo scaler — a functional algorithm;
; the logo data itself lives in the cart, never embedded here). VRAM was cleared
; at power-on, so the unwritten bitplane stays 0 (the logo renders as shade 1).
DecompressNintendo:
    ld de, $0104
    ld hl, $8220                 ; tile 34 (clear of the 33-tile SLOPGB logo)
.dn:
    ld a, [de]
    ld b, a
    call ExpandNib               ; double the high nibble of B
    call ExpandNib               ; double the (rotated) low nibble of B
    inc de
    ld a, e
    cp $34                       ; through $0133
    jr nz, .dn
    ret

; Double the top nibble of B into A (each bit twice), write it to two tile-rows
; plane 0 ([HL] and [HL+2]), advance HL by 4, and shift B left by 4 (the cart
; logo is designed to be doubled both ways).
ExpandNib:
    push de
    ld d, 4
.ex:
    ld e, b
    rl b
    rla
    rl e
    rla
    dec d
    jr nz, .ex
    pop de
    ld [hl+], a
    inc hl
    ld [hl+], a
    inc hl
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
    ; the reference holds tone 1 for ~4 frames before re-triggering ch1 (its
    ; chime loop is two frames per step; measured from the reference audio)
    ld b, 4
.gap:
    call WaitFrame
    dec b
    jr nz, .gap
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

; Write BG palette B's letter colours (indices 1..3) from its live RGB at
; $C000 + B*3, packing the three channels into BGR555.
CombineWrite:
    ld a, b
    add a, a
    add a, b                     ; A = B*3
    ld l, a
    ld h, $C0                    ; HL = $C000 + B*3
    ld a, [hl+]                  ; R
    ld e, a
    ld a, [hl+]                  ; G
    ld d, a
    ld c, [hl]                   ; B (blue channel)
    ; lo = R | ((G & 7) << 5)
    ld a, d
    and $07
    add a, a
    add a, a
    add a, a
    add a, a
    add a, a                     ; (G & 7) << 5
    or e
    ld e, a                      ; E = colour lo
    ; hi = (G >> 3) | (B << 2)
    ld a, d
    srl a
    srl a
    srl a                        ; G >> 3
    ld d, a
    ld a, c
    add a, a
    add a, a                     ; B << 2
    or d
    ld d, a                      ; D = colour hi
    ; BGPI = B*8 + 2 (index 1), auto-increment
    ld a, b
    add a, a
    add a, a
    add a, a                     ; B*8
    add a, 2
    or $80
    ldh [rBGPI], a
    ld b, 3
.cw:
    ld a, e
    ldh [rBGPD], a
    ld a, d
    ldh [rBGPD], a
    dec b
    jr nz, .cw
    ret

; Step A one unit toward target D (used to interpolate one colour channel).
StepCh:
    cp d
    ret z
    jr c, .up
    dec a
    ret
.up:
    inc a
    ret

; Interpolate every palette's letter colour toward the target R,G,B at HL over
; 32 frames (covers the full 0..31 channel range), one step per channel per frame.
FadeAll:
    ld a, [hl+]
    ldh [$FF90], a               ; target R
    ld a, [hl+]
    ldh [$FF91], a               ; target G
    ld a, [hl]
    ldh [$FF92], a               ; target B
    ld b, 32
.faFrame:
    ld c, 0
.faPal:
    ld a, c
    add a, a
    add a, c
    ld l, a
    ld h, $C0                    ; HL = $C000 + c*3
    ldh a, [$FF90]
    ld d, a
    ld a, [hl]
    call StepCh
    ld [hl+], a
    ldh a, [$FF91]
    ld d, a
    ld a, [hl]
    call StepCh
    ld [hl+], a
    ldh a, [$FF92]
    ld d, a
    ld a, [hl]
    call StepCh
    ld [hl], a
    push bc                      ; CombineWrite clobbers B,C
    ld b, c
    call CombineWrite
    pop bc
    inc c
    ld a, c
    cp 7                         ; palettes 0..6 (7 is the static logo)
    jr nz, .faPal
    call WaitFrame
    dec b
    jr nz, .faFrame
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

; Wait B frames.
DelayFrames:
.df:
    call WaitFrame
    dec b
    jr nz, .df
    ret

; 8 rainbow letter colours as R,G,B channels (0..31) — the hues the reveal
; paints the columns before they settle to blue. Plus the settle/fade targets.
HueRGB:
    db 31, 0, 0                  ; red
    db 31, 12, 0                 ; orange
    db 31, 28, 0                 ; yellow
    db 0, 28, 0                  ; green
    db 0, 28, 28                 ; cyan
    db 0, 8, 31                  ; blue
    db 12, 0, 31                 ; indigo
    db 24, 0, 28                 ; violet
BlueRGB:
    db 0, 6, 31                  ; the colour the logo settles to
WhiteRGB:
    db 31, 31, 31                ; the fade-out target

; The logo tile data is split: the first 13 tiles live in the small free gap
; between the entry code and the hand-off stub ($0029-$00F8); the rest floats in
; the main region. Both are boot ROM (not the cart-shadowed $0100-$01FF).
SECTION "logolow", ROM0[$0029]
LogoTilesLow:
    INCBIN "logo.2bpp", 0, LOGO_LOW_BYTES

SECTION "logohigh", ROM0
LogoTilesHigh:
    INCBIN "logo.2bpp", LOGO_LOW_BYTES, LOGO_HIGH_BYTES
