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
The host writes commands to the four ports; you edge-detect a *changed* command.
- **port0 (`$F4`) = `$01`–`$7F`** → **play** the song at `$2B00` from the start.
- **port0 = `$00` or `$80`–`$FF`** → **stop / idle** (do not restart). (A score
  with bit 7 set is a stop, not a song index.)
- **port3 (`$F7`) != 0** → **fade out** the current song, then stop.
Play (`port3 = 0`) and fade (`port0 = 0`) are mutually exclusive. Echoing the
command back is harmless but not required (the host does not gate on it). The main
loop must ALWAYS poll the command port every iteration — no path (fade, stop,
idle) may block it. After a fade completes, return to the exact power-on idle
state so a later play command cold-starts a fresh song.

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
frame: u16 track_ptr[8]         ; one per voice 0..7; 0 = channel unused
track: a byte stream of events (below), terminated by $00
```
Play a song = walk the song list; each frame plays its (up to) 8 tracks in
parallel across the 8 DSP voices. The 8 tracks of a frame are composed to be equal
length; the frame advances to the next song-list word when the tracks reach their
`$00` terminator. Loading a frame resets all 8 channels and starts them together.

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
| E1 | 1 | pan | opt |
| E2 | 2 | pan fade | no |
| E3 | 3 | vibrato | no |
| E4 | 0 | vibrato off | no |
| E5 | 1 | master volume | yes |
| E6 | 2 | master volume fade | no |
| E7 | 1 | tempo | yes |
| E8 | 2 | tempo fade | no |
| E9 | 1 | global transpose | yes |
| EA | 1 | channel volume | yes |
| EB | 3 | tremolo | no |
| EC | 0 | tremolo off | no |
| ED | 1 | channel pan | yes |
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

("no"/"opt" = consume operands, skip the effect — the Animaniacs songs need only
E0/E5/E7/EA/ED plus consuming F5/F7. Add others as needed.)

## Velocity byte (the `< $80` byte after a duration)
Split it: `quant_index = (byte >> 4) & 7`, `vel_index = byte & $0F`, then look up
the two ROM tables at `$4C10`:
```
QUANTTAB (8):  32 65 7F 98 B2 CB E5 FC          ; curquant = QUANTTAB[quant_index]
VELTAB   (16): 19 32 4C 65 72 7F 8C 98 A5 B2 BF CB D8 E5 F2 FC   ; curvel = VELTAB[vel_index]
```

## Note timing & gate
On a note (`$80`–`$C7`): compute pitch (below), set the voice volume from `curvel`
scaled by channel volume (`$EA`) and master volume (`$E5`), key the voice ON for
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
n        = note - REF_NOTE              ; REF_NOTE = $80
octave   = n / 12                       ; semitone = n % 12
factor   = ratiotab[semitone] >> (OCT_REF - octave)   ; OCT_REF = 6; octave is an
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
Title lead plays octave-low + quiet; fade cuts instead of ramping MVOL; clipping
with many voices. See `docs/hardware-state/sgb-audio.md`.
