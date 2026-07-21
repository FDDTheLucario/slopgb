# Clean-room SPC700 N-SPC engine — authoritative reference

An **original** SPC700 music sequencer for the SNES audio subsystem (WLA-DX
assembly), compatible with the SGB's N-SPC song/data format. This is the
implementation reference: format, protocol, and the exact playback math. It
describes a data format + documented S-DSP hardware — **no third-party code**.
Implement everything from this document; do not consult any existing sound
engine, ROM, or disassembly.

The corrections that got the spec to this state (and the bugs found during
bring-up) are collected in the last section, **"Fix history"** — kept separate so
this reference stays clean.

## Target & build
- CPU: SPC700. Assembler `wla-spc700`, linker `wlalink` (WLA-DX, both on `PATH`);
  `.spc700` syntax. `make` produces `driver.bin`.
- Output loads at ARAM **`$0400`** and is entered there (PC = `$0400`) with the
  SPC700 already IPL-booted. Keep the engine + its variables clear of the host
  data regions below.
- Keep the tunable constants at the top of `engine.asm` (see "Tunables").

## Memory map (the host loads these before entry at `$0400`)
| ARAM | Contents |
|---|---|
| `$0400` | engine code + variables (entry point) |
| `$2B00` | the **song** to play (see "Song data"), reloaded per song |
| `$4B00` | S-DSP **sample directory** (DIR page): 64 × 4 bytes `start_lo,start_hi,loop_lo,loop_hi`. Set DSP `DIR = $4B`. |
| `$4C10` | 8-byte **quantization** table + 16-byte **velocity** table (`$4C18`) — see "Velocity byte" |
| `$4C30` | **instrument table**, 6 bytes/entry (see "Instruments") |
| `$4DB0` | BRR waveform data pointed to by the directory |
All of `$2B00`/`$4B00`/`$4C10`/`$4C30`/`$4DB0` is provided data — read only.

## Host comm-port protocol (SPC I/O ports `$F4`–`$F7`)
The four ports carry an SGB SOUND command. **Only `port0` (`$F4`) — the Music
Score Code — controls music.** Ports 1–3 are the SGB sound-effect bytes (effect A
id, effect B id, effect attributes) and MUST NOT affect music: a sound effect
fires with score `$00` and arbitrary nonzero `port1`–`port3` while a song is
playing, and the song must keep playing untouched.

Edge-detect a change on **`port0` only** (ports 1–3 change on every sound effect
and are not music commands). On a changed `port0`:
- **`$00`** → **no music change** (SFX-only command; leave the current song
  playing).
- **`$01`–`$7F`** → **play** the song at `$2B00` from the start — but only once
  the song data is actually present (see "Play may precede the transfer" below).
- **`$80`–`$FF`** → **fade out** (bit 7 set = stop, not a song index; do not
  restart). This is NOT an instant cut: set the SGB-master target to `0`; **the music
  KEEPS PLAYING NORMALLY (the sequencer runs) while the master slews down** — measured
  on the reference, voices keep changing (new notes key on) as DSP MVOL falls `$60`→…
  →`0`, so do NOT freeze the sequencer. When the master reaches `0`, go idle (voices
  off, power-on idle state so a later play cold-starts). **The fade must be quick
  enough to finish and stop the song BEFORE the host loads the next song over `$2B00`.**
  The host preloads the next song (a `SOU_TRN` overwriting `$2B00`) and then plays it,
  and on the reference the fading song has already stopped by then. If the fade is so
  slow that the sequencer is still reading `$2B00` when the reload lands, it reads the
  new song's bytes through the old song's stale track cursors → a stuck/held garbage
  note. So keep the fade-out reasonably fast (it stops the old song in well under the
  gap before the next song's play command).

Echoing the command back is harmless but not required. The main loop must ALWAYS
poll `port0` every iteration — no path (stop, idle) may block it.

**Play may precede the transfer.** The host can set a play score (`$01`–`$7F`) on
`port0` *before* the song data has finished copying into `$2B00` (the SGB streams
that data in separately, and the score can win the race). While the copy is still
in flight, `$2B00`/`$2B01` read `0`. Song data always lives at `$2Bxx`, so the song
pointer's HIGH byte (`$2B01`) is non-zero once the data is present, and `0` while it
is not. Therefore a play command must be treated as **not-yet-ready** when the song
pointer's high byte is `0`: do NOT begin playback, do NOT walk the (still-empty)
song list, and do NOT consume the command — leave the engine idle and re-detect the
same play score on the next poll, retrying until the high byte is non-zero. Only a
non-zero high byte commits the engine to playing. (Starting on a `0` pointer walks
into zero-page, hits a `$0000` end-word, and latches silence forever — the failure
this guards against.)

**Fade trigger = `port0` in `$80`–`$FF`, NOT `port3`.** (An earlier draft wrongly
read `port3 != 0` as fade — that byte is the SFX-attributes byte and is nonzero for
ordinary sound effects; never fade from `port3`.) The confirmed host fade-out
signal is a `port0` stop code (`$80`–`$FF`); it lowers the SGB-master target to `0`
(next section), it does not cut instantly.

## Song data (at `$2B00`)
All pointers are little-endian 16-bit ARAM addresses.
```
$2B00:  u16 song_ptr            -> the song list
song list: a stream of 16-bit words, decoded by HIGH byte:
   high byte != 0  -> FRAME POINTER (the full word): play that frame, advance
   high byte == 0  -> CONTROL word, keyed by low byte:
        low == 0   -> end of song (stop / idle)
        low != 0   -> LOOP: the NEXT word is the loop-target address; set the song
                      pointer to it and keep reading. (low byte is a repeat count;
                      loop-forever is the simplest correct behavior.)
frame: u16 track_ptr[8]         ; one per voice 0..7; 0 = channel unused (silent)
track: a byte stream of events (below), terminated by $00
```
Play a song = walk the song list; each frame (re)starts its tracks and they play
in parallel across the 8 DSP voices.

**Frame loading — a `0` track pointer means "leave the channel running", NOT
silence.** For each of the 8 channels: if the frame's track pointer is NON-zero,
(re)start that channel on the new track (reset it, play from the top). If the
pointer is `0`, DO NOT touch that channel — it keeps playing whatever track it was
already on. A melodic line longer than one frame is therefore carried on a channel
that the *next* frame leaves `0`, so it continues seamlessly across the frame
boundary (it is not restarted and not silenced).

**Frame advance — channel 0 is the conductor.** The frame advances to the next
song-list word when **channel 0 (voice 0) reaches its `$00` end-of-track**. A
`$00` on any OTHER channel just stops that channel (it rests until the next frame
reloads it); it does NOT advance the frame and does NOT loop. So a track never
repeats itself within a frame — tracks are composed so the non-conductor channels
are ≤ channel 0's length, and any longer line rides a channel that the next frame
leaves `0` (per above). On advance, load the next frame per the loading rule.

## Track event encoding
Per channel, read events until one occupies time (note / tie / rest) or ends.
Maintain per channel: current duration, velocity, quantization, a ticks-remaining
counter, and the read pointer.
- **`$00`** — end of track.
- **`$01`–`$7F`** — set **duration** (in engine ticks). MAY be followed by one
  **velocity byte** in `$00`–`$7F` (present iff the *next* byte is `< $80`).
- **`$80`–`$C7`** — **note**: start it, occupy the current duration.
- **`$C8`** — **tie**: extend the previous note by the current duration (no key-on).
- **`$C9`** — **rest**: silence for the current duration (key off).
- **`$E0`–`$FA`** — **command**, with a FIXED operand count (table below). Consume
  ALL its operands even if you don't act on the command, so the stream stays
  synced. Commands take **zero ticks** — act, then immediately keep reading until
  a note/tie/rest/end (do not consume a tick on a command).

Duration and velocity bytes are OPTIONAL: a note/tie/rest with no preceding
duration reuses the last duration; a note directly after a duration (next byte
`>= $80`) reuses the last velocity.

### Command operand-count table (`$E0`–`$FA`), and what to act on
| cmd | ops | meaning | act? |
|-----|-----|------|------|
| E0 | 1 | instrument | yes |
| E1 | 1 | pan | yes |
| E2 | 2 | pan fade | no |
| E3 | 3 | vibrato | no |
| E4 | 0 | vibrato off | no |
| E5 | 1 | master volume | yes |
| E6 | 2 | master volume fade | no |
| E7 | 1 | tempo | yes |
| E8 | 2 | tempo fade | no |
| E9 | 1 | global transpose | yes |
| EA | 1 | per-channel transpose | yes |
| EB | 3 | tremolo | no |
| EC | 0 | tremolo off | no |
| ED | 1 | channel volume | yes |
| EE | 2 | channel volume fade | no |
| EF | 3 | call subroutine | no |
| F0 | 1 | vibrato fade | no |
| F1 | 3 | pitch envelope to | no |
| F2 | 3 | pitch envelope from | no |
| F3 | 0 | pitch envelope off | no |
| F4 | 1 | fine tune | no |
| F5 | 3 | echo enable/volumes | no |
| F6 | 0 | echo off | no |
| F7 | 3 | echo params | no |
| F8 | 3 | echo volume fade | no |
| F9 | 3 | pitch slide | no |
| FA | 1 | percussion base | no |

("no" = consume operands, skip the effect — the Animaniacs songs need
E0/E1/E5/E7/E9/EA/ED plus consuming F5/F7. Add others as needed.)

**Do not confuse the three that look alike** (getting this wrong plays a channel
at the wrong octave *and* mangles its volume): `$E1` = **pan**, `$EA` =
**per-channel transpose** (signed semitones, adds to the note like `$E9` but only
for its own channel), `$ED` = **channel volume**. The title theme keys its lead
via `$EA` (`+12`) and other channels via `$EA` (`-12`).

## Velocity byte (the `< $80` byte after a duration)
Split it: `quant_index = (byte >> 4) & 7`, `vel_index = byte & $0F`, then look up
the two ROM tables at `$4C10`:
```
QUANTTAB (8):  32 65 7F 98 B2 CB E5 FC          ; curquant = QUANTTAB[quant_index]
VELTAB   (16): 19 32 4C 65 72 7F 8C 98 A5 B2 BF CB D8 E5 F2 FC   ; curvel = VELTAB[vel_index]
```

## Note timing & gate
On a note (`$80`–`$C7`): compute pitch (below), set the voice volume (see "Master
volume & fade-in"), key the voice ON for `gate = (curdur * curquant) >> 8` ticks
(min 1), then key OFF — but the channel still occupies the full `curdur` ticks
before reading the next event. (Gate = articulation; it must NOT change the total
`curdur` timing.) Tie holds for the full duration; rest keys off immediately.

## Master volume & fade-in (two independent stages — keep them separate)
1. **SGB hardware master = the DSP main volume (`$0C/$1C` MVOL L/R).** This is the
   SGB's own output level, driven by the DRIVER, never by song data. MVOL is
   SIGNED, so a large unsigned song byte written here goes negative (`$F8` = −8 ≈
   mute) — NEVER write a song value to it. Three behaviors, kept distinct:
   - **Boot fade-IN (one-time).** MVOL is `0` ONLY at boot; from there it slews up to
     `$60` on the base-tick (wall-clock) timer, advancing even while idle. A song
     that plays during this ramp catches it and audibly fades in (measured: ~`$03` at
     half a second, `$60` at steady state); a song that starts later — after the ramp
     already reached `$60` during the silent boot/logo screens — starts at full. This
     is the ONLY slow fade-in; it is a boot-relative ramp, not a per-song thing.
   - **Play snaps to full.** Once the master has reached `$60`, a play command
     (`$01`–`$7F`) sets MVOL to `$60` **immediately (snap, no ramp)**. So after a
     fade-out drove the master to `0` and the song stopped, the NEXT song starts at
     full `$60` with NO fade-in (measured: title after the intro fade-out starts at
     `$60`). Do not re-run the slow fade-in per song.
   - **Fade-OUT** (`port0` `$80`–`$FF`): slew MVOL DOWN to `0` (base-tick paced) while
     **the music keeps playing normally** (sequencer runs; measured `$60`→`$38`→…→`0`
     with voices still changing). At `0`, key voices off and go idle. Keep it quick
     enough to stop the song before the host reloads `$2B00` (see the `$80` protocol
     note). Fade-out rate is a by-ear tunable.
2. **Song master volume = `$E5 vv`.** `$E5` sets a SOFTWARE scalar (`vv`/256, so
   `$F8` ≈ unity/full) applied to every voice's computed volume in software. It
   MUST NOT be written to the DSP main-volume register. Default = full.

**Per-voice volume** — computed independently for the left and right DSP volume
(`VxVOLL`/`VxVOLR`). This is the EXACT reference chain (disassembled from the ROM
engine's volume routine and numerically re-verified — it reproduces every voice's
`VOLL`/`VOLR` byte-for-byte). For EACH side (L then R, using that side's pan gain):
```
t = pan_gain            ; the side's pan gain (0..$FF); $FF at hard-center (no pan)
t = (t * song_master) >> 8    ; $E5 value ("songvol"), applied ONCE
t = (t * VELTAB[vel])  >> 8    ; note velocity
t = (t * channel_volume) >> 8  ; $ED value (default $FF)
t = (t * t) >> 8               ; <-- FINAL SQUARE: square the accumulated value
VxVOL(side) = t
```
Two things this fixes vs. the earlier (wrong) model:
1. **The final `(t*t)>>8` square is the real attenuation.** It is NOT "apply songvol
   twice." Because `song_master`, velocity, channel volume and pan are all inside the
   square, the OUTPUT scales with each of them squared — which is why a black-box
   measurement looked like `songvol²`, but that only held when velocity/channel-vol
   were equal across the compared notes. Square the whole per-voice value once, at
   the end, after every factor and the pan.
2. **`CHVOL_DEFAULT = $FF`** (not `$40`): the reference initializes every channel's
   volume to `$FF` at song start (a `$ED` command overrides it per channel).

`VELTAB`/`QUANTTAB` are already correct (verified identical to the reference tables
at `$4C18`/`$4C10`). Worked example (reference, center pan, `songvol $9C`, `chvol
$FF`, `vel $BF`): `$FF·$9C>>8=$9B`, `·$BF>>8=$73`, `·$FF>>8=$72`, square `$72·$72>>8
=$32` — matches the ROM's `VOLR=$32`. Pan curve for OFF-center voices is a further
refinement (the reference folds pan through a per-voice pan byte); the square and
the single master are the correctness-critical parts.

## Instruments (table at `$4C30`, 6 bytes/entry, indexed by `$E0 nn`)
```
byte0 = SRCN (sample-directory index)   byte1 = ADSR1   byte2 = ADSR2
byte3 = GAIN                            byte4,byte5 = 16-bit base pitch, BIG-ENDIAN
```
On `$E0 nn`, read `$4C30 + nn*6`, set the voice's `VxSRCN/VxADSR1/VxADSR2/VxGAIN`
from bytes 0–3, and store the base pitch as **`base16 = (byte4 << 8) | byte5`**
(big-endian) for the pitch formula.

## Pitch
```
note'    = note + global_transpose($E9) + per_channel_transpose($EA)  ; signed, byte-wraps
n        = note' - REF_NOTE             ; REF_NOTE = $80
octave   = n / 12                       ; semitone = n % 12
factor   = ratiotab[semitone] >> (OCT_REF - octave)   ; OCT_REF = 5; octave is an
                                          ; exact bit shift (left-shift if octave > OCT_REF)
VxPITCH  = (base16 * factor) >> PITCH_OUT_SHIFT        ; 16x16 multiply; PITCH_OUT_SHIFT ~ 8
clamp VxPITCH to $3FFF
ratiotab (12 x u16) = round($085F * 2^(k/12)):
  085F 08DE 0965 09F4 0A8C 0B2C 0BD6 0C8B 0D4A 0E14 0EEA 0FCD
```
The per-instrument `base16` is the multiplicand (each sample has its own native
tuning), and octave is handled as a shift — NOT a single continuous exponent.

## Tempo
Run Timer0 at target 16 → tick base **500 Hz** (`TIMER_DIV`). Each base tick,
`acc += tempo`; each 256-crossing runs one sequencer tick. So the sequencer runs
at **`(500 * tempo) / 256`** ticks/sec, and a duration-D note lasts exactly D
ticks. `$E7` sets `tempo` (default `TEMPO_DEFAULT`).

## Tunables (top of `engine.asm`)
`REF_NOTE`, `OCT_REF`, `PITCH_OUT_SHIFT` (pitch); `TIMER_DIV`, `TEMPO_DEFAULT`
(tempo); `MVOL_DEFAULT`, `CHVOL_DEFAULT` (volume).

## S-DSP register interface (public hardware, nocash *fullsnes*)
DSP regs via `$F2` (address) / `$F3` (data). Per voice X (base `X<<4`): `x0/x1`
VOL L/R, `x2/x3` PITCH, `x4` SRCN, `x5/x6` ADSR1/2, `x7` GAIN, `x8/x9` ENVX/OUTX.
Global: `$0C/$1C` MVOL L/R, `$4C` KON, `$5C` KOF, `$6C` FLG, `$5D` DIR (= `$4B`),
echo `$0D/$2C/$3C/$2D/$3D/$4D/$6D/$7D/$xF`. Play a note: set SRCN/PITCH/VOL/ADSR|
GAIN, then set the KON bit. Stop: set KOF (master untouched). Init: FLG echo off,
MVOL `0` — the ONLY time MVOL is 0; it then slews up to `$60` and stays (see
"Master volume & fade-in").

---

# Fix history

The corrections that shaped this reference (originally SPEC2–SPEC6) and the bugs
found live. Kept here so the record survives without cluttering the reference.

## Spec corrections (earlier drafts were wrong)
- **SBN / APU block header is `[len, dest]`**, not `[dest, len]` — song data never
  landed until this was fixed (upload path).
- **Pitch is per-instrument, not a fixed global base.** A note maps to
  `note_ratio * instrument_base16`; octave is a bit shift against an octave-6
  reference; the ratio table is scaled to `$085F`, not `$1000`.
- **The instrument base is BIG-ENDIAN** (`byte4<<8 | byte5`). Little-endian gave
  nonsense bases (`$0004`, `$F01D`) — this was the "pitch all out of whack".
- **Tempo base is 500 Hz** (Timer0 target 16); rate `(500*tempo)/256`.
- **`$4C10` is the quantization+velocity tables, `$4C30` the instrument table** —
  earlier mislabeled as pitch tables.
- **VCMD operands**: `$F7`/`$F5` take **3** (not 2); the full `$E0`–`$FA` table
  above must be honored or the track parse desyncs.
- **Fade is signalled on port3 (`$F7`)**, and a `$80`+ score is a **stop**.
- **Song list has loop/end control words** (high byte 0), not a flat list.
- **Commands take zero ticks**; **duration/velocity bytes are optional**.

## Engine bugs found live (SPC700 flag-clobber class)
- **`MOV X, savex` before `BEQ`** in the command dispatch clobbered the Z flag
  from the operand count, so voice 0 always took the "zero operands" branch → ch0
  consumed none of its command operands → tempo read `$00` → sequencer frozen.
  Fix: re-test with `CMP A, #0` after the restore.
- **`INCW wptr` before `BNE`** in the per-event read set Z from the pointer (never
  zero), so the `$00` end-of-track case was unreachable → tracks never terminated
  → frames never advanced (pattern repeated). Fix: `CMP A, #0` after the `INCW`.
- Lesson: on the SPC700, any load / `INC(W)` / `DEC` between a computed flag and
  its dependent branch clobbers the flag — re-establish it before branching.

## Known-remaining (as of this writing)
- **Echo/reverb not implemented.** The `$F5`/`$F7` echo commands are parsed and
  their operands consumed, but the DSP echo path (EON/EVOL/EFB/EDL/FIR) is left
  off. The reference enables echo (EON=`$07`, EVOL=`$C0`) and the wet tail adds
  audible body — a follow-up.

See `docs/hardware-state/sgb-audio.md`.

## Fixed since the last draft
- **VCMD mis-map**: `$EA` is **per-channel transpose** (not channel volume) and
  `$ED` is **channel volume** (not pan); `$E1` is pan. The old mapping dropped the
  title lead's `$EA +12` (octave-low) and wrote the transpose into channel volume
  (too quiet). Operand counts were already right, so only the handlers moved.
- **Octave calibration**: `OCT_REF` 6 → 5 (playback measured exactly one octave
  low across all voices once the transpose was applied).
- **Song-list loop** (`$00nn` control word): a `movw ya, wptr` left `Y` = high
  byte of the pointer, so the loop-target `rdword` read at `[wptr]+Y` and derailed
  the song cursor to garbage → freeze. Re-establish `Y=0` before that read.
- **SFX vs. fade**: music is driven by the score code (port0) ONLY; the SFX
  attributes byte (port3) is not a fade signal, and a `$00` score = no music
  change. An ordinary sound effect no longer stops the song.
- **Frame model = conductor**: a track's `$00` stops only that channel; the frame
  advances when **channel 0** ends; a null (`0`) frame track pointer leaves that
  channel running (a long line spans frames). Tracks never loop within a frame.
- **Play/transfer race**: the play score can win the race against the song-data
  copy, so `start_song` fired on an all-zero `$2B00` pointer, walked zero-page into
  a `$0000` end-word, and latched silence forever (edge-triggered `poll_comm` never
  retried). Fix: reject a `0` song-pointer high byte as not-yet-ready and un-latch
  the score so the play command retries until the data lands.
- **Master volume written to the wrong (signed) register**: `$E5 $F8` was written
  straight to DSP MVOL, which is signed, so `$F8` = −8 ≈ mute — the whole song
  came out barely audible. Fix: `$E5` is a software per-voice scalar; the DSP main
  volume is the SGB hardware master, faded `0`→`$60` on play. Channel-volume
  default was also far too low (`$12` per voice vs. the reference `$2E`).
