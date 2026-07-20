; mbc6test.asm — MBC6 exerciser ROM (RGBDS syntax; wla-dx twin: mbc6test-wla.s).
;
; Tests the MBC6 register file + Macronix MX29F008 flash per Pan Docs "MBC6":
; two independent 8 KiB ROM/flash windows (A: 4000-5FFF, B: 6000-7FFF), two
; independent 4 KiB RAM windows (A: A000-AFFF, B: B000-BFFF), and the flash
; command set (JEDEC ID, sector/hidden erase, 128-byte program, sector-0
; protection via both the Flash Write Enable register and the Protect
; Sector 0 command).
;
; Pass/fail: mooneye breakpoint protocol — LD B,B with B,C,D,E,H,L =
; 3,5,8,13,21,34 on pass; on failure B=$42 and C = the failing test number
; (see the "Test map" comments below).
;
; Build:
;   rgbasm -o mbc6test.o mbc6test.asm
;   rgblink -o mbc6test.gb -p 0xFF mbc6test.o
;   rgbfix -v -p 0xFF -m 0x20 -r 0x03 -t "MBC6TEST" mbc6test.gb

DEF RAMG     EQU $0000 ; $0A enables both RAM windows, $00 disables
DEF RAMB_A   EQU $0400 ; RAM bank at A000-AFFF (0-7)
DEF RAMB_B   EQU $0800 ; RAM bank at B000-BFFF (0-7)
DEF FLASH_EN EQU $0C00 ; bit 0 drives the flash chip /CE
DEF FLASH_WE EQU $1000 ; bit 0 drives flash /WP: gates sector 0 + hidden region
DEF ROMB_A   EQU $2000 ; ROM/flash bank at 4000-5FFF (0-$7F)
DEF SEL_A    EQU $2800 ; $08 = flash mapped at 4000-5FFF, $00 = ROM
DEF ROMB_B   EQU $3000 ; ROM/flash bank at 6000-7FFF (0-$7F)
DEF SEL_B    EQU $3800 ; $08 = flash mapped at 6000-7FFF, $00 = ROM

; Every 8 KiB MBC6 bank n carries the marker byte n at offset $1000, so a
; window read at $5000 (A) / $7000 (B) names the bank actually mapped.
SECTION "entry", ROM0[$100]
    nop
    jp start

; DMG-compat CGB flag, so this twin matches the wla-dx build (whose ROMDMG
; writes $00); without it the link-time $FF pad would boot CGB-native.
SECTION "cgb_flag", ROM0[$143]
    db $00

SECTION "marker0", ROM0[$1000]
    db 0
SECTION "marker1", ROM0[$3000]
    db 1

; 16 KiB RGBDS bank K holds MBC6 banks 2K (file offset K*$4000+$0000) and
; 2K+1 (K*$4000+$2000); their markers land at RGBDS addresses $5000/$7000.
FOR K, 1, 8
SECTION "markerA{d:K}", ROMX[$5000], BANK[K]
    db 2 * K
SECTION "markerB{d:K}", ROMX[$7000], BANK[K]
    db 2 * K + 1
ENDR

SECTION "main", ROM0[$150]
start:
    di
    ld sp, $DFFF

; --- Test 1: ROM window A: banks 0..15 each show their marker at $5000 ---
    ld c, 1
    xor a
    ld [SEL_A], a
    ld b, 0
.t1:
    ld a, b
    ld [ROMB_A], a
    ld a, [$5000]
    cp b
    jp nz, tfail
    inc b
    ld a, b
    cp 16
    jr nz, .t1

; --- Test 2: ROM window B: banks 0..15 each show their marker at $7000 ---
    ld c, 2
    xor a
    ld [SEL_B], a
    ld b, 0
.t2:
    ld a, b
    ld [ROMB_B], a
    ld a, [$7000]
    cp b
    jp nz, tfail
    inc b
    ld a, b
    cp 16
    jr nz, .t2

; --- Test 3: window A and B bank numbers are independent ---
    ld c, 3
    ld a, 4
    ld [ROMB_A], a
    ld a, 9
    ld [ROMB_B], a
    ld a, [$5000]
    cp 4
    jp nz, tfail
    ld a, [$7000]
    cp 9
    jp nz, tfail
    ld a, 5
    ld [ROMB_A], a          ; changing A must not disturb B
    ld a, [$7000]
    cp 9
    jp nz, tfail
    ld a, [$5000]
    cp 5
    jp nz, tfail

; --- Test 4: RAM enable gate: disabled reads $FF, disabled writes dropped ---
    ld c, 4
    ld a, $0A
    ld [RAMG], a
    xor a
    ld [RAMB_A], a
    ld a, $55
    ld [$A000], a
    xor a
    ld [RAMG], a
    ld a, [$A000]
    cp $FF                  ; open bus while disabled
    jp nz, tfail
    ld a, $AA
    ld [$A000], a           ; must be dropped
    ld a, $0A
    ld [RAMG], a
    ld a, [$A000]
    cp $55
    jp nz, tfail

; --- Test 5: 8 RAM banks of 4 KiB, windows A and B both reach all of them ---
    ld c, 5
    ld b, 0
.t5w:
    ld a, b
    ld [RAMB_A], a
    ld a, $A0
    or b
    ld [$A000], a           ; bank offset 0 via window A
    ld a, b
    ld [RAMB_B], a
    ld a, $B0
    or b
    ld [$B008], a           ; bank offset 8 via window B
    inc b
    ld a, b
    cp 8
    jr nz, .t5w
    ld b, 0
.t5v:
    ld a, b
    ld [RAMB_A], a
    ld a, [$A000]
    ld e, a
    ld a, $A0
    or b
    cp e
    jp nz, tfail
    ld a, b
    ld [RAMB_B], a
    ld a, [$B008]
    ld e, a
    ld a, $B0
    or b
    cp e
    jp nz, tfail
    inc b
    ld a, b
    cp 8
    jr nz, .t5v

; --- Test 6: both windows view the same RAM array (bank 3 aliases) ---
    ld c, 6
    ld a, 3
    ld [RAMB_A], a
    ld [RAMB_B], a
    ld a, $77
    ld [$A004], a
    ld a, [$B004]
    cp $77
    jp nz, tfail

; --- Test 7: flash JEDEC ID mode reads C2/81 ---
    ld c, 7
    ld a, 1
    ld [FLASH_EN], a
    ld a, $08
    ld [SEL_A], a           ; window A shows the flash from here on
    ld e, $90
    call flash_cmd
    ld a, [$4000]
    cp $C2
    jp nz, tfail
    ld a, [$4001]
    cp $81
    jp nz, tfail
    call flash_exit

; --- Test 8: unprotect sector 0 (also clears any leftover protect state) ---
    ld c, 8
    ld a, 1
    ld [FLASH_WE], a
    ld e, $60
    call flash_cmd
    ld e, $40
    call flash_cmd
    call poll_status
    call flash_exit

; --- Test 9/10: program witnesses into sectors 1+2, erase sector 1: its
;     bytes return to $FF while sector 2 keeps its data. WE is dropped
;     first: sectors 1-7 must erase/program regardless of it ---
    ld c, 9
    xor a
    ld [FLASH_WE], a
    ld d, 16
    call erase_sector       ; programming requires an erased sector
    ld d, 16
    ld e, $00
    call prog128            ; witness the erase must clear
    ld d, 32
    call erase_sector
    ld d, 32
    ld e, $33
    call prog128            ; sector-2 witness the erase must NOT touch
    ld d, 16
    call erase_sector
    ld c, 10
    ld a, 16
    ld [ROMB_A], a
    ld a, [$4000]
    cp $FF
    jp nz, tfail
    ld a, 32
    ld [ROMB_A], a
    ld a, [$4000]
    cp $33
    jp nz, tfail

; --- Test 11/12: program 128 bytes (value = offset) into sector 1 ---
    ld c, 11
    ld e, $A0
    call flash_cmd
    ld a, 16
    ld [ROMB_A], a
    ld hl, $4000
.t11w:
    ld a, l
    ld [hl+], a
    ld a, l
    cp $80
    jr nz, .t11w
    ld a, $7F
    ld [$407F], a           ; commit: rewrite the final address (not $F0)
    call poll_status
    call flash_exit
    ld c, 12
    ld hl, $4000
.t12v:
    ld a, [hl]
    cp l
    jp nz, tfail
    inc l
    ld a, l
    cp $80
    jr nz, .t12v

; --- Test 13: the same flash bytes are visible through window B ---
    ld c, 13
    ld a, $08
    ld [SEL_B], a
    ld a, 16
    ld [ROMB_B], a
    ld a, [$6000]
    cp $00
    jp nz, tfail
    ld a, [$6042]
    cp $42
    jp nz, tfail
    xor a
    ld [SEL_B], a

; --- Test 14/15: with Flash Write Enable set, sector 0 erases + programs ---
    ld c, 14
    ld a, 1
    ld [FLASH_WE], a
    ld d, 4                 ; bank 4 lies in sector 0
    call erase_sector
    ld a, 4
    ld [ROMB_A], a
    ld a, [$4000]
    cp $FF
    jp nz, tfail
    ld c, 15
    ld d, 4
    ld e, $55
    call prog128
    ld a, [$4000]
    cp $55
    jp nz, tfail

; --- Test 16: WE=0 blocks sector 0 erase ---
    ld c, 16
    xor a
    ld [FLASH_WE], a
    ld e, $80
    call flash_cmd
    call flash_unlock
    ld a, 4
    ld [ROMB_A], a
    ld a, $30
    ld [$4000], a           ; blocked: no poll (the op never starts)
    call flash_exit
    ld a, 4
    ld [ROMB_A], a
    ld a, [$4000]
    cp $55
    jp nz, tfail

; --- Test 17: WE=0 blocks sector 0 program ---
    ld c, 17
    ld d, 4
    ld e, $00
    call prog_blocked
    ld a, [$4000]
    cp $55
    jp nz, tfail

; --- Test 18: Protect Sector 0 command blocks programming even with WE=1,
;              and shows up as status bit 1; Unprotect clears it ---
    ld c, 18
    ld a, 1
    ld [FLASH_WE], a
    ld e, $60
    call flash_cmd
    ld e, $20
    call flash_cmd          ; protect sector 0
    call poll_status
    bit 1, a                ; status must report the protection
    jp z, tfail
    call flash_exit
    ld d, 4
    ld e, $00
    call prog_blocked       ; WE=1, but the protect flag must block it
    ld a, [$4000]
    cp $55
    jp nz, tfail
    ld e, $60
    call flash_cmd
    ld e, $40
    call flash_cmd          ; unprotect again
    call poll_status
    bit 1, a
    jp nz, tfail
    call flash_exit
    ld d, 4
    call erase_sector       ; unprotected + WE=1: must clear the $55 data
    ld a, 4
    ld [ROMB_A], a
    ld a, [$4000]
    cp $FF
    jp nz, tfail

; --- Test 19: hidden region erase, reads back $FF in hidden-read mode ---
    ld c, 19
    ld e, $60
    call flash_cmd
    ld e, $04
    call flash_cmd
    call poll_status
    call flash_exit
    ld e, $77
    call flash_cmd
    ld e, $77
    call flash_cmd          ; hidden-read mode
    xor a
    ld [ROMB_A], a
    ld a, [$4000]
    cp $FF
    jp nz, tfail
    ld a, [$40FF]
    cp $FF
    jp nz, tfail
    call flash_exit

; --- Test 20: hidden region program (128 bytes, value = offset) ---
    ld c, 20
    ld e, $60
    call flash_cmd
    ld e, $E0
    call flash_cmd          ; program mode for the hidden region
    xor a
    ld [ROMB_A], a
    ld hl, $4000
.t20w:
    ld a, l
    ld [hl+], a
    ld a, l
    cp $80
    jr nz, .t20w
    ld a, $7F
    ld [$407F], a           ; commit
    call poll_status
    call flash_exit
    ld e, $77
    call flash_cmd
    ld e, $77
    call flash_cmd
    ld hl, $4000
.t20v:
    ld a, [hl]
    cp l
    jp nz, tfail
    inc l
    ld a, l
    cp $80
    jr nz, .t20v
    call flash_exit
    ; Hidden erase again, now with data to clear: the pattern must go.
    ld e, $60
    call flash_cmd
    ld e, $04
    call flash_cmd
    call poll_status
    call flash_exit
    ld e, $77
    call flash_cmd
    ld e, $77
    call flash_cmd
    xor a
    ld [ROMB_A], a
    ld a, [$4000]
    cp $FF
    jp nz, tfail
    ld a, [$407F]
    cp $FF
    jp nz, tfail
    call flash_exit

; --- Test 21: back to ROM mapping; writes into a ROM-mapped window are inert ---
    ld c, 21
    xor a
    ld [SEL_A], a
    ld [FLASH_EN], a
    ld [FLASH_WE], a
    ld a, 5
    ld [ROMB_A], a
    ld a, $AA
    ld [$5555], a           ; ROM mapped: must not reach the flash or any register
    ld a, [$5000]
    cp 5
    jp nz, tfail

pass:
    ld b, 3
    ld c, 5
    ld d, 8
    ld e, 13
    ld h, 21
    ld l, 34
    ld b, b
.hang:
    jr .hang

tfail:
    ld b, $42
    ld d, $42
    ld e, $42
    ld h, $42
    ld l, $42
    ld b, b
.hang:
    jr .hang

; Write $AA to flash address 2:$5555 and $55 to 1:$4AAA through window A
; (the JEDEC $5555/$2AAA unlock, split across 8 KiB banks). Leaves bank 2
; selected. Clobbers A only.
flash_unlock:
    ld a, 2
    ld [ROMB_A], a
    ld a, $AA
    ld [$5555], a
    ld a, 1
    ld [ROMB_A], a
    ld a, $55
    ld [$4AAA], a
    ld a, 2
    ld [ROMB_A], a
    ret

; Unlock, then write command byte E to 2:$5555. Clobbers A.
flash_cmd:
    call flash_unlock
    ld a, e
    ld [$5555], a
    ret

; Exit the current flash mode ($F0 at any address). Clobbers A.
flash_exit:
    ld a, $F0
    ld [$4000], a
    ret

; Poll the flash status byte until bit 7 (operation finished) is set;
; returns it in A. Bound: 8 * 65536 polls (~6 emulated seconds), enough for
; a worst-case real MX29F008 sector erase. Clobbers A, B, DE.
poll_status:
    ld b, 8
.outer:
    ld de, 0
.lp:
    ld a, [$4000]
    bit 7, a
    ret nz
    dec de
    ld a, d
    or e
    jr nz, .lp
    dec b
    jr nz, .outer
    jp tfail

; Program 128 bytes of value E at offset 0 of flash bank D (must be erased
; first), with commit, status poll and mode exit. Clobbers A, B, DE, HL.
prog128:
    push de
    ld e, $A0
    call flash_cmd
    pop de
    ld a, d
    ld [ROMB_A], a
    ld hl, $4000
.lp:
    ld a, e
    ld [hl+], a
    ld a, l
    cp $80
    jr nz, .lp
    ld a, e
    ld [$407F], a           ; commit (E is never $F0 here)
    call poll_status
    jp flash_exit

; Like prog128 but for writes that must be blocked (sector 0 protection):
; no status poll — the operation never starts, so on hardware the chip
; would stay in read mode and bit 7 would never rise. Clobbers A, DE, HL.
prog_blocked:
    push de
    ld e, $A0
    call flash_cmd
    pop de
    ld a, d
    ld [ROMB_A], a
    ld hl, $4000
.lp:
    ld a, e
    ld [hl+], a
    ld a, l
    cp $80
    jr nz, .lp
    ld a, e
    ld [$407F], a
    jp flash_exit

; Erase the 128 KiB sector containing flash bank D: $30 written to an
; address inside the sector, then poll + exit. Clobbers A, B, DE.
erase_sector:
    ld e, $80
    call flash_cmd
    call flash_unlock
    ld a, d
    ld [ROMB_A], a
    ld a, $30
    ld [$4000], a
    call poll_status
    jp flash_exit
