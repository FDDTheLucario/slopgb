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
- **`$01`–`$7F`** → **play** the song at `$2B00` from the start.
- **`$80`–`$FF`** → **stop / idle** (bit 7 set = stop, not a song index; do not
  restart). Return to the exact power-on idle state so a later play cold-starts.

Echoing the command back is harmless but not required. The main loop must ALWAYS
poll `port0` every iteration — no path (stop, idle) may block it.

**No fade:** there is no confirmed host signal for a music fade-out (an earlier
draft wrongly read `port3 != 0` as fade — that byte is the SFX-attributes byte and
is nonzero for ordinary sound effects). Leave any `state 2` / `fade_step` code
present but unreachable until a real fade trigger is identified; never enter it
from `port3`.

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
On a note (`$80`–`$C7`): compute pitch (below), set the voice volume from `curvel`
scaled by channel volume (`$ED`) and master volume (`$E5`), key the voice ON for
`gate = (curdur * curquant) >> 8` ticks (min 1), then key OFF — but the channel
still occupies the full `curdur` ticks before reading the next event. (Gate =
articulation; it must NOT change the total `curdur` timing.) Tie holds for the
full duration; rest keys off immediately.

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
GAIN, then set the KON bit. Stop: set KOF. Init: FLG echo off, MVOL up.

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
- Clipping with many simultaneous voices (wants a mix / master-volume trim; the
  songs' own `$E5` master-volume values run hot vs. the reference).

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
