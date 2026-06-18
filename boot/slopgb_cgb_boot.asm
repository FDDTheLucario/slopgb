; slopgb CGB boot ROM — original work, written from public hardware
; documentation (Pan Docs / gbdev wiki). 2304 bytes, CGB-class.
;
; Memory map while mapped (crates/slopgb-core/src/interconnect/boot_rom.rs):
;   $0000-$00FF  entry/init (+ the FF50 hand-off stub at $00FE)
;   $0100-$01FF  the CART header shows through here (logo + title), not boot ROM
;   $0200-$08FF  main routine + logo tiles + (later) chime/palette data
; Writing FF50 bit 0 = 1 unmaps the boot ROM; PC then runs the cart at $0100.

INCLUDE "hardware.inc"

DEF LOGO_COLS    EQU 11        ; the SLOPGB wordmark is 11x3 tiles (88x24 px),
DEF LOGO_ROWS    EQU 3         ; same per-glyph size as the real GAME BOY logo
DEF LOGO_TILES   EQU LOGO_COLS * LOGO_ROWS
DEF LOGO_LOW_BYTES  EQU 13 * 16    ; first 13 tiles (fit the $0029 boot gap)
DEF LOGO_HIGH_BYTES EQU LOGO_TILES * 16 - LOGO_LOW_BYTES
DEF LOGO_PALS    EQU 6         ; logo columns share 6 rainbow palettes (2 cols ea)
DEF NIN_PAL      EQU 7         ; the static Nintendo subtext palette
DEF NIN_TILE     EQU LOGO_TILES + 1   ; first Nintendo tile (after the wordmark)
DEF NIN_COLS     EQU 6         ; Nintendo subtext is 6x1 tiles (48x8, native size)

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

    ; --- render the cart's Nintendo logo (native 6x1 subtext) into tiles 34+ ---
    call DecompressNintendo

    ; --- BG palettes. Every palette's 4 colours start WHITE ($7FFF) so the whole
    ; screen is blank when the LCD comes on. The Nintendo subtext is painted black
    ; after a short blank hold, and the logo palettes are painted by the reveal —
    ; exactly the reference's blank -> Nintendo -> wordmark sequence. ---
    ld a, $80                    ; auto-increment from colour index 0
    ldh [rBGPI], a
    ld c, 8                      ; 8 palettes
.palOuter:
    ld b, 4                      ; 4 colours each
.palInner:
    ld a, $FF                    ; $7FFF = white
    ldh [rBGPD], a
    ld a, $7F
    ldh [rBGPD], a
    dec b
    jr nz, .palInner
    dec c
    jr nz, .palOuter

    ; --- build the BG tilemap (bank 0): the 12x3 logo centred at col 4, rows 6-8 ---
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

    ; --- place the Nintendo subtext: 1 row x 6 tiles at row 13, centred (col 7) ---
    ld hl, $9800 + 13*32 + 7
    ld a, NIN_TILE               ; first Nintendo tile (after the wordmark tiles)
    ld c, NIN_COLS
.ntmCol:
    ld [hl+], a
    inc a
    dec c
    jr nz, .ntmCol

    ; --- BG attribute map (bank 1) ---
    ld a, 1
    ldh [rVBK], a
    ; logo columns share 6 rainbow palettes, two columns each (palette = col>>1),
    ; so the reveal sweeps a diagonal rainbow band. Palette 7 stays the static
    ; Nintendo subtext.
    ld hl, $9800 + 6*32 + 4
    ld c, LOGO_ROWS
.attrrow:
    ld b, 0                      ; column counter
.attrcol:
    ld a, b
    srl a                        ; palette = column >> 1 (0..5)
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
    ; Nintendo subtext -> palette 7 (static black)
    ld hl, $9800 + 13*32 + 7
    ld a, NIN_PAL
    ld c, NIN_COLS
.nattrCol:
    ld [hl+], a
    dec c
    jr nz, .nattrCol
    xor a
    ldh [rVBK], a                ; back to bank 0

    ; --- LCD on: BG enabled, $8000 tile data, $9800 BG map ---
    ld a, $91
    ldh [rLCDC], a

    ; Blank hold: the reference shows a white screen for ~33 frames before the
    ; Nintendo subtext fades in.
    ld b, 33
    call DelayFrames

    ; Paint the Nintendo subtext black (palette 7 letters), then hold on it alone
    ; for ~24 frames while the wordmark is still invisible — the reference shows
    ; 'Nintendo' by itself before the reveal begins.
    ld de, $0000                 ; black
    ld b, NIN_PAL
    call WritePalDE
    ld b, 24
    call DelayFrames

    ; --- Reveal: paint the 6 logo palettes left-to-right, each one cycling
    ; Yellow->Red->Magenta->Green->Blue (the reference's rainbow). A new group is
    ; revealed every REVEAL frames; every CYCLE frames each revealed group steps
    ; one colour onward, capping at blue. Earlier groups run ahead, so a diagonal
    ; rainbow band sweeps across the wordmark and resolves to solid blue.
    ; WRAM: $C000+g = group g's phase (0..4) or $FF hidden; $C006 revealed count,
    ;       $C007 reveal timer, $C008 cycle timer. ---
    ld hl, $C000
    ld b, LOGO_PALS
    ld a, $FF
.rvInit:
    ld [hl+], a                  ; every group hidden
    dec b
    jr nz, .rvInit
    xor a
    ld [$C006], a                ; revealed = 0
    ld a, 1
    ld [$C007], a                ; reveal the first group on the first frame
    ld a, 6
    ld [$C008], a                ; cycle timer
.rvLoop:
    ld hl, $C007                 ; reveal timer
    dec [hl]
    jr nz, .rvNoReveal
    ld [hl], 5                   ; a new group every 5 frames
    ld a, [$C006]
    cp LOGO_PALS
    jr nc, .rvNoReveal           ; all groups already revealed
    ld e, a
    ld d, $C0
    xor a
    ld [de], a                   ; phase[revealed] = 0 (yellow)
    ld a, [$C006]
    inc a
    ld [$C006], a
.rvNoReveal:
    ld hl, $C008                 ; cycle timer
    dec [hl]
    jr nz, .rvNoCycle
    ld [hl], 6                   ; advance phases every 6 frames
    ld hl, $C000
    ld b, LOGO_PALS
.rvAdv:
    ld a, [hl]
    cp $FF
    jr z, .rvAdvNext             ; hidden
    cp 4
    jr z, .rvAdvNext             ; already blue
    inc a
    ld [hl], a
.rvAdvNext:
    inc hl
    dec b
    jr nz, .rvAdv
.rvNoCycle:
    ld c, 0                      ; render each group's current colour
.rvRender:
    ld a, c
    ld e, a
    ld d, $C0
    ld a, [de]                   ; phase
    cp $FF
    jr z, .rvRenderNext          ; hidden -> palette stays white (invisible)
    add a, a                     ; phase*2 = SeqColors offset
    ld e, a
    ld d, 0
    ld hl, SeqColors
    add hl, de
    ld e, [hl]
    inc hl
    ld d, [hl]                   ; DE = colour word
    ld b, c                      ; palette = group index
    push bc
    call WritePalDE
    pop bc
.rvRenderNext:
    inc c
    ld a, c
    cp LOGO_PALS
    jr nz, .rvRender
    call WaitFrame
    ; done when every group is revealed and the last group has reached blue
    ; (groups cycle in lockstep, so the last-revealed group settles last).
    ld a, [$C006]
    cp LOGO_PALS
    jr nz, .rvLoop
    ld a, [$C000 + LOGO_PALS - 1]   ; phase of the last group
    cp 4
    jr nz, .rvLoop

    ; --- the two-tone chime rings out over the solid blue logo; its ~34 frames
    ; are the reference's blue dwell ---
    call PlayChime

    ; --- Fade: ramp the blue logo and the black Nintendo subtext up to white
    ; over 31 frames (the reference fades both out together before hand-off) ---
    call FadeToWhite

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

; Render the cart's 48-byte Nintendo logo ($0104) at its NATIVE size — 6x1 tiles
; (48x8), the small subtext size the reference CGB boot shows (the DMG boot
; doubles it; this is that logo un-doubled). The standard logo data, halved, is
; exactly two source nibbles per tile-row: for native tile T (0..5) and row y,
;   byte = (nibble(src[base + 4T + off]) << 4) | nibble(src[base + 4T+2 + off])
; with base = $0104 + (y>=4 ? 24 : 0), off = (y&2 ? 1 : 0), and the high nibble
; for even y, the low nibble for odd y. The logo data stays in the cart (this is
; a functional/interop re-render, never an embedded copy); the hi bitplane stays
; 0 (VRAM was cleared) so the subtext renders as shade 1.
DecompressNintendo:
    ld b, 0                      ; row y (0..7)
.nrow:
    ; DE -> the row's left source byte = $0104 + band + off
    ld a, b
    and 4                        ; y >= 4 ?
    ld e, 0
    jr z, .nband
    ld e, 24
.nband:
    bit 1, b                     ; (y & 2) -> +1
    jr z, .noff
    inc e
.noff:
    ld a, e
    add a, $04
    ld e, a
    ld d, $01
    ; HL = tile 0, row y, plane 0 = $8000 + NIN_TILE*16 + y*2 (no carry into hi)
    ld a, b
    add a, a
    add a, LOW($8000 + NIN_TILE * 16)
    ld l, a
    ld h, HIGH($8000 + NIN_TILE * 16)
    ld c, NIN_COLS               ; tile T (0..5)
.ntile:
    ld a, [de]                   ; left source byte
    call NinNib                  ; left nibble -> low 4 bits
    swap a                       ; -> high 4 bits
    ld [hl], a
    inc de
    inc de                       ; DE -> right source byte (left + 2)
    ld a, [de]
    call NinNib                  ; right nibble -> low 4 bits
    or [hl]                      ; (left << 4) | right
    ld [hl], a
    inc de
    inc de                       ; DE -> next tile's left (left + 4)
    push de
    ld de, 16
    add hl, de                   ; HL -> next tile, same row
    pop de
    dec c
    jr nz, .ntile
    inc b
    ld a, b
    cp 8
    jr nz, .nrow
    ret

; Return in A the nibble of A selected by row parity in B: the high nibble when
; y is even, the low nibble when odd, placed in the low 4 bits.
NinNib:
    bit 0, b
    jr nz, .lo
    swap a
.lo:
    and $0F
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

; Write the BGR555 colour word in DE to BG palette B's letter colours (indices
; 1,2,3), via BGPI auto-increment. Preserves DE; clobbers A and B.
WritePalDE:
    ld a, b
    add a, a
    add a, a
    add a, a                     ; B*8
    add a, 2                     ; colour index 1
    or $80                       ; auto-increment
    ldh [rBGPI], a
    ld b, 3                      ; indices 1,2,3
.wp:
    ld a, e
    ldh [rBGPD], a
    ld a, d
    ldh [rBGPD], a
    dec b
    jr nz, .wp
    ret

; Fade the 6 blue logo palettes and the black Nintendo subtext up to white over
; 31 frames. At level k (1..31) the logo colour is (k,k,31) and the Nintendo
; colour is (k,k,k); both reach $7FFF (white) at k=31. Packed into BGR555 as
; lo = k | ((k&7)<<5), hi = (k>>3) | (b<<2).
FadeToWhite:
    ld c, 1                      ; ramp level
.fwLoop:
    ld a, c                      ; lo byte (shared: R=G=k)
    and 7
    swap a
    add a, a                     ; (k&7)<<5
    or c                         ; | k
    ld e, a                      ; E = colour lo
    ld a, c
    srl a
    srl a
    srl a                        ; A = k>>3 (0..3)
    or 124                       ; | (31<<2): blue channel full
    ld d, a                      ; D = logo hi -> DE = (k,k,31)
    ld b, 0
.fwLogo:
    push bc
    call WritePalDE              ; preserves DE
    pop bc
    inc b
    ld a, b
    cp LOGO_PALS
    jr nz, .fwLogo
    ld a, d                      ; Nintendo hi = (k>>3) | (k<<2)
    and 3                        ; recover k>>3
    ld d, a
    ld a, c
    add a, a
    add a, a                     ; k<<2
    or d
    ld d, a                      ; DE = (k,k,k)
    ld b, NIN_PAL
    call WritePalDE
    call WaitFrame
    inc c
    ld a, c
    cp 32
    jr nz, .fwLoop
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

; The reveal's colour cycle as packed BGR555 words: Yellow -> Red -> Magenta ->
; Green -> Blue, the exact pure hues the reference steps each column through
; before it settles on blue (measured frame-by-frame from a legal boot ROM).
SeqColors:
    dw $03FF                     ; yellow  (31,31,0)
    dw $001F                     ; red     (31,0,0)
    dw $7C1F                     ; magenta (31,0,31)
    dw $03E0                     ; green   (0,31,0)
    dw $7C00                     ; blue    (0,0,31)

; The logo tile data is split: the first 13 tiles live in the small free gap
; between the entry code and the hand-off stub ($0029-$00F8); the rest floats in
; the main region. Both are boot ROM (not the cart-shadowed $0100-$01FF).
SECTION "logolow", ROM0[$0029]
LogoTilesLow:
    INCBIN "logo.2bpp", 0, LOGO_LOW_BYTES

SECTION "logohigh", ROM0
LogoTilesHigh:
    INCBIN "logo.2bpp", LOGO_LOW_BYTES, LOGO_HIGH_BYTES
