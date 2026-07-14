# MSU-1 (+ resident-handler streaming) plugin — plan

**Status: implemented + wired into a running machine.** The chip is
`crates/slopgb-msu1-plugin` (the register interface + polled-mailbox mode) on the
v4 coprocessor bulk channels (proof: `slopgb-plugin-host/tests/msu1_roundtrip.rs`;
SDK surface in [`ui-state/plugin-api.md`](ui-state/plugin-api.md#coprocessor-plugins-tier-3)).
The frontend now hosts it as a playable peripheral (`crates/slopgb/src/msu1.rs`,
`--msu1 <dir>`): the register mapping + live play + PCM mixing landed — see
**Mapping decision** below. The register-map / bit-layout spec further down is the
design reference.

An MSU-1-style streaming-audio coprocessor as a slopgb tier-3 plugin, plus the
more general "resident frame-handler + polled mailbox" custom-music pattern it
generalizes.

## What landed (and how it maps to the plan)

- **Register interface.** Comm ports `0..=7` map to `$2000-$2007`: status/data +
  32-bit seek, 16-bit track select, volume, control (play/repeat/resume), and the
  `"S-MSU1"` id bytes. The `.pcm` header (`"MSU1"` + 32-bit LE loop point) and the
  interleaved 16-bit LE stereo samples stream via the bulk-file channel.
- **Resident handler + polled mailbox.** Every `run_until` the plugin polls the
  host mailbox; a game-written `[cmd, track_lo, track_hi, flags]` play-request
  starts playback with no register writes.
- **ABI v4 bulk channels** (shared, general): `host_recv` (mailbox) + `host_file`
  (keyed host-owned file by offset), both reusing the guest-scratch pattern (no
  `unsafe`, no `from_raw_parts`). The per-frame handler hook is the already-pumped
  `run_until`.

## Mapping decision (the "one integration decision")

**Where the 8 registers live:** the Game Boy cartridge I/O window, base
**`$A000-$A007`** (register `n` ↔ plugin comm port `n`, mirroring the SNES
`$2000-$2007`). Rationale: slopgb is a Game Boy, so the natural route is a
GB-cartridge-mapped MSU-1, not the SNES memory map. `$A000-$BFFF` is the external-
cartridge address space where MBC RAM / RTC registers already sit — and real MSU-1
hardware carries SRAM there — so a homebrew MSU-1 cart maps its control registers
into that same window. (`$2000-$2007` is the SNES convention being adapted, not a
GB address.)

**What landed (`crates/slopgb/src/msu1.rs`, opt-in `--msu1 <dir>` / `SLOPGB_MSU1`):**

- **Live play** (golden-safe). Each rendered frame the frontend polls
  `$A004-$A007` with the read-only `&self` `GameBoy::debug_read` and edge-forwards
  the audio-control registers (track select / volume / control) to the chip's
  comm ports; a change on either track-select byte re-commits `select_track`
  (so a low-byte-only change with a stable high byte still re-selects), and the
  control register is edge-triggered (write-once-and-leave does not restart). It
  then advances the plugin one frame of 44.1 kHz samples, drains its PCM, and
  resamples 44.1 kHz → the core output rate.
- **PCM mixing.** `AudioPipe::pump_mixing` adds the resampled track into the Game
  Boy stream sample-for-sample before the device resample + gain (mirroring the
  built-in SGB `mix_into`). An empty extra is byte-identical to the plain pump.
- **Pack loading.** `--msu1 <dir>` reads the coprocessor plugin (`dir/msu1.wasm`),
  every `*.pcm` (keyed by its trailing track number → the plugin host-file key),
  and an optional `*.msu` data ROM. A missing/broken plugin is a non-fatal logged
  error (the game still runs, MSU-1 silent).

**Golden-safe** (the hard gate). There is **no core memory hook at all**. A wasm
store can't be cloned into the machine's save-state, so MSU-1 lives entirely in
the frontend (outside `GameBoy`), like the audio pipe; with no `--msu1` pack the
`msu1: Option<Msu1>` is `None`, nothing is polled or mixed, and the core + audio
path are byte-identical (proven: `golden_fingerprint` + mooneye 93/93 unchanged;
no core file was touched). Test: `crates/slopgb/src/msu1_tests.rs` authors a
fixture pack, writes the registers into cart RAM, and asserts a pumped frame
produces non-silent mixed audio; with no pack the path is inert.

## Still deferred (honest coverage)

- **Per-access live register reads.** A running game reads `$A000` (status) /
  `$A001` (data-ROM byte) / the id ports from its own CPU mid-frame; the frontend
  can't observe those golden-safe without a per-access core intercept. The live
  poll therefore drives the *write* side (which is what starts a track); the
  data-ROM seek/read feature (`$A000-$A003`) awaits that intercept. The
  register↔port read map itself is proven (`Msu1::read_reg`, test-only today).
- **Muted / timer-paced audio.** MSU-1 pumps only where the audio pipe pumps
  (audio-paced, un-muted), so a muted or device-less session leaves the track
  paused — same gating as the built-in APU.

## Two usage modes (both ride the coprocessor tier)

1. **MSU-1 register interface.** Memory-mapped registers (control / track no. /
   seek / status) → `port_write`/`port_read`; streams a user-supplied `.pcm`
   audio track and reads a `.msu` data ROM by offset.
2. **Resident handler + polled mailbox** (the general homebrew pattern): the game
   uploads code to the coprocessor (`SOU_TRN` / `DATA_SND`+`JUMP`); that code is
   attached to the **per-frame handler** (runs every `run_until` pump); it polls a
   shared memory region each frame; the game writes a play-request into that
   region (via `DATA_SND` / comm packets) when it wants a song. The plugin must
   support this directly: resident uploaded code + a game-writable mailbox region
   + per-frame execution. MSU-1's fixed registers are a special case of this.

## ABI extensions needed (shared with the SGB SPC700 work — build once)

- **PCM-drain path** in the tier-3 `Coprocessor` ABI: stream samples out + mix
  into the Game Boy output. The SGB integration needs the same path.
- **Bulk data channel** (guest-scratch pattern, like the tool ABI): a host↔guest
  window so (a) the game can write a larger-than-a-few-bytes mailbox / upload data
  into the coprocessor's guest RAM at an offset (`DATA_SND`), and (b) the
  coprocessor can read chunks of a large host-owned file (`.pcm`/`.msu`) by
  offset — scalar comm ports can't carry megabytes.
- **Per-frame handler hook**: `run_until` is already pumped each frame; ensure a
  plugin can register resident code that runs every pump (the "attach to the
  frame handler" step).

## Copyright

MSU-1 is an open homebrew spec (near/byuu). The audio + data packs are
**user-supplied files**; uploaded game code is the game's own. Nothing to
reproduce or clean-room here.

## Placement

MSU-1 is natively a SNES `$002000` register mapping. slopgb took the
Game-Boy-cart-mapped route (registers at `$A000-$A007`; see **Mapping decision**
above) rather than a SNES-side SGB coprocessor — slopgb emulates a Game Boy, and
the cart window is where GB homebrew would map the chip. The coprocessor *plugin*
is the host either way.

## Depends on

The SGB tier-3 PCM-drain path (in progress). Queue the build after that lands — it
unblocks MSU-1 and the SGB driver together.

## References (read before building)

- MSU-1 notes (register map, seek/pause/loop/volume semantics, `.pcm`/`.msu`
  file format): <https://zumi.neocities.org/stuff/msu1_notes/>
- MSU-1 docs collection: <https://github.com/Sunlitspace542/MSU-1-Docs>

Both are open MSU-1 documentation — the spec + register behavior to implement
against. The audio/data packs themselves stay user-supplied.
