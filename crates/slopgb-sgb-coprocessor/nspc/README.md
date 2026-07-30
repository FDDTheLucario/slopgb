# Clean-room SPC700 music sequencer engine

Original SPC700 (SNES S-SMP) music driver written **from
[`spec/SPEC.md`](spec/SPEC.md) alone** (cited below as SPEC.md) — no existing
sound engine, driver, ROM, SPC file, or disassembly was consulted. Only the
documented song data format and the public S-DSP hardware registers were used.

## Where it runs

`driver.bin` is SPC700 machine code; slopgb's *core* emulates no SNES chip. The
built binary is embedded as `NSPC_ENGINE` (`../src/lib.rs`) and uploaded to APU
RAM `$0400` inside the **`spc700.wasm` plugin** (crate `slopgb-spc700-plugin`)
that the SGB coprocessor loads — the plugin executes it, `SgbCoprocessor` only
moves bytes.

`SgbCoprocessor::install_nspc` (`../src/samples.rs`) picks the engine, and it is
reached only with `--sgb-bios` and/or the `sf2` plugin flag:

- `--sgb-bios` alone → the SGB system ROM's **own** resident engine
  (`Engine::Rom`, the authentic playback); this clean-room engine sits unused.
- `SLOPGB_NSPC_CLEANROOM` set, or `--sf2` with no `--sgb-bios` (no ROM engine
  code to fall back on) → `Engine::CleanRoom`, this engine, uploaded over
  `$0400`.

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
  ~1.6 KB (last non-zero byte at `$0A3B`). Everything stays clear of the host data
  at `$2B00`, `$4B00`, `$4DB0`. Engine variables live in direct page `$10`–`$D3`
  (RAM the host does not touch; never emitted into the binary).

## What is implemented

- **DSP init + comm poll + a note.** Full S-DSP init (FLG, DIR=`$4B`, master
  volume, echo fully disabled, all voices silenced/keyed-off), Timer0 heartbeat,
  and the port0 command poll. On a play command a real note keys on.
- **Track playback.** Full track parser: durations, notes (`$80`–`$C7`), ties
  (`$C8`), rests (`$C9`), end-of-track (`$00`).
- **All 8 tracks in parallel across frames** on 8 DSP voices; song-list / frame
  walking (including the loop control word); tempo (`$E7`); instruments (`$E0` →
  full instrument-table entry: SRCN/ADSR1/ADSR2/GAIN + per-instrument base pitch).
- **Velocity / quantization byte** parsing, per-note velocity → voice volume, the
  `$E5` song-master scalar, `$E1` (pan), `$E9` (global transpose), `$EA`
  (per-channel transpose) and `$ED` (channel volume).
- **Note gate** from the quantization index (early key-off inside the note's
  duration, `tgate`).
- **Master fade in/out** (`ramp_master`): the one-time boot ramp to `SGB_MASTER`,
  and the fade to 0 on a `$80`–`$FF` stop code, which then keys off and idles.

**Deferred / stubbed**
- **Song-list loop repeat count** — a `$00nn` control word loops forever; the low
  byte's repeat count is not honored (see the `ponytail:` note in `load_frame`).
- **Echo/reverb (`$F5`/`$F7`)** — their 3 operand bytes each are consumed and
  ignored; S-DSP echo is left fully off (EON/EVOL/EDL = 0) so it can never stomp
  ARAM.
- **`$FA`** (percussion base, 1 operand) is consumed but not acted on.

## Comm-port poll (`$F4`)

**Only port0 (`$F4`), the SGB Music Score Code, controls music** — ports 1–3 are
the SGB sound-effect bytes and change on every effect, so `poll_comm`
edge-detects on port0 alone and acts on a change (SPEC.md):
- **`$00`** → no music change (an SFX-only command; the song keeps playing).
- **`$01`–`$7F`** → play the song at `$2B00` from the start.
- **`$80`–`$FF`** → stop: drop the master target to 0 and let `ramp_master` slew
  it down. The sequencer keeps running the whole way; when the master reaches 0,
  `stop_all` keys the voices off and idles.

`lastp0` inits to 0 so a command already latched at boot reads as changed. A play
score whose song data has not landed yet (`$2B01` still reads 0) is *not*
consumed — `start_song` un-latches `lastp0` so the same score retries on the next
pass. The engine echoes port0 back, but the host does not depend on it (SPEC.md).

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
note'    = note_byte + transpose($E9) + ttrans($EA)   ; signed, byte-wraps
n        = note' - REF_NOTE               ; REF_NOTE = $80
octave   = n / 12,  semitone = n % 12
factor   = ratiotab[semitone] >> (OCT_REF - octave)   ; shifts left if octave>OCT_REF
VxPITCH  = (instrument_base16 * factor) >> PITCH_OUT_SHIFT   ; 16x16 mul (four MUL YA)
clamp VxPITCH to $3FFF
```

**`instrument_base16` is BIG-ENDIAN**: `(entry_b4 << 8) | entry_b5` — b4 high, b5
low. Reading it little-endian gives nonsense bases (near-zero / near-max);
big-endian gives sane tight-range multipliers (`$0400`, `$1DF0`). Fed in raw —
never clamped/sanitized. `ratiotab[k] = round($085F * 2^(k/12))`, k=0..11 (16-bit
words). `REF_NOTE`, `OCT_REF` (default 5), and `PITCH_OUT_SHIFT` (default 8, dial
±a few) are exposed to align octaves by ear. `DEFAULT_BASE` is used only before a
track's first `$E0`. Tiny-base / bottom-octave notes lose a couple % to the integer
shift (lower `PITCH_OUT_SHIFT` for more bits). The host's `$4C10`
quantization/velocity tables are not read — the engine bakes its own `quanttab` /
`veltab` and its own `2^(n/12)` `ratiotab`.

## Volume / velocity / pan

- The optional velocity byte after a duration (`<$80`) splits into
  `curquant = QUANTTAB[(byte>>4)&7]` and `curvel = VELTAB[byte&$0F]` (SPEC.md
  tables). `calc_vol` then accumulates, all via `MUL YA`:
  `vscaled = (((curvel * channel_volume) >> 8) * songvol) >> 8`.
- Pan (`p_pan`, 0 = hard left … `$40` = center … `$7F` = hard right):
  `left_gain = min($FC, (127-pan)*4)`, `right_gain = min($FC, pan*4)`. Each side is
  then squared, which is where the real attenuation comes from (SPEC.md
  "Per-voice volume"): `t = vscaled*gain>>8`, `VxVOL(side) = t*t>>8`. Center pans
  give near-full level on both sides; hard pans zero the opposite side.
- `$E5 vv` is a **software** scalar (`songvol`, folded in above), never written to
  the DSP main volume — `MVOL` is signed, so a song byte like `$F8` there reads as
  −8 (≈ mute).
- `MVOL` is driver-owned: it starts at 0 once at boot and slews to `SGB_MASTER`
  (`$60`), the SGB hardware master level. `CHVOL_DEFAULT=$FF` — the final square
  supplies the headroom, so the per-channel default is full-scale. `SGB_MASTER`,
  `CHVOL_DEFAULT`, `FADE_IN_RATE` and `FADE_OUT_RATE` are all exposed at the top of
  `engine.asm`.

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
(`$F5` and `$F7` take 3 operands each, not 2 — with the wrong count the master
track truncates at a false `$00` and the sequence scrambles.)

| cmd | ops | acted? |
|---|---|---|
| `$E0` | 1 | yes — instrument: load entry `$4C30+nn*6` → SRCN/ADSR1/ADSR2/GAIN + base |
| `$E1` | 1 | yes — pan (`tpan`) |
| `$E5` | 1 | yes — song-master scalar (`songvol`, software) |
| `$E7` | 1 | yes — tempo |
| `$E9` | 1 | yes — global transpose (signed semitones, added to every note) |
| `$EA` | 1 | yes — per-channel transpose (`ttrans`, signed semitones) |
| `$ED` | 1 | yes — channel volume (`tchvol`) |
| `$E2`–`$E4`,`$E6`,`$E8`,`$EB`,`$EC`,`$EE`–`$FA` | per `cmdlen` (0–3) | no — operands consumed only |

`cmdlen` = `1 1 2 3 0 1 2 1 2 1 1 3 0 1 2 3 1 3 3 0 1 3 0 3 3 3 1` for `$E0`…`$FA`.

## Key assumptions

1. **Song-list model (SPEC.md):** `$2B00` holds a `u16` pointer to a song list of
   `u16` words, decoded by **high byte**: high ≠ 0 = frame pointer (play it);
   high == 0 = control — low == 0 ends the song, low ≠ 0 is a loop whose *next*
   word is the target address the song pointer jumps to (loop forever; the low
   byte's repeat count is not yet honored — see the `ponytail:` note in
   `load_frame`). Each frame is 8 `u16` track pointers; each track is the event
   byte stream. Tracks play in parallel. A `$0000` track pointer means **leave that
   channel running** on its current track (a line longer than one frame rides
   across the boundary), not "unused". **Channel 0 is the conductor:** the frame
   advances when **voice 0** reaches its `$00` end-of-track; a `$00` on any other
   channel only stops that channel until the next frame reloads it.
2. **Frame = voice mapping:** track slot *i* always plays on DSP voice *i*.
3. **Per-track defaults reset on the channels a frame (re)starts** — i.e. only
   those whose new track pointer is non-zero (duration=1, velocity=full,
   quant=full, SRCN=0, channel-vol=`CHVOL_DEFAULT`, pan=center, default ADSR,
   `DEFAULT_BASE` pitch). Tempo, `songvol` and the master are global and persist
   across frames. Assumption: each track re-declares its instrument/params at frame
   start, typical for patterns.
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
8. **Fade → stop → ready is a recoverable cycle.** The main loop *always* polls
   port0 (and still echoes, though the host no longer needs it), and `ramp_master`
   runs every base tick whether idle or playing. A stop code only lowers
   `mastertarget` to 0; when the slew lands there, `ramp_master` calls `stop_all`,
   which clears all sequencer/track state and keys off all voices but **leaves
   `MVOL` alone** — the master is persistent, so the next song starts at full with
   no re-fade. Every play command routes to `start_song`, whose first act is
   `stop_all` — a full cold re-init identical to the first power-on play, reloading
   the song pointer from `$2B00`. So "play A → fade A → play B" plays B every time,
   no reset. `FADE_OUT_RATE=4` / `FADE_IN_RATE=8` base ticks per volume step
   (tunable).

## File map

- `engine.asm` — the whole engine (single WLA-DX `.spc700` source).
- `linkfile` — wlalink object list.
- `Makefile` — assemble + link to `driver.bin`.
- `driver.bin` — the committed build, embedded as `NSPC_ENGINE` by `../src/lib.rs`.
- `spec/SPEC.md` — the format/protocol/math reference this engine was written from.
