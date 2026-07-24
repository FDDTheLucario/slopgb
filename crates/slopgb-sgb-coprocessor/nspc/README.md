# Clean-room SPC700 music sequencer engine

Original SPC700 (SNES S-SMP) music driver written **from `SPEC.md` alone** — no
existing sound engine, driver, ROM, SPC file, or disassembly was consulted. Only
the documented song data format and the public S-DSP hardware registers were used.

## Build

```sh
make            # -> driver.bin
```

- Assembler: `wla-spc700`, linker: `wlalink` (WLA-DX; both must be on `PATH`).
- Output `driver.bin` is a raw binary whose **first byte is the entry point**, meant to
  be loaded at ARAM `$0400` and entered at PC `$0400` (IPL already booted).
- The bank is mapped `SLOT 0 $0400`, `.ORG 0`, so every label resolves to a
  `$0400`-based address and byte 0 of the file is address `$0400`.
- The file is padded to the 4 KB bank (`$0400`–`$13FF`); actual code+tables use
  ~1.1 KB (`$0400`–`$0863`). Everything stays clear of the host data at `$2B00`,
  `$4B00`, `$4DB0`. Engine variables live in direct page `$10`–`$77` (RAM the host
  does not touch; never emitted into the binary).

## What is implemented (milestones 1–3 complete, most of 4–5 too)

- **1. DSP init + comm handshake + a note.** Full S-DSP init (FLG, DIR=`$4B`, master
  volume, echo fully disabled, all voices silenced/keyed-off), Timer0 heartbeat,
  and the comm-port handshake. On a play command a real note keys on.
- **2. Single-track playback.** Full track parser: durations, notes, ties, rests.
- **3. All 8 tracks in parallel across frames** on 8 DSP voices; song-list / frame
  walking; tempo (`$E7`); instruments (`$E0` → full instrument-table entry:
  SRCN/ADSR1/ADSR2/GAIN + per-instrument base pitch).
- **4. (done)** velocity/quantization byte parsing, per-note velocity → voice
  volume, master volume (`$E5`), `$EA` (channel volume) and `$ED` (pan).
- **5. (partial)** fade-out command (ramp master volume to 0, then stop). Song
  looping is **not** implemented: at the `$0000` song-list terminator the engine
  stops and idles (spec's "start simple" option).

**Deferred / stubbed**
- **Quantization** is *parsed and stored* (`tquant`) but not yet applied — notes
  currently sound for their full duration, keyed off only when the next event
  (rest/new note) arrives or the track ends. Hook point: `pt_note` in `parse_track`
  could schedule an early key-off at `duration * quant/8`.
- **Echo/reverb (`$F7`)** — the 2 operand bytes are consumed and ignored; S-DSP
  echo is left fully off (EON/EVOL/EDL = 0) so it can never stomp ARAM.

## Comm-port handshake (`$F4`–`$F7`)

Each `poll_comm` pass edge-detects on **port0 (`$F4`)** and **port3 (`$F7`)** and
acts on a change (SPEC.md):
- **port3 (`$F7`) ≠ 0** while playing → fade-out the current song.
- else **port0 (`$F4`) ≠ 0** → play the song at `$2B00` from the start.
- else (both 0) → stop / idle.

A play arrives as `[songN,0,0,0]` (port0 set, port3 = 0); a fade as `[0,0,0,$CC]`
(port3 set, port0 = 0), so they never collide. `last_p0`/`last_p3` init to 0 so a
command already latched at boot reads as changed. The engine still echoes the two
bytes back, but the host no longer depends on it (SPEC.md).

## Timing / tempo mapping

- **Base tick:** Timer0, target `TIMER_DIV=16` → 8000/16 = **500 Hz** (SPEC.md).
- **Engine tick:** an 8-bit accumulator adds `tempo` every base tick; each carry
  (crossing 256) is one engine tick. So

  ```
  engine_ticks_per_second = 500 * tempo / 256
  ```

  Default `tempo = $28` (40) → ~78 engine ticks/s. `$E7 tt` overrides `tempo`
  live. Raise `TIMER_DIV` to slow the music, lower it to speed up (exposed at the
  top of `engine.asm`).
- **Per-tick loop (SPEC.md):** each engine tick decrements every active channel's
  `ticksleft` (`tdurrem`) and its gate countdown (`tgate`). A duration-D note
  occupies exactly D ticks. A channel reads its next event only when `ticksleft`
  hits 0; **commands take zero ticks** — the parser consumes the command's
  operands and keeps reading until it reaches a note/tie/rest/end, so a header of
  several commands (e.g. ch0's `E7 F7 F5 E5 E0`) never delays the first note.
- **Note gate (articulation):** a note keys on for `gate = (curdur*curquant)>>8`
  ticks (min 1), then keys off, while still occupying the full `curdur` before the
  next event. Ties hold for the whole duration; rests key off immediately.

## Pitch mapping (SPEC.md)

Octave is an exact bit shift; only the 12 semitones use a ratio table; the
per-instrument 16-bit base is a tuning **multiplier**:

```
n        = note_byte - REF_NOTE          ; REF_NOTE = $80
octave   = n / 12,  semitone = n % 12
factor   = ratiotab[semitone] >> (OCT_REF - octave)   ; shifts left if octave>OCT_REF
VxPITCH  = (instrument_base16 * factor) >> PITCH_OUT_SHIFT   ; 16x16 mul (four MUL YA)
clamp VxPITCH to $3FFF
```

**`instrument_base16` is BIG-ENDIAN**: `(entry_b4 << 8) | entry_b5` — b4 high, b5
low. This was the "all out of whack" bug: reading it little-endian gave nonsense
bases (near-zero / near-max); big-endian gives sane tight-range multipliers
(`$0400`, `$1DF0`). Fed in raw — never clamped/sanitized. `ratiotab[k] =
round($085F * 2^(k/12))`, k=0..11 (16-bit words). `REF_NOTE`, `OCT_REF` (default 6),
and `PITCH_OUT_SHIFT` (default 8, dial ±a few) are exposed to align octaves by ear.
`DEFAULT_BASE` is used only before a track's first `$E0`. Tiny-base / bottom-octave
notes lose a couple % to the integer shift (lower `PITCH_OUT_SHIFT` for more bits).
The `$4C10` semitone table is not used — `ratiotab` is our own `2^(n/12)` math.

## Volume / velocity / pan

- The optional velocity byte after a duration (`<$80`) splits into
  `curquant = QUANTTAB[(byte>>4)&7]` and `curvel = VELTAB[byte&$0F]` (SPEC.md ROM
  tables). `vscaled = (curvel * channel_volume) >> 8` via `MUL YA`.
- Pan (`p_pan`, 0 = hard left … `$40` = center … `$7F` = hard right):
  `left_gain = min($FC, (127-pan)*4)`, `right_gain = min($FC, pan*4)`;
  `VxVOLL = vscaled*left_gain>>8`, `VxVOLR = vscaled*right_gain>>8`. Center pans give
  near-full level on both sides; hard pans zero the opposite side.
- `$E5 vv` writes `vv` straight to `MVOLL`/`MVOLR`.
- Defaults lowered again to sit under game SFX: `MVOL_DEFAULT=$28`,
  `CHVOL_DEFAULT=$18` (both exposed). With 8 voices summing, these keep headroom.

## Instrument table (`$4C30`, 6 bytes/entry)

`$E0 nn` reads entry `nn` at `$4C30 + nn*6`: `b0=SRCN`, `b1=ADSR1`, `b2=ADSR2`,
`b3=GAIN`, `b4:b5=base pitch` (**big-endian**: b4 high, b5 low). On the next note
the voice is set from these
(`VxSRCN/VxADSR1/VxADSR2/VxGAIN`), and the base pitch drives `calc_pitch`. ADSR1
has bit7 set in the data so the envelope comes from ADSR (GAIN is the fallback).
Applying the real per-instrument ADSR is what makes instruments sound correct.

## VCMD coverage

Events: `$00` end of track; `$01`–`$7F` set duration (+ optional velocity/quant
byte iff the next byte is `<$80`); `$80`–`$C7` note; `$C8` tie; `$C9` rest.

Commands `$E0`–`$FA` each have a **fixed operand-byte count** (SPEC.md, in the
`cmdlen` table). The parser always consumes exactly that many operand bytes to
stay in sync, and *acts* on the ones below; the rest are consumed-and-ignored.
(Getting `$F7`=3 and `$F5`=3 right is what stopped the master track from
truncating at a false `$00` and scrambling the sequence.)

| cmd | ops | acted? |
|---|---|---|
| `$E0` | 1 | yes — instrument: load entry `$4C30+nn*6` → SRCN/ADSR1/ADSR2/GAIN + base |
| `$E5` | 1 | yes — master volume → `MVOLL/MVOLR` |
| `$E7` | 1 | yes — tempo |
| `$E9` | 1 | yes — global transpose (signed semitones, added to every note) |
| `$EA` | 1 | yes — channel volume (`tchvol`) |
| `$ED` | 1 | yes — pan (`tpan`) |
| `$E1`–`$E8`,`$EB`,`$EC`,`$EE`–`$FA` | per `cmdlen` (0–3) | no — operands consumed only |

`cmdlen` = `1 1 2 3 0 1 2 1 2 1 1 3 0 1 2 3 1 3 3 0 1 3 0 3 3 3 1` for `$E0`…`$FA`.
`$FA` (percussion base, 1 op) is consumed but not yet acted on.

## Key assumptions

1. **Song-list model (SPEC.md):** `$2B00` holds a `u16` pointer to a song list of
   `u16` words, decoded by **high byte**: high ≠ 0 = frame pointer (play it);
   high == 0 = control — low == 0 ends the song, low ≠ 0 is a loop whose *next*
   word is the target address the song pointer jumps to (loop forever; the low
   byte's repeat count is not yet honored — see the `ponytail:` note in
   `load_frame`). Each frame is 8 `u16` track pointers (`$0000` = unused channel);
   each track is the event byte stream. Tracks play in parallel; the frame advances
   only when **all** its tracks have ended.
2. **Frame = voice mapping:** track slot *i* always plays on DSP voice *i*.
3. **Per-track defaults reset each frame** (duration=1, velocity=full, quant=full,
   SRCN=0, channel-vol=`$20`, pan=center, default ADSR, `DEFAULT_BASE` pitch).
   Tempo and master volume are global and persist across frames. Assumption: each
   track re-declares its instrument/params at frame start, typical for patterns.
4. **Velocity byte:** present iff the byte after a duration is `<$80`; a `$00` in
   that position is taken as velocity 0 (per the spec's `$00`–`$7F` range), not a
   track terminator — a terminator is not expected immediately after a duration.
5. **Note timing:** a note occupies exactly `duration` engine ticks; retriggered by
   `KON` on each new note event. `KON` is cleared at the start of each engine tick
   and set (batched) at the end, giving one clean key-on edge per note (the
   set-and-hold-until-next-tick pattern).
6. **ADSR/GAIN come from the instrument table** (`$E0`); `ADSR1_DEF/ADSR2_DEF/
   GAIN_DEF` are only fallbacks used before the first `$E0` on a track.
7. **Echo disabled** (`EDL=0`, `EON=0`, echo-write bit set in `FLG`) so the driver
   never writes an echo buffer into host RAM.
8. **Fade → stop → ready is a bulletproof recoverable cycle.** The main loop
   *always* polls the command port (and still echoes, though the host no longer
   needs it). Fade requires state==1; when it ends, `stop_all` clears all
   sequencer/track state, keys off all voices, **and restores MVOL to default**
   (never left at 0 outside an active fade). Any play command while not mid-song
   routes to `start_song`, whose first act is `stop_all` — a full cold re-init
   identical to the first power-on play, reloading song pointers from `$2B00`. So
   "play A → fade A → play B" plays B every time, no reset. `FADE_RATE=2` engine
   ticks per volume step (tunable).

## File map

- `engine.asm` — the whole engine (single WLA-DX `.spc700` source).
- `linkfile` — wlalink object list.
- `Makefile` — assemble + link to `driver.bin`.
