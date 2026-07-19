# SGB audio — SNES S-DSP + SPC700 wiring

The Super Game Boy's sound hardware is a Super Famicom audio subsystem: the
**SPC700** (S-SMP) CPU running a sound driver out of 64 KB APU RAM, feeding the
**S-DSP** 8-voice sample synthesizer. slopgb emulates both and mixes the result
into the Game Boy audio stream. Everything here is **`Model::Sgb`/`Sgb2`-scoped**
— on `Dmg`/`Cgb` the subsystem is never constructed, so output is byte-identical
(the golden-safe law).

Code: the SPC700 (`spc700/`) and the S-DSP (`dsp/`) live in the shared
`crates/slopgb-snes-apu` crate (so the same logic backs both this built-in path
and a wasm coprocessor plugin — no duplication); `crates/slopgb-core/src/sgb/apu.rs`
holds the `SgbApu` wiring that clocks them off the Game Boy stream, and `GameBoy`
integration is in `lib.rs` + `lib/sgb_api.rs`. The CPU detail is
[spc700.md](spc700.md). The SGB *presentation* side (border/palette/attributes)
is [sgb.md](sgb.md).

## The `AudioCoprocessor` swap seam

`GameBoy` holds the SGB audio side as `Option<Box<dyn sgb::AudioCoprocessor>>`
(`sgb/mod.rs`), not the concrete `SgbApu` — so the built-in SPC700 + S-DSP can
be swapped for an alternative implementation (e.g. one backed by a wasm
coprocessor plugin) without touching `GameBoy`. The trait is **`pub`** and
**bus-agnostic**: `clock` / `poll(&mut dyn SgbCommandSource)` / `mix_into` /
`set_output_rate` / `load_bios` / `write_state` / `read_state` / `clone_box`.
`poll` takes the small public `SgbCommandSource` trait (`take_sound_event` /
`take_data_snd` / `sou_trn_data` / `data_trn_data` / `flags`) instead of the
core-private `Interconnect`, so the trait can be implemented outside
`slopgb-core`; the bus (`impl SgbCommandSource for Interconnect`, crate-private)
is the live source `GameBoy::step` passes as a `&mut dyn`, so the bus type never
leaks. The built-in `SgbApu` is the default implementation; it bridges to its
own inherent methods, so the path is **byte-identical** whether reached directly
(unit tests hold a concrete `SgbApu`) or through the trait object.

`GameBoy::set_audio_coprocessor(Box<dyn AudioCoprocessor>)` (`lib/sgb_api.rs`)
is the public injection seam — a frontend/host installs a plugin-backed
coprocessor here. It only replaces on `Model::Sgb`/`Sgb2` (off SGB there is no
slot, so the box is dropped); like `debug_set_reg`/load-state it is an explicit
user-initiated mutation, never taken on the passive frame loop. The box exists
only on `Model::Sgb`/`Sgb2`, so `Dmg`/`Cgb` never touch the seam (golden-safe).
Verified byte-identical: `golden_fingerprint` + mooneye 93/93 + the SGB audio
unit tests + the two injection tests
(`injected_audio_coprocessor_is_driven_through_the_public_seam`,
`set_audio_coprocessor_is_a_noop_off_sgb`), all green after the decoupling.

## SNES-side coprocessor plugins — status + the full-integration path

The SNES-side chips exist as standalone wasm coprocessor plugins:

- **`slopgb-spc700-plugin`** wraps `slopgb-snes-apu` (SPC700 + S-DSP — the exact
  built-in code) as a tier-3 `Coprocessor`; clocking it in wasm runs the real
  IPL ROM (`$AA`/`$BB` handshake) and the S-DSP synthesizes. It buffers the
  synthesized stereo PCM and hands it back through the ABI's `drain_pcm` path
  (see item 1). Proven in `slopgb-plugin-host/tests/spc700_roundtrip.rs`.
- **`slopgb-w65c816-plugin`** wraps the clean-room 65C816 (the SNES CPU) with a
  guest SNES-RAM + comm-port bus. Proven in `w65c816_roundtrip.rs`.

The full chain runs as **two loaded wasm plugins** orchestrated by
**`slopgb-sgb-coprocessor`** (`SgbCoprocessor`): it loads `spc700.wasm` +
`w65c816.wasm` through `LoadedCoprocessor`, installs the clean-room firmware into
each chip's RAM (`write_ram`/`set_pc`), mediates the four `$2140-$2143` SNES↔APU
comm ports between the two loaded plugins each pumped chunk, routes the SGB sound
commands into the chips, and mixes the drained S-DSP PCM — exposed as a
`slopgb-core` `AudioCoprocessor` a frontend injects via `set_audio_coprocessor`.
It depends on **neither** `slopgb-snes-apu` nor `slopgb-w65c816` (the plugins are
built from those); `wasmi` stays quarantined in `slopgb-plugin-host`. Plugin
pumps are **batched** (a ~4096 GB-cycle flush chunk) so the per-frame wasm
crossing count stays low — never one crossing per emulated cycle. The integration
milestones, smallest-first:

1. **A PCM-drain path in the tier-3 ABI — DONE (ABI v3).** `Coprocessor` gained
   `drain_pcm` (default: none, for a non-audio chip like the 65C816); the
   generated `slopgb_drain_pcm` export ships the stereo samples over the emit
   channel (interleaved LE `i16` L,R pairs, kind `EMIT_KIND_PCM`) and
   `LoadedCoprocessor::drain_pcm` decodes them, so the host can mix a plugin's PCM
   like the built-in's `mix_into`. Proven in
   `spc700_roundtrip::spc700_pcm_drains_to_the_host`.
2. **Decouple `AudioCoprocessor` from `Interconnect` — DONE.** `poll` now takes
   the public `SgbCommandSource` trait instead of the core-private `Interconnect`,
   so the trait is `pub` and implementable outside core. Same ops, same order —
   the built-in `SgbApu` keeps its throttle + edge-detection and stays default;
   byte-identical (see the swap-seam section above).
3. **A public `GameBoy` injection API — DONE.** `set_audio_coprocessor` installs
   a plugin-backed `AudioCoprocessor` (core can't depend on `wasmi`; the adapter
   lives in the host and forwards to the wasm plugin, calling `drain_pcm` each
   frame from item 1). SGB-only, golden-safe (see the swap-seam section).
4. **Combine the 65C816 + SPC700 + DSP into one SNES coprocessor — DONE
   (`slopgb-sgb-coprocessor`).** The injected `SgbCoprocessor` drives both loaded
   plugins: DATA_SND ($0F) lands in SNES work RAM (`write_ram` on the CPU plugin),
   JUMP ($12) redirects the 65C816 (`set_pc`), and the host mediates the four
   `$2140-$2143` comm ports between the SNES CPU and the SPC700 plugins — the
   no-ops the built-in HLE path leaves. It installs an **original clean-room
   firmware** (owned as byte arrays here, poked into the chips' RAM) in place of
   the (unshipped, copyrighted) SGB system ROM: a 65C816 shim that forwards a
   SNES-RAM sound mailbox to the SPC700 ports, and a SPC700 driver that waits on a
   port and keys the S-DSP. So a bare SGB `SOUND ($08)` command produces audio
   with no game-supplied driver, and a `SOU_TRN` game driver still plays (the
   upload replaces the resident driver). Save-state / clone ride the tier-3
   `save_state`/`load_state` ABI (v5); clone re-instantiates fresh plugin
   instances and loads the snapshot. Proven end to end
   (`sound_command_drives_the_firmware_chain_to_audio`,
   `injected_coprocessor_makes_a_gameboy_sound_command_audible`) — a SOUND packet
   through a real `GameBoy` yields non-zero PCM. **Honest limits:** the SNES↔GB
   clock ratio is a loose HLE approximation (not cycle-exact), the SOUND→note
   mapping and the DATA_SND packet layout are original clean-room interpretations
   (the real SGB effect-code→driver semantics live in the unread system ROM), and
   the tone is a synthesized square, not the SGB sample bank.
5. **Runtime plugin loading — DONE (no toggle: it's a plugin).** The SGB
   coprocessor auto-loads from the plugins dir (`--plugins` / `SLOPGB_PLUGINS_DIR`,
   or the Options→Plugins browse): on an SGB machine, when the dir holds both
   `spc700.wasm` + `w65c816.wasm`, `Session::apply_sgb_coprocessor` loads them and
   injects the `SgbCoprocessor` via `set_audio_coprocessor` after every machine
   build; a missing plugin (or non-SGB machine) leaves the built-in HLE `SgbApu` in
   place (byte-identical golden path), silently — absence is the norm, not an
   error. slopgb itself neither builds nor bundles the wasm. The dir is held by
   `Session::set_plugins_dir` (`plugins_dir`), re-applied on power-cycle / model
   switch. There is no `--sgb-coprocessor` flag and no Sound-tab backend selector:
   drop the plugin in the dir and it runs. Proven in
   `session::tests::sgb_coprocessor_plugin_in_the_dir_swaps_the_audio_backend`
   (silent on the built-in default, silent on the missing-plugins fallback,
   audible with the coprocessor loaded).

## Sources

Every table and quirk is a verbatim port of a cited reference:

- **Blargg `snes_spc/SPC_DSP.cpp`** — the reference S-DSP used by higan/bsnes/
  snes9x. Source of the BRR decode + predictor forms, the 512-entry Gaussian
  table + interpolation, the ADSR/GAIN per-step arithmetic, and the
  rate-counter tables.
- **nocash fullsnes** ("SNES APU DSP …") — register map, BRR block layout, the
  shift ≥ 13 quirk, echo buffer addressing, and the SGB command packet formats.
- **bsnes** `dsp` — echo FIR + feedback path.

## Clocking (SPC700 ↔ DSP ↔ Game Boy)

The SPC700 runs at 1.024 MHz, the Game Boy at 4.194304 MHz, so **1 GB T-cycle =
125/512 SPC cycle** exactly. Each GB instruction, `SgbApu::clock` advances the
SPC700 by that many cycles (budget accumulated in `1/512`-cycle units to stay
exact). The S-DSP emits **one 32 kHz stereo sample every 32 SPC cycles**
(1.024 MHz ÷ 32). That 32 kHz stream is zero-order-held up to the Game Boy APU's
output rate (48 kHz by default) using the *same* accumulator law as the GB APU,
so the two streams emit an equal sample count per drain and mix sample-for-sample
in `GameBoy::drain_audio`. The DSP↔SPC seam is the `Dsp` trait
(`sgb/spc700/ports.rs`): `$F2`/`$F3` route to the S-DSP; synthesis (which needs
APU RAM) is driven by `SgbApu`, not from the trait's `tick`.

## S-DSP register map (`$00-$7F`)

Per voice `v` (base `v<<4`):

| off | reg | | off | reg |
|--|--|--|--|--|
| +0/+1 | `VOLL`/`VOLR` (signed) | | +5 | `ADSR1` (bit7 ADSR-enable, AR, DR) |
| +2/+3 | `PL`/`PH` (14-bit pitch) | | +6 | `ADSR2` (SR, SL) |
| +4 | `SRCN` (directory index) | | +7 | `GAIN` |
| +8 | `ENVX` (RO, `env>>4`) | | +9 | `OUTX` (RO, sample>>8) |
| +F | `FIR` (echo tap) | | | |

Globals: `MVOLL 0C` `MVOLR 1C` `EVOLL 2C` `EVOLR 3C` `KON 4C` `KOF 5C`
`FLG 6C` `ENDX 7C` `EFB 0D` `PMON 2D` `NON 3D` `EON 4D` `DIR 5D` `ESA 6D`
`EDL 7D`. Writes to `$80-$FF` (the read-only mirror) are ignored.

## Models

### BRR decode (`dsp/brr.rs`)

9-byte blocks: header `SSSS FF LE` (shift, filter, loop, end) + 16 signed 4-bit
nibbles. Ported verbatim from Blargg (half-scale internal arithmetic, stored at
full 16-bit): nibble → `(n<<shift)>>1` (shift ≥ 13 → sign only, the fullsnes
quirk) → one of four linear predictors → clamp-16 → `*2` wrap to 15-bit. The four
predictors are the documented coefficients (filter 1 ≈ 0.9375·p1; filter 2 ≈
1.906·p1 − 0.9375·p2; filter 3 ≈ 1.797·p1 − 0.8125·p2). Loop/end drive `ENDX`;
end-without-loop mutes the voice.

### Gaussian interpolation (`dsp/gaussian.rs`)

The 512-entry SNES Gaussian table (max 1305, byte-identical to the DSP mask ROM /
higan `gauss[512]`). The pitch counter's fraction (bits 4-11) is the 8-bit index
`i`; four taps `gauss[255-i] gauss[511-i] gauss[256+i] gauss[i]` weight the four
newest samples, `>> 11` each, with an intermediate 16-bit wrap after the first
three taps, a final clamp, and the low bit cleared (the output is always even).
The coefficients sum to ≈ 2048 → unity gain.

### Envelope: ADSR + GAIN (`dsp/envelope.rs`)

11-bit envelope. **ADSR** (`ADSR1` bit 7): attack `+0x20`/step (`+0x400` at the
max rate), exponential decay/sustain (`env -= 1 + (env>>8)`), release `-8`/step.
**GAIN**: direct (`env = (gain&0x7F)<<4`), linear ±`0x20`, exponential decrease,
and bent increase (`+0x20` below `0x600`, else `+0x8`). Every step is gated by a
global rate counter (period `0x7800`) via `(counter + OFFSET[rate]) % RATE[rate]
== 0`; rate 0 is frozen. `RATE`/`OFFSET` are Blargg's `counter_rates`/
`counter_offsets`.

### Echo (`dsp/echo.rs`)

Ring buffer in APU RAM at `ESA<<8`, length `(EDL&0xF)*2 KiB` (latched at the ring
start). Each sample: read one stereo slot, push through the 8-tap `FIR0-7` (÷128),
add scaled by `EVOL(L/R)` to the master mix, and — unless `FLG` bit 5 (ECEN)
disables writes — write `echo_bus + FIR*EFB/128` back.

### Noise / pitch-mod / key-on

`FLG` bits 0-4 clock a 15-bit noise LFSR; a voice with its `NON` bit reads noise
instead of BRR. `PMON` modulates a voice's pitch by the previous voice's output.
`KON` is edge-triggered (0→1) with a **1-sample latch delay**; `KOF` is
level-sensitive (release). `FLG` bit 6 mutes, bit 7 soft-resets.

## SGB command routing (`sgb/apu.rs`)

The Game Boy sends SGB commands; the seams are drained from the PPU each step:

- **SOU_TRN ($09)** — a 4096-byte self-describing block (`(dest, len, data…)`
  descriptors, per fullsnes). `SgbApu::upload_transfer` copies each descriptor
  into APU RAM and starts the SPC700 at the first load address (typically the
  Program Area `0x0400`). This is the path that produces **real audio with no
  BIOS** for a game that ships its own SPC700 driver + samples. An **original,
  clean-room SPC700 driver** proving this path is in `apu_tests.rs`
  (`original_sou_trn_driver_synthesizes_a_tone`): a hand-authored SPC700 program
  that writes the S-DSP registers over `$F2`/`$F3` and plays a synthesized
  square-wave tone (my own BRR sample), uploaded via `SOU_TRN` and executed on
  the emulated SPC700 — no DSP register is poked from Rust, so the audible output
  is proof the uploaded driver ran end to end. (This is the SPC700 slice; a full
  65C816 SGB *system* driver that interprets the SGB SOUND effect codes /
  DATA_SND / JUMP needs the 65C816 wired into a live SGB machine — see the
  integration path above.)
- **SOUND ($08)** — decoded to the four SNES↔APU comm ports (effect A/B,
  attenuation, bank). See "unverified" below.
- **DATA_SND ($0F)** — targets SNES *work RAM*, not the APU; drained/ignored (no
  audio effect without a 65816).
- **JUMP ($12)** — the SNES program-jump target is recorded (not executed — no
  65816).

## BIOS gating — what does and doesn't make sound

The SGB's **default sound driver + sample bank live in the SGB cartridge's SNES
ROM**, which slopgb does not ship, and slopgb does **not** emulate the SNES's
65816 CPU. Consequences, stated honestly:

| Scenario | Result |
|---|---|
| Game uploads its own driver via **SOU_TRN** (e.g. Space Invaders) | **Real audio, no BIOS needed** — the uploaded SPC700 program runs on the emulated SPC700 and the S-DSP synthesizes it. |
| Game uses only **SOUND ($08)** / **SOU_TRN** song data, no BIOS | **Silent for real music** — a game that ships only song data (Animaniacs et al.) relies on the SGB's *resident* sound driver, which the clean-room firmware does not implement. |
| Game uses song data, **BIOS supplied via `--sgb-bios`** (coprocessor path) | **Real audio** — the resident N-SPC driver is extracted from the supplied SGB ROM and uploaded to the SPC700 (see below). |
| `Dmg`/`Cgb` | Subsystem absent; output byte-identical. |

`GameBoy::load_sgb_bios(&[u8])` mirrors the opt-in boot-ROM **bytes** API — an
embedder supplies the SGB SNES ROM image. On the built-in HLE path this is still
inert (no 65816 to run the SGB system program). On the **coprocessor path** it is
live — see below.

## Resident N-SPC driver from `--sgb-bios` (coprocessor extract+upload path)

Games like **Animaniacs** ship only *song data* and rely on the SGB's resident
sound driver — the **N-SPC engine** (sneslab.net/wiki/N-SPC_Engine) plus its
sample "soundfont", both living in the SGB system ROM. With that ROM supplied via
`--sgb-bios` on an SGB machine with the coprocessor plugins loaded,
`SgbCoprocessor::install_sgb_bios` (`slopgb-sgb-coprocessor`) makes it play. This
is a **local convenience path** — the copyrighted ROM is the user's, nothing is
committed; the clean-room engine is the upstreamable version (parked below).

Everything reaches the SPC700 through the plugin ABI (`write_ram`/`set_pc`/
`port_read`/`port_write`) — the plugin boundary stays clean; ROM parsing is
host-side. The pieces, all reverse-engineered from a real SGB1 `program.rom`:

- **Driver upload.** The SGB stores its resident SPC700 program as a standard SNES
  APU block table (`[u16 len, u16 dest, len bytes]*` then `[0000, entry]`) at
  LoROM `$06:8000`. `install_sgb_bios` parses it (`parse_apu_blocks`, validated —
  a wrong ROM/offset falls back to clean-room), uploads the 5 blocks (engine
  `$0400`, routines `$4C10`/`$4C30`, sample dir `$4B00`, ~40 KB BRR soundfont
  `$4DB0`) into APU RAM, and execs `$0400`.
- **Song upload = SOU_TRN ($09).** The game renders SBN into VRAM tiles; the SGB
  screen-capture delivers the 4 KB to `upload_transfer`, which writes the SBN
  blocks into APU RAM (song base `$2B00`) **without** re-execing the running
  engine. SBN header order is **`[len, dest]`** (SBN2SPC; the ROM's loader at
  `$00:AC6C`) — not `[dest, len]`.
- **Play trigger = SOUND ($08).** Byte 4 (Music Score Code) selects the song. The
  SGB BIOS forms a command word (command = `score | effect_a<<8`, data =
  `effect_b | attr<<8`; `program.rom $00:C554`) and delivers it to the engine over
  comm ports `$2140-$2143` with a 16-bit **echo-ack handshake** (BIOS `$00:BAC4`):
  send the word, wait for the engine to echo it back, retry otherwise.
  `nspc_flush` ports this handshake to the host, driven at the throttled poll
  cadence; `nspc_cmd`/`nspc_shadow`/`nspc_pending` hold its state (serialized).
- **Mix balance.** The SNES DSP mixes at unity (`MIX_SCALE = 1.0/32768`, matching
  the GB APU's full-scale headroom); the GB feed is attenuated (`GB_GAIN = 0.6`)
  in `mix_into` **only while `nspc_resident`**, mirroring the SGB routing GB below
  the SNES level. Off that path GB audio is untouched.

The `coprocessor` MCP tool's status line reports the resident-driver state and
SOUND/SOU_TRN/DATA_SND counts + DSP peak for diagnosing this path.

## Save states

The SGB APU (SPC700 RAM + registers + timers, the full S-DSP register file +
per-voice/echo/envelope state, and the clock accumulators) is appended to the
save state on SGB models only (format **v4**) — `Dmg`/`Cgb` states stay
byte-identical to v3. The transient output queue and the BIOS image are not
serialized (the output rate is re-derived from the live host). `GameBoy` is
cloned for the atomic restore, so the `SgbApu` implements `Clone` (deep-copying
the shared DSP and re-attaching the SPC700 link).

## What's tested

- **BRR** (`brr_tests.rs`): silence, shift-only decode, sign extension, the
  filter-1 predictor slope vs the float coefficient, end/loop parse, predictor
  threading.
- **Envelope** (`envelope_tests.rs`): rate-counter firing, attack ramp +
  attack→decay, slow-rate stepping, release floor, all GAIN modes, ENVX.
- **Gaussian** (`gaussian_tests.rs`): table endpoints, the **unity-gain
  property at every index**, constant-input passthrough, even output.
- **Echo** (`echo_tests.rs`): silence, FIR/EVOL passthrough, ECEN write-disable,
  ring wrap at the EDL length.
- **Voice** (`voice_tests.rs`): key-on playback, envelope gating, startup delay,
  pitch-zero hold, end-without-loop → ENDX + mute, noise override.
- **DSP** (`dsp_tests.rs`): register R/W + mirror, live ENVX/OUTX/ENDX, key-on →
  audio end-to-end, FLG mute, save-state round-trip.
- **SgbApu** (`apu_tests.rs`): model gating, emission rate, SOU_TRN uploader,
  SOUND→ports, mixing, save-state round-trip, independent clone.
- **Integration** (`lib_tests.rs`): SGB save-state round-trip through `GameBoy`;
  mooneye 91/91 unchanged (the SGB clocking does not perturb GB timing).

## What's unverified / parked

- **No hardware capture in this environment.** The BRR/Gaussian/envelope/echo
  math is a verbatim port of Blargg's reference and is validated by
  self-consistency + the unity-gain and coefficient-slope properties, **not**
  against real DSP output vectors. If a canned bsnes/higan trace becomes
  available, add it as a golden vector.
- **SOUND ($08) → comm-port encoding is a best-effort guess.** The standard SGB
  driver's exact effect-code→port mapping lives inside the SGB system ROM and is
  not publicly documented; `apply_sound` writes the decoded fields to ports 0-3
  and would only be meaningful against a resident driver that expects that
  layout. Do not treat it as canonical.
- **SOU_TRN entry point** is best-effort (first load address / Program Area
  `0x0400`); fullsnes documents the load regions but not a fixed public entry.
- **BIOS default-bank playback** needs either a 65816 core or a verified offset
  of the standard SPC700 driver+samples within a real SGB BIOS to wire up — see
  the gating table. Not attempted rather than fabricated.
- **KON is edge-on-write** (documented "write 0 then 1 to restart") with a
  1-sample latch, not the full multi-sample DSP pipeline; the per-voice 32-step
  pipeline phase and the exact ~5-sample decode startup are approximated.
- **Bent GAIN** uses `env` for the `0x600` test rather than a separate hidden
  envelope shadow (a minor curve difference).
