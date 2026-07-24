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
.DEFINE OCT_REF        5        ; ratio >> (OCT_REF - octave); higher octave = higher
                                ; pitch. Raise/lower to shift every note by octaves.
.DEFINE PITCH_OUT_SHIFT 8       ; VxPITCH = (base*factor) >> PITCH_OUT_SHIFT.
                                ; Dial +/- a few to align the absolute octave by ear.
.DEFINE DEFAULT_BASE   $1000    ; per-instrument base multiplier before any $E0
.DEFINE SGB_MASTER     $60      ; DSP main-volume target: the SGB hardware master
                                ; output level, faded in on play (SPEC.md "Master
                                ; volume"). DSP MVOL is signed -- never a song byte,
                                ; only this driver-owned level reaches it.
.DEFINE CHVOL_DEFAULT  $FF      ; per-channel volume default: the reference initializes
                                ; every channel's volume to $FF at song start (a $ED
                                ; command overrides it per channel). The final (t*t)>>8
                                ; square in calc_vol supplies the attenuation, so the
                                ; default is full-scale, not pre-attenuated (SPEC.md
                                ; "Per-voice volume").
.DEFINE PAN_CENTER     $40      ; pan center (0=hard L .. $7F=hard R)
.DEFINE VEL_DEFAULT    $FC      ; default curvel (VELTAB value, full)
.DEFINE QUANT_DEFAULT  $FC      ; default curquant (QUANTTAB value, full = legato)
.DEFINE ADSR1_DEF      $DF      ; default ADSR1 before any $E0 (enable, fast attack)
.DEFINE ADSR2_DEF      $C0      ; default ADSR2 before any $E0 (sustain 6/8, hold)
.DEFINE GAIN_DEF       $00      ; default GAIN before any $E0 (unused while ADSR on)
.DEFINE FADE_IN_RATE   8        ; BASE ticks per +1 DSP main-volume step during the master
                                ; fade-IN (mvol climbs to SGB_MASTER, 96 steps). The base
                                ; tick is ~500 Hz, so 8 base ticks/step ramps in ~768 base
                                ; ticks ~= 1.5 s. By-ear tunable; raise to slow.
.DEFINE FADE_OUT_RATE  4        ; BASE ticks per -1 DSP main-volume step during the master
                                ; fade-OUT (mvol falls to 0 on a stop command, 96 steps
                                ; ~= 1.3 s). Must finish + stop the song BEFORE the host
                                ; reloads $2B00 for the next song, or the still-running
                                ; sequencer reads the new song's bytes through stale track
                                ; cursors -> a garbage held note (SPEC.md $80..$FF). By-ear
                                ; tunable; raise to slow the fade-out.

; --------------------------------------------------------------------------
; Direct-page variables ($10..$77). Direct page 0 is selected (CLRP) so that
; the $F0..$FF hardware registers stay reachable as direct-page addresses.
; These addresses are RAM the host does not touch; they are never emitted
; into driver.bin (they are just address equates).
; --------------------------------------------------------------------------
.DEFINE wptr        $10      ; (word) working pointer for [wptr]+Y reads
.DEFINE songlp      $12      ; (word) cursor into the song list
.DEFINE tempo       $14      ; current tempo byte
.DEFINE mvol        $15      ; current DSP main volume (MVOL L/R): set to 0 only at boot,
                             ; then a single slew-limited value that chases mastertarget in
                             ; either direction (fade-in up / fade-out down); persists across
                             ; stops and song changes (SPEC.md "Master volume & fade-in").
.DEFINE tickacc     $16      ; 8-bit tempo accumulator (carry = one engine tick)
.DEFINE lastp0      $17      ; last-seen port0 ($F4) command byte
.DEFINE lastp3      $18      ; last-seen port3 ($F7) command byte
.DEFINE state       $19      ; 0=idle 1=playing. A fade-out does NOT change state: the
                             ; song keeps playing (state stays 1) while ramp_master slews
                             ; the master to 0, then ramp_master's stop_all idles it
                             ; (SPEC.md fade-OUT -- the music keeps playing during the fade).
.DEFINE fadeacc     $1A      ; master-volume slew accumulator (shared by fade-in/out)
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
.DEFINE ttrans      $B9      ; per-track transpose ($B9..$C0), signed semitones ($EA)
.DEFINE newptlo     $C1      ; frame-load scratch: new track pointer low  ($C1..$C8)
.DEFINE newpthi     $C9      ; frame-load scratch: new track pointer high ($C9..$D0)
.DEFINE songvol     $D1      ; ($E5) software song-master scalar (vv/256), folded into
                             ; every voice volume in software; NOT a DSP register --
                             ; DSP MVOL is signed (SPEC.md "Master volume"). Default $FF.
.DEFINE mastertarget $D2     ; DSP-master target that the slew-limited mvol chases:
                             ; SGB_MASTER while a song is active (boot + start_song), 0 on a
                             ; stop/fade-out command. One value, either direction (ramp_master,
                             ; SPEC.md "Master volume & fade-in").
.DEFINE booted      $D3      ; 0 until the one-time boot fade-in reaches SGB_MASTER, then 1.
                             ; Gates the fade-in as boot-ONLY: once set, start_song snaps the
                             ; master to full instead of re-running the slow fade (SPEC.md
                             ; "Master volume & fade-in": boot fade-IN one-time / play snaps).

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
    mov mvol, #0         ; boot the software master at 0 to agree with the DSP MVOL
                         ; init above -- the ONLY place the master starts at 0; it
                         ; then slews up to $60 once (SPEC.md "Master volume & fade-in")
    mov mastertarget, #SGB_MASTER   ; chase $60 from boot (fade-in emerges from WHEN a
                                    ; song starts relative to this ramp; SPEC.md)
    mov booted, #0       ; the one-time boot fade-in has not completed yet; ramp_master
                         ; sets this once mvol reaches SGB_MASTER (SPEC.md boot-only fade-in)
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
; Host comm-port. Only port0 ($F4) — the Music Score Code — controls music;
; ports 1..3 are the SGB sound-effect bytes and MUST NOT touch playback, so we
; edge-detect a change on port0 alone (SPEC.md "Host comm-port protocol"). A
; sound effect fires with score $00 and arbitrary nonzero port1..3 while a song
; plays, and the song must keep playing. On a changed port0:
;   $00        -> no music change (SFX-only command; leave the song playing)
;   $01..$7F   -> play that song at $2B00 from the start
;   $80..$FF   -> fade out: drop mastertarget to 0 and let ramp_master slew the master
;                 down. The song KEEPS PLAYING NORMALLY meanwhile -- state stays 1, the
;                 sequencer runs, new notes key on as MVOL falls $60->0 (SPEC.md fade-OUT:
;                 do NOT freeze the sequencer). ramp_master keys voices off + idles (via
;                 stop_all) when it reaches 0 (bit7 set = stop, not a song index; no
;                 restart). NOT an instant cut. FADE_OUT_RATE is fast enough that the
;                 song stops before the host reloads $2B00 for the next song.
; Then echo port0 back (harmless, not required).
; ==========================================================================
poll_comm:
    mov a, $f4
    cmp a, lastp0
    bne pc_new
    ret                     ; port0 unchanged -> not a new music command
pc_new:
    mov a, $f4
    mov lastp0, a           ; store does not touch flags; N/Z still reflect port0
    beq pc_echo             ; $00 -> SFX-only, leave the current song playing
    bmi pc_stop             ; bit7 set ($80..$FF) -> fade out, no restart
    call !start_song        ; $01..$7F -> play that song, resetting playback state
    jmp !pc_echo
pc_stop:
    mov mastertarget, #0    ; fade out: master slews to 0; ramp_master keys voices off +
                            ; idles (stop_all) when it lands (SPEC.md). NOT instant. No
                            ; stop_all here -- the master must slew down first. Do NOT touch
                            ; state: the song keeps playing (state stays 1, sequencer runs)
                            ; while the master slews down (SPEC.md fade-OUT: no freeze).
pc_echo:
    mov a, lastp0
    mov $f4, a
    ret

; ==========================================================================
; Timer service: convert elapsed base ticks into engine ticks via the tempo
; accumulator, running the sequencer once per engine tick. The master-volume slew
; (ramp_master) runs first, every base tick, whether idle or playing.
; ==========================================================================
service_timer:
    mov a, $fd              ; elapsed base ticks (0..15), reading clears it -- read
    beq st_ret             ; ALWAYS, before the play-state gate, so the master ramp
    mov basecnt, a         ; runs whether idle or playing
    call !ramp_master       ; master slew toward mastertarget: advances every base tick
                           ; regardless of play state (SPEC.md "Master volume & fade-in")
    mov a, state
    beq st_ret             ; idle (state 0) skips the sequencer; any playing state runs it.
                           ; The song keeps sequencing all through a fade-out (SPEC.md
                           ; fade-OUT: no freeze); ramp_master already ran above regardless.
st_loop:
    mov a, tickacc
    clrc
    adc a, tempo
    mov tickacc, a
    bcc st_cont             ; no wrap past 256 -> no engine tick this base tick
    call !do_engine_tick
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
    ; Channel 0 is the conductor: advance the frame the moment voice 0 stops, i.e.
    ; when its $00 has cleared bit0 of activemask (SPEC.md "Frame advance"). A $00 on
    ; any other channel only clears its own bit and never advances. AND sets Z for the
    ; branch below it (no clobber between).
    mov a, activemask
    and a, #$01
    bne det_ret              ; channel 0 still playing -> stay on this frame
    call !load_frame         ; conductor reached $00 -> next frame
    mov a, konpending       ; latch key-ons primed by the new frame
    mov $f2, #$4c
    mov $f3, a
det_ret:
    ret

; ==========================================================================
; ramp_master: slew the DSP main volume (SGB hardware master, $0C/$1C MVOL L/R)
; toward mastertarget -- one routine, both directions (SPEC.md "Master volume &
; fade-in"). Paced by the base-tick timer and called from service_timer BEFORE the
; play-state gate, so it advances on every elapsed base tick whether idle or
; playing. Whether a song audibly fades IN depends only on WHEN it starts relative
; to the boot ramp toward $60; a stop command drops mastertarget to 0 and the same
; slew fades OUT, keying voices off + going idle when it lands. Driven only by the
; driver, never by song data (MVOL is signed, so a song byte there mutes). Iterates
; on a COPY of basecnt in Y -- must not disturb basecnt, which st_loop still needs.
; ==========================================================================
ramp_master:
    mov a, mvol
    cmp a, mastertarget    ; CMP A,dp: sets Z (equal) and C (mvol >= target)
    beq rm_ret             ; at target -> hold, touch nothing
    bcc rm_up              ; mvol < target -> fade in (step up)
    ; mvol > target -> fade out (step down)
    mov y, basecnt         ; copy the elapsed base-tick count; do NOT decrement basecnt
rm_dn:
    inc fadeacc
    mov a, fadeacc
    cmp a, #FADE_OUT_RATE
    bcc rm_dn_next         ; not enough base ticks elapsed for the next -1 step
    mov fadeacc, #0
    dec mvol
    mov a, mvol
    mov $f2, #$0c          ; MVOLL
    mov $f3, a
    mov $f2, #$1c          ; MVOLR
    mov $f3, a
    mov a, mvol
    beq rm_dn_done         ; reached 0 -> fade-out complete
    cmp a, mastertarget
    beq rm_ret             ; reached a nonzero target -> hold
rm_dn_next:
    dec y
    bne rm_dn
    ret
rm_dn_done:
    call !stop_all         ; fade-out done: voices off + idle (SPEC.md). Leaves mvol/MVOL
    ret                    ; at 0; a later play sets mastertarget=$60 and fades back in.
rm_up:
    mov y, basecnt         ; copy the elapsed base-tick count; do NOT decrement basecnt
rm_up_loop:
    inc fadeacc
    mov a, fadeacc
    cmp a, #FADE_IN_RATE
    bcc rm_up_next         ; not enough base ticks elapsed for the next +1 step
    mov fadeacc, #0
    inc mvol
    mov a, mvol
    mov $f2, #$0c          ; MVOLL
    mov $f3, a
    mov $f2, #$1c          ; MVOLR
    mov $f3, a
    mov a, mvol
    cmp a, mastertarget
    bne rm_up_next         ; not at target yet -> keep ramping
    mov booted, #1         ; up-ramp reached SGB_MASTER: the one-time boot fade-in is done.
    ret                    ; From now start_song snaps to full instead of re-fading (SPEC.md
                           ; boot-only fade-IN). Only the up path sets it; the down path
                           ; (fade-out to 0) must not touch booted.
rm_up_next:
    dec y
    bne rm_up_loop
rm_ret:
    ret

; ==========================================================================
; start_song: (re)start playback of the song at $2B00.
; ==========================================================================
start_song:
    call !stop_all          ; cold reset: voices off, state cleared. Leaves the
                            ; master untouched, so a song starting while it is
                            ; already at $60 does NOT re-fade (SPEC.md).
    mov a, #TEMPO_DEFAULT
    mov tempo, a
    mov tickacc, #0
    mov fadeacc, #0
    mov mastertarget, #SGB_MASTER   ; chase $60 again: a song played after a fade-out left
                                    ; the master at 0, so it must fade back in (SPEC.md)
    mov songvol, #$ff       ; song-master scalar full until an $E5 lowers it (SPEC.md)
    mov konpending, #0
    mov transpose, #0       ; clear global transpose for the new song
    mov x, #7               ; clear per-channel transpose ($EA) for all 8 tracks
    mov a, #0
clr_ttrans:
    mov ttrans+x, a
    dec x
    bpl clr_ttrans
    mov a, !$2b00           ; song list pointer = word at $2B00
    mov songlp, a
    mov a, !$2b01
    mov songlp+1, a
    ; Play may precede the transfer (SPEC.md): the host can set the play score
    ; before the song data finishes copying into $2Bxx, and while it is in flight
    ; the pointer high byte ($2B01) reads 0. Starting on a 0 pointer walks zero-page
    ; into a $0000 end-word and latches silence forever, so treat a 0 high byte as
    ; not-yet-ready: stay idle, do not consume the command -- un-latch lastp0 (the
    ; play score is always nonzero) so poll_comm re-detects the same score next
    ; iteration and retries until the data lands.
    mov a, songlp+1
    bne ss_ready
    mov lastp0, #0
    ret
ss_ready:
    ; Play snaps the master to full -- but only AFTER the one-time boot fade-in
    ; (booted != 0). A song starting after a fade-out (which drove the master to 0)
    ; then begins at full $60 with NO re-fade (SPEC.md "Master volume": play snaps to
    ; full). If booted == 0 we are still in the boot ramp: leave mvol alone so the
    ; slow boot fade-in keeps ramping (mastertarget = SGB_MASTER, set above, still
    ; drives ramp_master's up-slew). mov a,booted sets Z with no clobber before beq.
    mov a, booted
    beq ss_play
    mov a, #SGB_MASTER
    mov mvol, a            ; snap the slew-limited master to full immediately
    mov $f2, #$0c          ; MVOLL = SGB_MASTER
    mov $f3, a
    mov $f2, #$1c          ; MVOLR = SGB_MASTER
    mov $f3, a
ss_play:
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
    ; Do NOT touch mvol or the DSP main-volume regs ($0C/$1C): the SGB hardware
    ; master is persistent and slew-limited -- set to 0 only at boot, then it
    ; chases $60 forever (SPEC.md "Master volume & fade-in"). Stopping keys the
    ; voices off but leaves the master where it is, so the next song starts at
    ; full with no re-fade.
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
    mov y, #0               ; rdword needs Y=0, but the movw ya,wptr above left Y =
                            ; high(wptr); without this the loop-target read derails.
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
    call !rdword             ; new track pointer i -> tmp1:tmp0. Read into scratch,
    mov a, tmp0             ; NOT tptr: a null new pointer means "leave the channel
    mov newptlo+x, a       ; running", so it must not clobber that channel's live
    mov a, tmp1            ; read position. (rdword also reuses wptr, which parse_track
    mov newpthi+x, a       ; below clobbers -- so read all 8 first, then set up.)
    inc x
    cmp x, #8
    bne lf_rd
    ; Do NOT clear activemask: a channel the new frame leaves 0 keeps playing its
    ; current track (a long line rides across the frame boundary, SPEC.md).
    mov x, #0
lf_setup:
    mov a, newptlo+x
    or  a, newpthi+x
    beq lf_next             ; new pointer 0 -> leave this channel running, untouched
    mov a, newptlo+x       ; non-zero -> (re)start this channel on the new track
    mov tptrlo+x, a
    mov a, newpthi+x
    mov tptrhi+x, a
    mov a, #$01             ; per-track defaults (reset for the (re)started channel)
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

pt_endtrack:                ; $00 = end of track: stop THIS channel (key off + drop
                            ; its active bit); it rests until the next frame reloads it.
                            ; It does not loop. Channel 0's stop is what advances the
                            ; frame (see do_engine_tick); any other channel just stops.
    call !note_off
    mov a, vbit
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
    cmp a, #$e1
    beq pt_pan             ; $E1 = pan
    cmp a, #$e5
    beq pt_mvol
    cmp a, #$e7
    beq pt_tempo
    cmp a, #$e9
    beq pt_transpose       ; $E9 = global transpose
    cmp a, #$ea
    beq pt_chtrans         ; $EA = per-channel transpose
    cmp a, #$ed
    beq pt_chvol           ; $ED = channel volume
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
pt_mvol:                    ; $E5 vv -> song-master scalar (SPEC.md "Master volume"):
    mov a, op0              ; a SOFTWARE per-voice scalar (vv/256), NOT the DSP main
    mov songvol, a          ; volume -- DSP MVOL is signed, so a song byte ($F8 = -8)
    jmp !pt_next            ; there would mute. Folded into calc_vol instead.
pt_tempo:                   ; $E7 tt -> tempo
    mov a, op0
    mov tempo, a
    jmp !pt_next
pt_transpose:               ; $E9 nn -> global transpose (signed semitones)
    mov a, op0
    mov transpose, a
    jmp !pt_next
pt_chtrans:                 ; $EA nn -> per-channel transpose (signed semitones)
    mov a, op0
    mov ttrans+x, a
    jmp !pt_next
pt_chvol:                   ; $ED vv -> channel volume
    mov a, op0
    mov tchvol+x, a
    jmp !pt_next
pt_pan:                     ; $E1 pp -> pan
    mov a, op0
    mov tpan+x, a
    jmp !pt_next

; ==========================================================================
; note_on: set up voice X for a new note and queue its key-on.
; A = note byte ($80..$C7). vbase/vbit already set for voice X.
; ==========================================================================
note_on:
    clrc                    ; apply global ($E9) then per-channel ($EA) transpose
    adc a, transpose        ; both default 0; signed semitones, byte-wraps
    clrc
    adc a, ttrans+x
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
; calc_vol: compute signed L/R voice volumes from channel volume, velocity,
; the song-master scalar and pan (SPEC.md "Per-voice volume"). For each side the
; reference chain is: pan_gain, *songvol, *velocity, *chvol, then a FINAL square:
;   vscaled = ((curvel * chvol) >> 8) * songvol >> 8   ; $E5 applied ONCE
;   volL    = (vscaled * left_gain) >> 8               ; pan
;   volL    = (volL * volL) >> 8                        ; final square (attenuation)
;   (likewise volR with right_gain). The single square over the fully-accumulated
;   per-side value IS the attenuation -- it is NOT applying songvol twice.
; curvel is the VELTAB value (0..$FC). Uses MUL YA (Y*A -> YA, high in Y). Y=0 out.
; ==========================================================================
calc_vol:
    mov y, p_vel            ; curvel (VELTAB value)
    mov a, p_chvol
    mul ya                  ; YA = curvel*chvol; Y = >>8 result
    mov vscaled, y
    mov y, songvol          ; fold in the $E5 song-master scalar ONCE (SPEC.md
    mov a, vscaled          ; "Per-voice volume"): vscaled = (vscaled * songvol) >> 8
    mul ya
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
    mov volL, y            ; volL = (vscaled * left_gain) >> 8 (pan)
    mov y, volL           ; final square (reference attenuation): volL = (volL*volL)>>8
    mov a, volL
    mul ya                ; SPEC.md squares the fully-accumulated per-side value
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
    mov volR, y            ; volR = (vscaled * right_gain) >> 8 (pan)
    mov y, volR           ; final square (reference attenuation): volR = (volR*volR)>>8
    mov a, volR
    mul ya                ; last op on this side, AFTER the pan multiply (SPEC.md)
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
