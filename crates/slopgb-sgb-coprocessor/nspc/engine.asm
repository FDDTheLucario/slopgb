; ==========================================================================
; Clean-room SPC700 (SNES S-SMP) music sequencer engine.
;
; Written from SPEC.md alone. No reference to any existing sound engine,
; driver, ROM, SPC file, or disassembly. Every design choice below is my
; own engineering decision from the documented data format + public S-DSP
; hardware registers; assumptions are called out in comments and README.md.
;
; Loads at ARAM $0400, entered at PC=$0400 with the SPC700 already IPL-booted.
; Assembler: WLA-DX wla-spc700 (.spc700 syntax).
; ==========================================================================

; --------------------------------------------------------------------------
; Output layout: one bank mapped so that org 0 == CPU address $0400, i.e. the
; first byte of driver.bin is the entry point and every label resolves to a
; $0400-based address. Bank size $1000 -> driver.bin occupies $0400..$13FF,
; well clear of the host data at $2B00 / $4B00 / $4DB0.
; --------------------------------------------------------------------------
.MEMORYMAP
DEFAULTSLOT 0
SLOTSIZE $1000
SLOT 0 $0400
.ENDME

.ROMBANKMAP
BANKSTOTAL 1
BANKSIZE $1000
BANKS 1
.ENDRO

.EMPTYFILL $00

; --------------------------------------------------------------------------
; Tunable constants (exposed so the coordinator can retune by ear).
; --------------------------------------------------------------------------
.DEFINE DSP_DIR_PAGE   $4B      ; S-DSP sample-directory page (DIR reg)
.DEFINE TIMER_DIV      16       ; Timer0 target: base tick = 8000/16 = 500 Hz.
.DEFINE TEMPO_DEFAULT  $28      ; default tempo; engine tick = base*tempo/256
                                ; => 500*40/256 ~= 78 engine ticks/sec
.DEFINE INST_TABLE     $4C30    ; instrument table: 6 bytes/entry
                                ; b0=SRCN b1=ADSR1 b2=ADSR2 b3=GAIN
                                ; b4:b5 = base pitch, BIG-ENDIAN (b4=high, b5=low)
.DEFINE REF_NOTE       $80      ; note byte offset subtracted before octave/semitone
                                ; split (octave = (note-REF)/12). Tune octaves by ear.
.DEFINE OCT_REF        6        ; ratio >> (OCT_REF - octave); higher octave = higher
                                ; pitch. Raise/lower to shift every note by octaves.
.DEFINE PITCH_OUT_SHIFT 8       ; VxPITCH = (base*factor) >> PITCH_OUT_SHIFT.
                                ; Dial +/- a few to align the absolute octave by ear.
.DEFINE DEFAULT_BASE   $1000    ; per-instrument base multiplier before any $E0
.DEFINE MVOL_DEFAULT   $28      ; master volume on play (lowered to sit under SFX)
.DEFINE CHVOL_DEFAULT  $18      ; per-channel volume default (lowered)
.DEFINE PAN_CENTER     $40      ; pan center (0=hard L .. $7F=hard R)
.DEFINE VEL_DEFAULT    $FC      ; default curvel (VELTAB value, full)
.DEFINE QUANT_DEFAULT  $FC      ; default curquant (QUANTTAB value, full = legato)
.DEFINE ADSR1_DEF      $DF      ; default ADSR1 before any $E0 (enable, fast attack)
.DEFINE ADSR2_DEF      $C0      ; default ADSR2 before any $E0 (sustain 6/8, hold)
.DEFINE GAIN_DEF       $00      ; default GAIN before any $E0 (unused while ADSR on)
.DEFINE FADE_RATE      2        ; engine ticks per master-volume step during fade

; --------------------------------------------------------------------------
; Direct-page variables ($10..$77). Direct page 0 is selected (CLRP) so that
; the $F0..$FF hardware registers stay reachable as direct-page addresses.
; These addresses are RAM the host does not touch; they are never emitted
; into driver.bin (they are just address equates).
; --------------------------------------------------------------------------
.DEFINE wptr        $10      ; (word) working pointer for [wptr]+Y reads
.DEFINE songlp      $12      ; (word) cursor into the song list
.DEFINE tempo       $14      ; current tempo byte
.DEFINE mvol        $15      ; current master volume (both channels)
.DEFINE tickacc     $16      ; 8-bit tempo accumulator (carry = one engine tick)
.DEFINE lastp0      $17      ; last-seen port0 ($F4) command byte
.DEFINE lastp3      $18      ; last-seen port3 ($F7) command byte
.DEFINE state       $19      ; 0=idle 1=playing 2=fading
.DEFINE fadeacc     $1A      ; fade step accumulator
.DEFINE activemask  $1B      ; bit i set = track i still playing this frame
.DEFINE vbase       $1C      ; current voice DSP register base (voice<<4)
.DEFINE vbit        $1D      ; current voice bit (1<<voice)
.DEFINE vtmp        $1E      ; scratch for VDSP macro
.DEFINE tmp0        $1F      ; scratch (word low)
.DEFINE tmp1        $20      ; scratch (word high)
.DEFINE kofsoft     $21      ; software mirror of KOF bits
.DEFINE konpending  $22      ; key-on bits to latch at end of this tick
.DEFINE basecnt     $23      ; base-tick loop counter
.DEFINE savex       $24      ; save X across calc_pitch
.DEFINE octtmp      $25      ; octave counter in calc_pitch
.DEFINE vscaled     $26      ; velocity-scaled channel volume
.DEFINE volL        $27      ; computed left voice volume
.DEFINE volR        $28      ; computed right voice volume
.DEFINE p_note      $29      ; note_on params (snapshot to free X)
.DEFINE p_srcn      $2A
.DEFINE p_chvol     $2B
.DEFINE p_pan       $2C
.DEFINE p_vel       $2D
.DEFINE p_adsr1     $2E
.DEFINE p_adsr2     $2F

; Per-track arrays, 8 entries each, indexed by voice (X = 0..7).
.DEFINE tptrlo      $30      ; track stream pointer low
.DEFINE tptrhi      $38      ; track stream pointer high
.DEFINE tdur        $40      ; current note duration (engine ticks)
.DEFINE tdurrem     $48      ; ticks remaining on current note/rest
.DEFINE tvel        $50      ; per-track velocity (0..15)
.DEFINE tquant      $58      ; per-track quantization (0..7)
.DEFINE tsrcn       $60      ; per-track sample/instrument index
.DEFINE tchvol      $68      ; per-track channel volume
.DEFINE tpan        $70      ; per-track pan
.DEFINE tadsr1      $78      ; per-track ADSR1 (from instrument table)
.DEFINE tadsr2      $80      ; per-track ADSR2
.DEFINE tgain       $88      ; per-track GAIN
.DEFINE tbaselo     $90      ; per-track base pitch low
.DEFINE tbasehi     $98      ; per-track base pitch high

; More scalars (above the arrays).
.DEFINE mcl         $A0      ; 16x16 multiply: multiplicand (base) low
.DEFINE mch         $A1      ;                multiplicand high
.DEFINE fcl         $A2      ;                multiplier (factor) low
.DEFINE fch         $AB      ;                multiplier (factor) high
.DEFINE p0          $A3      ;                product byte 0 (lo)
.DEFINE p1          $A4      ;                product byte 1
.DEFINE p2          $A5      ;                product byte 2
.DEFINE p3          $AC      ;                product byte 3 (hi)
.DEFINE iptr        $A6      ; (word) pointer into the instrument table
.DEFINE p_gain      $A8      ; note_on: instrument GAIN snapshot
.DEFINE p_baselo    $A9      ; note_on: base pitch low snapshot
.DEFINE p_basehi    $AA      ; note_on: base pitch high snapshot
.DEFINE cmdb        $AD      ; current track command byte being parsed
.DEFINE cnt         $AE      ; operand-consume counter
.DEFINE op0         $AF      ; first operand of the current command
.DEFINE transpose   $B0      ; global transpose (signed semitones, $E9)
.DEFINE tgate       $B1      ; per-track note-gate countdown ($B1..$B8)

; --------------------------------------------------------------------------
; Macros for S-DSP register access via $F2 (address) / $F3 (data).
; --------------------------------------------------------------------------
; Global write, immediate register + immediate value.
.MACRO GDSP
    mov $f2, #\1
    mov $f3, #\2
.ENDM

; Global write, immediate register, value already in A.
.MACRO GDSPA
    mov $f2, #\1
    mov $f3, a
.ENDM

; Per-voice write: register = (vbase | offset); value already in A.
.MACRO VDSP
    mov vtmp, a
    mov a, #\1
    or  a, vbase
    mov $f2, a
    mov a, vtmp
    mov $f3, a
.ENDM

.BANK 0 SLOT 0
.ORG 0

.SECTION "driver" FORCE

; ==========================================================================
; Entry point ($0400)
; ==========================================================================
start:
    clrp                    ; direct page 0 (keeps $F0..$FF SFRs addressable)
    mov x, #$ff
    mov sp, x               ; stack at $01FF downward

    ; ---- S-DSP initialization ----
    GDSP $6c, $20           ; FLG: reset=0 mute=0 echo-writes-disabled(bit5) noise=0
    GDSP $5d, DSP_DIR_PAGE  ; DIR = sample directory page ($4B)
    GDSP $0c, $00           ; MVOLL = 0 (silent until a song plays)
    GDSP $1c, $00           ; MVOLR = 0
    GDSP $2c, $00           ; EVOLL = 0 (echo off)
    GDSP $3c, $00           ; EVOLR = 0
    GDSP $0d, $00           ; EFB  = 0
    GDSP $2d, $00           ; PMON = 0 (no pitch modulation)
    GDSP $3d, $00           ; NON  = 0 (no noise voices)
    GDSP $4d, $00           ; EON  = 0 (no echo per voice)
    GDSP $7d, $00           ; EDL  = 0 (echo buffer size 0 -> no ARAM writes)

    ; per-voice: silence volumes
    mov x, #0
init_v:
    mov a, !vbasetab+x
    mov vbase, a
    mov a, #0
    VDSP $00                ; VxVOLL = 0
    mov a, #0
    VDSP $01                ; VxVOLR = 0
    inc x
    cmp x, #8
    bne init_v

    GDSP $5c, $ff           ; KOF: key off all voices
    GDSP $4c, $00           ; KON: clear

    ; ---- Timer0 as engine heartbeat ----
    mov $fa, #TIMER_DIV     ; Timer0 divider
    mov $f1, #%00000001     ; enable Timer0 (leave port-clear bits untouched)
    mov a, $fd              ; read/clear Timer0 counter

    ; ---- engine state ----
    mov state, #0
    mov kofsoft, #$ff
    mov konpending, #0
    mov activemask, #0
    mov tempo, #TEMPO_DEFAULT
    mov tickacc, #0
    mov lastp0, #0       ; 0 so a command already latched by the host reads as new
    mov lastp3, #0

main:
    call !poll_comm
    call !service_timer
    jmp !main

; ==========================================================================
; Host comm-port ($F4..$F7). Commands are edge-detected on port0 ($F4) and
; port3 ($F7); acted on, then echoed back (host no longer needs the echo,
; SPEC.md, but it is harmless). Command semantics (SPEC.md):
;   port3 ($F7) != 0 while playing -> fade-out the current song
;   else port0 ($F4) in $01..$7F   -> play that song at $2B00 from the start
;   else (port0 $00, or $80..$FF)  -> stop / idle (bit7 set = stop, no restart)
; play has port3=0 and fade has port0=0, so the two never collide.
; ==========================================================================
poll_comm:
    mov a, $f4
    cmp a, lastp0
    bne pc_new
    mov a, $f7
    cmp a, lastp3
    bne pc_new
    ret                     ; command unchanged
pc_new:
    mov a, $f4
    mov lastp0, a
    mov a, $f7
    mov lastp3, a
    ; fade only if we are actually playing (state 1) and port3 is set
    mov a, state
    cmp a, #1
    bne pc_playcheck        ; idle or fading -> not a fresh fade
    mov a, lastp3
    beq pc_playcheck        ; port3 0 -> a play/stop command
    mov a, #2               ; begin fade-out
    mov state, a
    mov fadeacc, #0
    jmp !pc_echo
pc_playcheck:
    mov a, lastp0
    beq pc_stop             ; port0 $00 -> stop/idle
    bmi pc_stop             ; port0 bit7 set ($80..$FF) -> stop/idle, no restart
    jmp !pc_play            ; port0 $01..$7F -> play that song
pc_stop:
    call !stop_all
    jmp !pc_echo
pc_play:
    call !start_song         ; plays / restarts, fully resetting playback state
pc_echo:
    mov a, lastp0
    mov $f4, a
    mov a, lastp3
    mov $f7, a
    ret

; ==========================================================================
; Timer service: convert elapsed base ticks into engine ticks via the tempo
; accumulator, running the sequencer (and fade) once per engine tick.
; ==========================================================================
service_timer:
    mov a, state
    beq st_ret              ; idle: nothing to do
    mov a, $fd              ; elapsed base ticks (0..15), reading clears it
    beq st_ret
    mov basecnt, a
st_loop:
    mov a, tickacc
    clrc
    adc a, tempo
    mov tickacc, a
    bcc st_cont             ; no wrap past 256 -> no engine tick this base tick
    call !do_engine_tick
    mov a, state
    cmp a, #2
    bne st_cont
    call !fade_step
st_cont:
    dec basecnt
    bne st_loop
st_ret:
    ret

; ==========================================================================
; One engine tick: advance every active track; key-ons are batched and
; latched once (KON is cleared at the start of the tick so the previous
; tick's key-ons get an edge and then release the KON latch).
; ==========================================================================
do_engine_tick:
    mov a, state
    bne det_go
    ret
det_go:
    mov $f2, #$4c           ; KON = 0 (clear last tick's key-ons)
    mov $f3, #$00
    mov konpending, #0
    mov x, #0
det_loop:
    mov a, !bittab+x
    and a, activemask
    beq det_skip            ; track not active
    mov a, !vbasetab+x
    mov vbase, a
    mov a, !bittab+x
    mov vbit, a
    ; note gate: key off early once the gate ticks elapse (SPEC.md articulation).
    ; The note still occupies its full duration; only the key-on is shortened.
    mov a, tgate+x
    beq det_gdone          ; 0 -> no gate armed (rest / already released)
    dec tgate+x
    mov a, tgate+x
    bne det_gdone
    call !note_off          ; gate expired -> KOF this voice
det_gdone:
    dec tdurrem+x
    mov a, tdurrem+x
    bne det_skip            ; still occupying its duration
    call !parse_track        ; note/rest expired -> fetch next event(s)
det_skip:
    inc x
    cmp x, #8
    bne det_loop
    mov a, konpending       ; latch this tick's key-ons
    mov $f2, #$4c
    mov $f3, a
    mov a, activemask
    bne det_ret
    call !load_frame         ; all tracks ended -> next frame
    mov a, konpending       ; latch key-ons primed by the new frame
    mov $f2, #$4c
    mov $f3, a
det_ret:
    ret

; ==========================================================================
; Fade-out: step the master volume down toward 0, then stop.
; ==========================================================================
fade_step:
    inc fadeacc
    mov a, fadeacc
    cmp a, #FADE_RATE
    bcc fs_ret
    mov fadeacc, #0
    mov a, mvol
    beq fs_done             ; already silent
    dec a
    mov mvol, a
    mov $f2, #$0c           ; MVOLL
    mov $f3, a
    mov a, mvol
    mov $f2, #$1c           ; MVOLR
    mov $f3, a
    ret
fs_done:
    call !stop_all
fs_ret:
    ret

; ==========================================================================
; start_song: (re)start playback of the song at $2B00.
; ==========================================================================
start_song:
    call !stop_all          ; full cold reset: MVOL restored, voices off, state cleared
    mov a, #TEMPO_DEFAULT
    mov tempo, a
    mov tickacc, #0
    mov fadeacc, #0
    mov konpending, #0
    mov transpose, #0       ; clear global transpose for the new song
    mov a, !$2b00           ; song list pointer = word at $2B00
    mov songlp, a
    mov a, !$2b01
    mov songlp+1, a
    mov a, #1               ; mark playing (song-end handler clears it)
    mov state, a
    call !load_frame
    mov a, konpending       ; latch initial key-ons
    mov $f2, #$4c
    mov $f3, a
    ret

; ==========================================================================
; stop_all: silence everything and go idle.
; ==========================================================================
stop_all:
    mov state, #0
    mov activemask, #0
    mov konpending, #0
    mov tickacc, #0
    mov kofsoft, #$ff
    mov $f2, #$5c           ; KOF all
    mov $f3, #$ff
    mov $f2, #$4c           ; KON clear
    mov $f3, #$00
    ; restore master volume (never leave MVOL at 0 outside an active fade)
    mov a, #MVOL_DEFAULT
    mov mvol, a
    mov $f2, #$0c           ; MVOLL
    mov $f3, a
    mov $f2, #$1c           ; MVOLR
    mov $f3, a
    ret

; ==========================================================================
; load_frame: read the next song-list word and set up its frame (SPEC.md).
; Decode each 16-bit word by HIGH byte:
;   high != 0 -> frame pointer: play it (8 channel-track pointers).
;   high == 0 -> control word keyed by low byte:
;       low == 0 -> end of song: stop / idle.
;       low != 0 -> loop: NEXT word is the loop-target address; set the song
;                   pointer to it and keep reading (loop forever).
; ==========================================================================
load_frame:
lf_read:
    movw ya, songlp
    movw wptr, ya
    mov y, #0
    call !rdword             ; word -> tmp1:tmp0, wptr advanced past it
    movw ya, wptr
    movw songlp, ya          ; songlp now points at the next word
    mov a, tmp1
    bne lf_have             ; high byte != 0 -> frame pointer
    ; high byte == 0 -> control word
    mov a, tmp0
    bne lf_loop             ; low byte != 0 -> loop control
    call !stop_all           ; $0000 -> end of song (stop + idle)
    ret
lf_loop:                     ; ponytail: loop-forever; honor tmp0 as a finite
    call !rdword             ; repeat count if a song ever needs it.
    mov a, tmp0              ; next word = loop target address
    mov songlp, a
    mov a, tmp1
    mov songlp+1, a
    jmp !lf_read            ; resume reading from the loop target
lf_have:
    mov a, tmp0
    mov wptr, a
    mov a, tmp1
    mov wptr+1, a
    mov y, #0
    mov x, #0
lf_rd:
    call !rdword             ; track pointer i -> tmp1:tmp0
    mov a, tmp0
    mov tptrlo+x, a
    mov a, tmp1
    mov tptrhi+x, a
    inc x
    cmp x, #8
    bne lf_rd
    mov activemask, #0
    mov x, #0
lf_setup:
    mov a, tptrlo+x
    or  a, tptrhi+x
    beq lf_next             ; pointer 0 -> channel unused this frame
    mov a, #$01             ; per-track defaults (reset each frame; see README)
    mov tdur+x, a
    mov a, #VEL_DEFAULT
    mov tvel+x, a
    mov a, #QUANT_DEFAULT
    mov tquant+x, a
    mov a, #$00
    mov tsrcn+x, a
    mov a, #CHVOL_DEFAULT
    mov tchvol+x, a
    mov a, #PAN_CENTER
    mov tpan+x, a
    mov a, #ADSR1_DEF
    mov tadsr1+x, a
    mov a, #ADSR2_DEF
    mov tadsr2+x, a
    mov a, #GAIN_DEF
    mov tgain+x, a
    mov a, #<DEFAULT_BASE
    mov tbaselo+x, a
    mov a, #>DEFAULT_BASE
    mov tbasehi+x, a
    mov a, #$00
    mov tdurrem+x, a
    mov a, #$00
    mov tgate+x, a
    mov a, !bittab+x
    or  a, activemask
    mov activemask, a
    mov a, !vbasetab+x
    mov vbase, a
    mov a, !bittab+x
    mov vbit, a
    call !parse_track        ; prime first event
lf_next:
    inc x
    cmp x, #8
    bne lf_setup
    ret

; ==========================================================================
; rdword: read a little-endian 16-bit word through wptr into tmp1:tmp0,
; advancing wptr by 2. Requires Y=0.
; ==========================================================================
rdword:
    mov a, [wptr]+y
    incw wptr
    mov tmp0, a
    mov a, [wptr]+y
    incw wptr
    mov tmp1, a
    ret

; ==========================================================================
; parse_track: consume events for track X until one that occupies time
; (note / tie / rest) or end-of-track. On entry X=voice, vbase/vbit set.
; Zero-time commands (instrument, tempo, volume...) loop within here.
; ==========================================================================
parse_track:
    mov a, tptrlo+x
    mov wptr, a
    mov a, tptrhi+x
    mov wptr+1, a
    mov y, #0
pt_next:
    mov a, [wptr]+y
    incw wptr               ; INCW clobbers Z, so re-test the event byte below
    cmp a, #0
    bne pt_notend
    jmp !pt_endtrack         ; $00 = end of track
pt_notend:
    cmp a, #$80
    bcc pt_setdur           ; $01..$7F = set duration
    cmp a, #$c8
    bcc pt_note             ; $80..$C7 = note
    beq pt_tie              ; $C8 = tie
    cmp a, #$c9
    beq pt_rest             ; $C9 = rest
    jmp !pt_command          ; >= $CA = command

pt_setdur:
    mov tdur+x, a
    mov a, [wptr]+y         ; peek: optional velocity/quant byte iff < $80
    cmp a, #$80
    bcs pt_next             ; >=$80 -> it's the next event, reparse it
    incw wptr               ; consume the velocity byte
    mov tmp0, a             ; velocity byte ($00..$7F)
    mov savex, x            ; free X to index the ROM tables
    and a, #$0f             ; vel_index = byte & $0F
    mov x, a
    mov a, !veltab+x        ; curvel = VELTAB[vel_index]
    mov tmp1, a
    mov a, tmp0
    lsr a
    lsr a
    lsr a
    lsr a
    and a, #7              ; quant_index = (byte >> 4) & 7
    mov x, a
    mov a, !quanttab+x     ; curquant = QUANTTAB[quant_index]
    mov x, savex           ; restore voice index
    mov tquant+x, a
    mov a, tmp1
    mov tvel+x, a
    jmp !pt_next

pt_note:
    call !note_on            ; A=note byte, X=voice
    mov a, tdur+x
    mov tdurrem+x, a
    jmp !pt_save
pt_tie:
    mov a, tdur+x           ; extend previous note, no new key-on
    mov tdurrem+x, a
    mov tgate+x, a          ; hold the note for the whole tie (no early gate-off)
    jmp !pt_save
pt_rest:
    call !note_off
    mov a, #0
    mov tgate+x, a          ; silent -> no gate
    mov a, tdur+x
    mov tdurrem+x, a
    jmp !pt_save
pt_save:
    mov a, wptr
    mov tptrlo+x, a
    mov a, wptr+1
    mov tptrhi+x, a
    ret

pt_endtrack:
    call !note_off
    mov a, vbit             ; clear this track's active bit
    eor a, #$ff
    and a, activemask
    mov activemask, a
    ret

pt_command:                 ; A = command byte ($E0..$FA)
    ; Consume its FIXED operand count from cmdlen (SPEC.md) to stay synced, then
    ; act on the subset we support. cmdlen index = cmd - $E0.
    mov cmdb, a
    setc
    sbc a, #$E0
    cmp a, #27              ; entries for $E0..$FA; out of range -> 0 operands
    bcs pt_cmd_act
    mov savex, x           ; free X to index cmdlen
    mov x, a
    mov a, !cmdlen+x       ; A = operand count
    mov x, savex           ; restore X (this load clobbers the Z flag)
    cmp a, #0              ; re-test the count, not the restored X value
    beq pt_cmd_act         ; zero-operand command
    mov cnt, a
    mov a, [wptr]+y        ; first operand -> op0
    incw wptr
    mov op0, a
    dec cnt
    beq pt_cmd_act
pt_cmd_more:
    mov a, [wptr]+y        ; consume the remaining operands
    incw wptr
    dec cnt
    bne pt_cmd_more
pt_cmd_act:
    mov a, cmdb
    cmp a, #$e0
    beq pt_instr
    cmp a, #$e5
    beq pt_mvol
    cmp a, #$e7
    beq pt_tempo
    cmp a, #$e9
    beq pt_transpose
    cmp a, #$ea
    beq pt_chvol
    cmp a, #$ed
    beq pt_pan
    jmp !pt_next           ; unsupported: operands already consumed
pt_instr:                   ; $E0 nn -> load instrument-table entry nn (nn = op0)
    mov a, op0
    mov y, #6
    mul ya                  ; YA = nn*6
    clrc
    adc a, #<INST_TABLE
    mov iptr, a
    mov a, y
    adc a, #>INST_TABLE
    mov iptr+1, a
    mov y, #0
    mov a, [iptr]+y
    mov tsrcn+x, a          ; b0 = SRCN
    mov y, #1
    mov a, [iptr]+y
    mov tadsr1+x, a         ; b1 = ADSR1
    mov y, #2
    mov a, [iptr]+y
    mov tadsr2+x, a         ; b2 = ADSR2
    mov y, #3
    mov a, [iptr]+y
    mov tgain+x, a          ; b3 = GAIN
    mov y, #4
    mov a, [iptr]+y
    mov tbasehi+x, a        ; b4 = base pitch HIGH byte (big-endian)
    mov y, #5
    mov a, [iptr]+y
    mov tbaselo+x, a        ; b5 = base pitch LOW byte
    mov y, #0               ; restore Y for the [wptr]+Y stream reads
    jmp !pt_next
pt_mvol:                    ; $E5 vv -> master volume
    mov a, op0
    mov mvol, a
    mov $f2, #$0c
    mov $f3, a
    mov a, mvol
    mov $f2, #$1c
    mov $f3, a
    jmp !pt_next
pt_tempo:                   ; $E7 tt -> tempo
    mov a, op0
    mov tempo, a
    jmp !pt_next
pt_transpose:               ; $E9 nn -> global transpose (signed semitones)
    mov a, op0
    mov transpose, a
    jmp !pt_next
pt_chvol:                   ; $EA xx -> channel volume
    mov a, op0
    mov tchvol+x, a
    jmp !pt_next
pt_pan:                     ; $ED xx -> pan
    mov a, op0
    mov tpan+x, a
    jmp !pt_next

; ==========================================================================
; note_on: set up voice X for a new note and queue its key-on.
; A = note byte ($80..$C7). vbase/vbit already set for voice X.
; ==========================================================================
note_on:
    clrc                    ; apply global transpose (default 0) to the pitch note
    adc a, transpose
    mov p_note, a
    mov a, tsrcn+x          ; snapshot per-track params so X is free afterwards
    mov p_srcn, a
    mov a, tchvol+x
    mov p_chvol, a
    mov a, tpan+x
    mov p_pan, a
    mov a, tvel+x
    mov p_vel, a
    mov a, tadsr1+x
    mov p_adsr1, a
    mov a, tadsr2+x
    mov p_adsr2, a
    mov a, tgain+x
    mov p_gain, a
    mov a, tbaselo+x
    mov p_baselo, a
    mov a, tbasehi+x
    mov p_basehi, a

    ; note gate = (curdur * curquant) >> 8 ticks, minimum 1 (SPEC.md articulation)
    mov a, tdur+x
    mov y, tquant+x
    mul ya                  ; YA = curdur*curquant ; Y = >>8
    mov a, y
    bne no_gate_ok
    inc a                   ; clamp to at least 1 tick
no_gate_ok:
    mov tgate+x, a

    mov a, p_srcn
    VDSP $04                ; VxSRCN
    mov a, p_adsr1
    VDSP $05                ; VxADSR1 (bit7 set in data -> envelope from ADSR)
    mov a, p_adsr2
    VDSP $06                ; VxADSR2
    mov a, p_gain
    VDSP $07                ; VxGAIN  (fallback when ADSR1 bit7 = 0)

    mov a, p_note
    call !calc_pitch         ; pitch -> tmp1:tmp0 (preserves X)
    mov a, tmp0
    VDSP $02                ; VxPITCHL
    mov a, tmp1
    VDSP $03                ; VxPITCHH

    call !calc_vol           ; -> volL / volR
    mov a, volL
    VDSP $00                ; VxVOLL
    mov a, volR
    VDSP $01                ; VxVOLR

    mov a, vbit             ; queue key-on
    or  a, konpending
    mov konpending, a
    mov a, vbit             ; drop this voice's KOF bit and rewrite KOF
    eor a, #$ff
    and a, kofsoft
    mov kofsoft, a
    mov $f2, #$5c
    mov a, kofsoft
    mov $f3, a
    ret

; ==========================================================================
; note_off: key off voice (vbit set); also drop any pending key-on.
; ==========================================================================
note_off:
    mov a, vbit
    or  a, kofsoft
    mov kofsoft, a
    mov $f2, #$5c
    mov a, kofsoft
    mov $f3, a
    mov a, vbit
    eor a, #$ff
    and a, konpending
    mov konpending, a
    ret

; ==========================================================================
; calc_pitch: note byte -> 14-bit VxPITCH in tmp1:tmp0. Per SPEC.md:
;   n        = note - REF_NOTE
;   octave   = n / 12,  semitone = n % 12          (octave is a bit shift)
;   factor   = ratiotab[semitone] >> (OCT_REF - octave)   (left if octave>OCT_REF)
;   VxPITCH  = (instrument_base16 * factor) >> 8    (16x16 multiply, then >>8)
;   clamp to $3FFF
; ratiotab = round($085F * 2^(k/12)); instrument base (p_basehi:p_baselo) is a
; per-instrument tuning multiplier (fed in raw, never sanitized). Preserves X.
; ==========================================================================
calc_pitch:
    mov savex, x
    setc
    sbc a, #REF_NOTE        ; A = n (notes are >= REF_NOTE)
    mov octtmp, #0
cp_div:
    cmp a, #12
    bcc cp_have
    setc
    sbc a, #12
    inc octtmp             ; octtmp = octave
    jmp !cp_div
cp_have:
    asl a                   ; semitone * 2 -> word index
    mov x, a
    mov a, !ratiotab+x
    mov fcl, a
    inc x
    mov a, !ratiotab+x
    mov fch, a              ; factor = ratiotab[semitone] (before octave shift)
    ; shift amount = OCT_REF - octave; >=0 shift right, <0 shift left
    mov a, #OCT_REF
    cmp a, octtmp
    bcc cp_left            ; OCT_REF < octave -> shift left
    setc
    sbc a, octtmp          ; A = OCT_REF - octave (right shift count)
    mov octtmp, a
cp_rs:
    mov a, octtmp
    beq cp_mul
    lsr fch
    ror fcl
    dec octtmp
    jmp !cp_rs
cp_left:
    mov a, octtmp
    setc
    sbc a, #OCT_REF        ; A = octave - OCT_REF (left shift count)
    mov octtmp, a
cp_ls:
    mov a, octtmp
    beq cp_mul
    asl fcl
    rol fch
    dec octtmp
    jmp !cp_ls
cp_mul:
    mov a, p_baselo
    mov mcl, a
    mov a, p_basehi
    mov mch, a
    call !mul16x16         ; p3:p2:p1:p0 = base * factor
    ; VxPITCH = product >> PITCH_OUT_SHIFT, then take low word; clamp to 14 bits
    mov octtmp, #PITCH_OUT_SHIFT
cp_out:
    mov a, octtmp
    beq cp_outdone
    lsr p3
    ror p2
    ror p1
    ror p0
    dec octtmp
    jmp !cp_out
cp_outdone:
    mov a, p3
    bne cp_sat             ; high bytes still set -> overflow
    mov a, p2
    bne cp_sat
    mov a, p1
    cmp a, #$40
    bcs cp_sat             ; >= $4000 -> overflow
    mov a, p0
    mov tmp0, a
    mov a, p1
    mov tmp1, a
    jmp !cp_done
cp_sat:
    mov tmp0, #$ff
    mov tmp1, #$3f
cp_done:
    mov x, savex
    ret

; ==========================================================================
; mul16x16: p3:p2:p1:p0 = (mch:mcl) * (fch:fcl), via four 8x8 MUL YA. X unused.
; ==========================================================================
mul16x16:
    mov y, fcl
    mov a, mcl
    mul ya                  ; mcl*fcl
    mov p0, a
    mov p1, y
    mov p2, #0
    mov p3, #0
    mov y, fch             ; + (mcl*fch) << 8
    mov a, mcl
    mul ya
    clrc
    adc a, p1
    mov p1, a
    mov a, y
    adc a, p2
    mov p2, a
    mov a, #0
    adc a, p3
    mov p3, a
    mov y, fcl             ; + (mch*fcl) << 8
    mov a, mch
    mul ya
    clrc
    adc a, p1
    mov p1, a
    mov a, y
    adc a, p2
    mov p2, a
    mov a, #0
    adc a, p3
    mov p3, a
    mov y, fch             ; + (mch*fch) << 16
    mov a, mch
    mul ya
    clrc
    adc a, p2
    mov p2, a
    mov a, y
    adc a, p3
    mov p3, a
    ret

; ==========================================================================
; calc_vol: compute signed L/R voice volumes from channel volume, velocity
; and pan. vscaled = (curvel * chvol) >> 8; then split by pan. curvel is the
; VELTAB value (0..$FC). Uses MUL YA (Y*A -> YA, high byte in Y). Restores Y=0.
; ==========================================================================
calc_vol:
    mov y, p_vel            ; curvel (VELTAB value)
    mov a, p_chvol
    mul ya                  ; YA = curvel*chvol; Y = >>8 result
    mov vscaled, y

    mov a, #127             ; left gain = min($FC, (127-pan)*4)
    setc
    sbc a, p_pan
    cmp a, #$40
    bcc cv_lok
    mov a, #$3f
cv_lok:
    asl a
    asl a
    mov y, a
    mov a, vscaled
    mul ya
    mov volL, y

    mov a, p_pan            ; right gain = min($FC, pan*4)
    cmp a, #$40
    bcc cv_rok
    mov a, #$3f
cv_rok:
    asl a
    asl a
    mov y, a
    mov a, vscaled
    mul ya
    mov volR, y

    mov y, #0
    ret

; --------------------------------------------------------------------------
; Data tables
; --------------------------------------------------------------------------
; voice bit masks (1<<voice)
bittab:
    .db $01, $02, $04, $08, $10, $20, $40, $80
; voice DSP register base (voice<<4)
vbasetab:
    .db $00, $10, $20, $30, $40, $50, $60, $70
; semitone ratios within one octave: round($085F * 2^(k/12)), k=0..11
; (SPEC.md values), little-endian 16-bit words.
ratiotab:
    .dw $085F, $08DE, $0965, $09F4, $0A8C, $0B2C
    .dw $0BD6, $0C8B, $0D4A, $0E14, $0EEA, $0FCD
; fixed operand-byte count per track command $E0..$FA (SPEC.md). Index = cmd-$E0.
;        E0 E1 E2 E3 E4 E5 E6 E7 E8 E9 EA EB EC ED EE EF F0 F1 F2 F3 F4 F5 F6 F7 F8 F9 FA
cmdlen:
    .db  1, 1, 2, 3, 0, 1, 2, 1, 2, 1, 1, 3, 0, 1, 2, 3, 1, 3, 3, 0, 1, 3, 0, 3, 3, 3, 1
; note-gate fraction per quant index (SPEC.md). curquant = QUANTTAB[(velbyte>>4)&7]
quanttab:
    .db $32, $65, $7F, $98, $B2, $CB, $E5, $FC
; per-note volume per velocity index (SPEC.md). curvel = VELTAB[velbyte&$0F]
veltab:
    .db $19, $32, $4C, $65, $72, $7F, $8C, $98
    .db $A5, $B2, $BF, $CB, $D8, $E5, $F2, $FC

.ENDS
