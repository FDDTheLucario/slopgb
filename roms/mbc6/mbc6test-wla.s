; mbc6test-wla.s — MBC6 exerciser ROM (wla-dx syntax twin of mbc6test.asm).
;
; Same test program and numbering as the RGBDS source; see mbc6test.asm for
; the test map and the Pan Docs "MBC6" behavior each test pins.
;
; Build:
;   wla-gb -o mbc6test-wla.o mbc6test-wla.s
;   echo -e "[objects]\nmbc6test-wla.o" > linkfile-wla
;   wlalink linkfile-wla mbc6test-wla.gb

.MEMORYMAP
SLOTSIZE $4000
DEFAULTSLOT 0
SLOT 0 $0000
SLOT 1 $4000
.ENDME

.ROMBANKMAP
BANKSTOTAL 8
BANKSIZE $4000
BANKS 8
.ENDRO

.EMPTYFILL $FF

.GBHEADER
NAME "MBC6TEST"
CARTRIDGETYPE $20
RAMSIZE $03
ROMSIZE
NINTENDOLOGO
ROMDMG
.ENDGB

.DEFINE RAMG     $0000
.DEFINE RAMB_A   $0400
.DEFINE RAMB_B   $0800
.DEFINE FLASH_EN $0C00
.DEFINE FLASH_WE $1000
.DEFINE ROMB_A   $2000
.DEFINE SEL_A    $2800
.DEFINE ROMB_B   $3000
.DEFINE SEL_B    $3800

; 8 KiB half-bank markers: MBC6 bank n exposes marker byte n at offset $1000
; (addresses $1000/$3000 of each 16 KiB bank, windowed at $5000/$7000).
.BANK 0 SLOT 0
.ORG $100
    nop
    jp start

.ORG $1000
.db 0
.ORG $3000
.db 1

.BANK 1 SLOT 1
.ORG $1000
.db 2
.ORG $3000
.db 3
.BANK 2 SLOT 1
.ORG $1000
.db 4
.ORG $3000
.db 5
.BANK 3 SLOT 1
.ORG $1000
.db 6
.ORG $3000
.db 7
.BANK 4 SLOT 1
.ORG $1000
.db 8
.ORG $3000
.db 9
.BANK 5 SLOT 1
.ORG $1000
.db 10
.ORG $3000
.db 11
.BANK 6 SLOT 1
.ORG $1000
.db 12
.ORG $3000
.db 13
.BANK 7 SLOT 1
.ORG $1000
.db 14
.ORG $3000
.db 15

.BANK 0 SLOT 0
.ORG $150
start:
    di
    ld sp,$DFFF

; --- Test 1: ROM window A: banks 0..15 each show their marker at $5000 ---
    ld c,1
    xor a
    ld (SEL_A),a
    ld b,0
t1loop:
    ld a,b
    ld (ROMB_A),a
    ld a,($5000)
    cp b
    jp nz,tfail
    inc b
    ld a,b
    cp 16
    jr nz,t1loop

; --- Test 2: ROM window B: banks 0..15 each show their marker at $7000 ---
    ld c,2
    xor a
    ld (SEL_B),a
    ld b,0
t2loop:
    ld a,b
    ld (ROMB_B),a
    ld a,($7000)
    cp b
    jp nz,tfail
    inc b
    ld a,b
    cp 16
    jr nz,t2loop

; --- Test 3: window A and B bank numbers are independent ---
    ld c,3
    ld a,4
    ld (ROMB_A),a
    ld a,9
    ld (ROMB_B),a
    ld a,($5000)
    cp 4
    jp nz,tfail
    ld a,($7000)
    cp 9
    jp nz,tfail
    ld a,5
    ld (ROMB_A),a          ; changing A must not disturb B
    ld a,($7000)
    cp 9
    jp nz,tfail
    ld a,($5000)
    cp 5
    jp nz,tfail

; --- Test 4: RAM enable gate: disabled reads $FF, disabled writes dropped ---
    ld c,4
    ld a,$0A
    ld (RAMG),a
    xor a
    ld (RAMB_A),a
    ld a,$55
    ld ($A000),a
    xor a
    ld (RAMG),a
    ld a,($A000)
    cp $FF                  ; open bus while disabled
    jp nz,tfail
    ld a,$AA
    ld ($A000),a            ; must be dropped
    ld a,$0A
    ld (RAMG),a
    ld a,($A000)
    cp $55
    jp nz,tfail

; --- Test 5: 8 RAM banks of 4 KiB, windows A and B both reach all of them ---
    ld c,5
    ld b,0
t5wloop:
    ld a,b
    ld (RAMB_A),a
    ld a,$A0
    or b
    ld ($A000),a            ; bank offset 0 via window A
    ld a,b
    ld (RAMB_B),a
    ld a,$B0
    or b
    ld ($B008),a            ; bank offset 8 via window B
    inc b
    ld a,b
    cp 8
    jr nz,t5wloop
    ld b,0
t5vloop:
    ld a,b
    ld (RAMB_A),a
    ld a,($A000)
    ld e,a
    ld a,$A0
    or b
    cp e
    jp nz,tfail
    ld a,b
    ld (RAMB_B),a
    ld a,($B008)
    ld e,a
    ld a,$B0
    or b
    cp e
    jp nz,tfail
    inc b
    ld a,b
    cp 8
    jr nz,t5vloop

; --- Test 6: both windows view the same RAM array (bank 3 aliases) ---
    ld c,6
    ld a,3
    ld (RAMB_A),a
    ld (RAMB_B),a
    ld a,$77
    ld ($A004),a
    ld a,($B004)
    cp $77
    jp nz,tfail

; --- Test 7: flash JEDEC ID mode reads C2/81 ---
    ld c,7
    ld a,1
    ld (FLASH_EN),a
    ld a,$08
    ld (SEL_A),a            ; window A shows the flash from here on
    ld e,$90
    call flash_cmd
    ld a,($4000)
    cp $C2
    jp nz,tfail
    ld a,($4001)
    cp $81
    jp nz,tfail
    call flash_exit

; --- Test 8: unprotect sector 0 (also clears any leftover protect state) ---
    ld c,8
    ld a,1
    ld (FLASH_WE),a
    ld e,$60
    call flash_cmd
    ld e,$40
    call flash_cmd
    call poll_status
    call flash_exit

; --- Test 9/10: program witnesses into sectors 1+2, erase sector 1: its
;     bytes return to $FF while sector 2 keeps its data. WE is dropped
;     first: sectors 1-7 must erase/program regardless of it ---
    ld c,9
    xor a
    ld (FLASH_WE),a
    ld d,16
    call erase_sector       ; programming requires an erased sector
    ld d,16
    ld e,$00
    call prog128            ; witness the erase must clear
    ld d,32
    call erase_sector
    ld d,32
    ld e,$33
    call prog128            ; sector-2 witness the erase must NOT touch
    ld d,16
    call erase_sector
    ld c,10
    ld a,16
    ld (ROMB_A),a
    ld a,($4000)
    cp $FF
    jp nz,tfail
    ld a,32
    ld (ROMB_A),a
    ld a,($4000)
    cp $33
    jp nz,tfail

; --- Test 11/12: program 128 bytes (value = offset) into sector 1 ---
    ld c,11
    ld e,$A0
    call flash_cmd
    ld a,16
    ld (ROMB_A),a
    ld hl,$4000
t11wloop:
    ld a,l
    ldi (hl),a
    ld a,l
    cp $80
    jr nz,t11wloop
    ld a,$7F
    ld ($407F),a            ; commit: rewrite the final address (not $F0)
    call poll_status
    call flash_exit
    ld c,12
    ld hl,$4000
t12vloop:
    ld a,(hl)
    cp l
    jp nz,tfail
    inc l
    ld a,l
    cp $80
    jr nz,t12vloop

; --- Test 13: the same flash bytes are visible through window B ---
    ld c,13
    ld a,$08
    ld (SEL_B),a
    ld a,16
    ld (ROMB_B),a
    ld a,($6000)
    cp $00
    jp nz,tfail
    ld a,($6042)
    cp $42
    jp nz,tfail
    xor a
    ld (SEL_B),a

; --- Test 14/15: with Flash Write Enable set, sector 0 erases + programs ---
    ld c,14
    ld a,1
    ld (FLASH_WE),a
    ld d,4                  ; bank 4 lies in sector 0
    call erase_sector
    ld a,4
    ld (ROMB_A),a
    ld a,($4000)
    cp $FF
    jp nz,tfail
    ld c,15
    ld d,4
    ld e,$55
    call prog128
    ld a,($4000)
    cp $55
    jp nz,tfail

; --- Test 16: WE=0 blocks sector 0 erase ---
    ld c,16
    xor a
    ld (FLASH_WE),a
    ld e,$80
    call flash_cmd
    call flash_unlock
    ld a,4
    ld (ROMB_A),a
    ld a,$30
    ld ($4000),a            ; blocked: no poll (the op never starts)
    call flash_exit
    ld a,4
    ld (ROMB_A),a
    ld a,($4000)
    cp $55
    jp nz,tfail

; --- Test 17: WE=0 blocks sector 0 program ---
    ld c,17
    ld d,4
    ld e,$00
    call prog_blocked
    ld a,($4000)
    cp $55
    jp nz,tfail

; --- Test 18: Protect Sector 0 blocks programming even with WE=1, shows in
;              status bit 1; Unprotect clears it ---
    ld c,18
    ld a,1
    ld (FLASH_WE),a
    ld e,$60
    call flash_cmd
    ld e,$20
    call flash_cmd          ; protect sector 0
    call poll_status
    bit 1,a                 ; status must report the protection
    jp z,tfail
    call flash_exit
    ld d,4
    ld e,$00
    call prog_blocked       ; WE=1, but the protect flag must block it
    ld a,($4000)
    cp $55
    jp nz,tfail
    ld e,$60
    call flash_cmd
    ld e,$40
    call flash_cmd          ; unprotect again
    call poll_status
    bit 1,a
    jp nz,tfail
    call flash_exit
    ld d,4
    call erase_sector       ; unprotected + WE=1: must clear the $55 data
    ld a,4
    ld (ROMB_A),a
    ld a,($4000)
    cp $FF
    jp nz,tfail

; --- Test 19: hidden region erase, reads back $FF in hidden-read mode ---
    ld c,19
    ld e,$60
    call flash_cmd
    ld e,$04
    call flash_cmd
    call poll_status
    call flash_exit
    ld e,$77
    call flash_cmd
    ld e,$77
    call flash_cmd          ; hidden-read mode
    xor a
    ld (ROMB_A),a
    ld a,($4000)
    cp $FF
    jp nz,tfail
    ld a,($40FF)
    cp $FF
    jp nz,tfail
    call flash_exit

; --- Test 20: hidden region program (128 bytes, value = offset) ---
    ld c,20
    ld e,$60
    call flash_cmd
    ld e,$E0
    call flash_cmd          ; program mode for the hidden region
    xor a
    ld (ROMB_A),a
    ld hl,$4000
t20wloop:
    ld a,l
    ldi (hl),a
    ld a,l
    cp $80
    jr nz,t20wloop
    ld a,$7F
    ld ($407F),a            ; commit
    call poll_status
    call flash_exit
    ld e,$77
    call flash_cmd
    ld e,$77
    call flash_cmd
    ld hl,$4000
t20vloop:
    ld a,(hl)
    cp l
    jp nz,tfail
    inc l
    ld a,l
    cp $80
    jr nz,t20vloop
    call flash_exit
    ; Hidden erase again, now with data to clear: the pattern must go.
    ld e,$60
    call flash_cmd
    ld e,$04
    call flash_cmd
    call poll_status
    call flash_exit
    ld e,$77
    call flash_cmd
    ld e,$77
    call flash_cmd
    xor a
    ld (ROMB_A),a
    ld a,($4000)
    cp $FF
    jp nz,tfail
    ld a,($407F)
    cp $FF
    jp nz,tfail
    call flash_exit

; --- Test 21: back to ROM mapping; writes into a ROM-mapped window inert ---
    ld c,21
    xor a
    ld (SEL_A),a
    ld (FLASH_EN),a
    ld (FLASH_WE),a
    ld a,5
    ld (ROMB_A),a
    ld a,$AA
    ld ($5555),a            ; ROM mapped: must not reach flash or registers
    ld a,($5000)
    cp 5
    jp nz,tfail

pass:
    ld b,3
    ld c,5
    ld d,8
    ld e,13
    ld h,21
    ld l,34
    ld b,b
passhang:
    jr passhang

tfail:
    ld b,$42
    ld d,$42
    ld e,$42
    ld h,$42
    ld l,$42
    ld b,b
failhang:
    jr failhang

; Write $AA to flash address 2:$5555 and $55 to 1:$4AAA through window A
; (the JEDEC $5555/$2AAA unlock, split across 8 KiB banks). Leaves bank 2
; selected. Clobbers A only.
flash_unlock:
    ld a,2
    ld (ROMB_A),a
    ld a,$AA
    ld ($5555),a
    ld a,1
    ld (ROMB_A),a
    ld a,$55
    ld ($4AAA),a
    ld a,2
    ld (ROMB_A),a
    ret

; Unlock, then write command byte E to 2:$5555. Clobbers A.
flash_cmd:
    call flash_unlock
    ld a,e
    ld ($5555),a
    ret

; Exit the current flash mode ($F0 at any address). Clobbers A.
flash_exit:
    ld a,$F0
    ld ($4000),a
    ret

; Poll the flash status byte until bit 7 (operation finished) is set;
; returns it in A. Bound: 8 * 65536 polls (~6 emulated seconds), enough for
; a worst-case real MX29F008 sector erase. Clobbers A, B, DE.
poll_status:
    ld b,8
pollouter:
    ld de,0
pollloop:
    ld a,($4000)
    bit 7,a
    ret nz
    dec de
    ld a,d
    or e
    jr nz,pollloop
    dec b
    jr nz,pollouter
    jp tfail

; Program 128 bytes of value E at offset 0 of flash bank D (must be erased
; first), with commit, status poll and mode exit. Clobbers A, B, DE, HL.
prog128:
    push de
    ld e,$A0
    call flash_cmd
    pop de
    ld a,d
    ld (ROMB_A),a
    ld hl,$4000
p128loop:
    ld a,e
    ldi (hl),a
    ld a,l
    cp $80
    jr nz,p128loop
    ld a,e
    ld ($407F),a            ; commit (E is never $F0 here)
    call poll_status
    jp flash_exit

; Like prog128 but for writes that must be blocked (sector 0 protection):
; no status poll — the operation never starts, so on hardware the chip
; would stay in read mode and bit 7 would never rise. Clobbers A, DE, HL.
prog_blocked:
    push de
    ld e,$A0
    call flash_cmd
    pop de
    ld a,d
    ld (ROMB_A),a
    ld hl,$4000
pblkloop:
    ld a,e
    ldi (hl),a
    ld a,l
    cp $80
    jr nz,pblkloop
    ld a,e
    ld ($407F),a
    jp flash_exit

; Erase the 128 KiB sector containing flash bank D: $30 written to an
; address inside the sector, then poll + exit. Clobbers A, B, DE.
erase_sector:
    ld e,$80
    call flash_cmd
    call flash_unlock
    ld a,d
    ld (ROMB_A),a
    ld a,$30
    ld ($4000),a
    call poll_status
    jp flash_exit
