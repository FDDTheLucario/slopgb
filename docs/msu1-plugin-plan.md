# MSU-1 (+ resident-handler streaming) plugin

**Status: implemented + shipped the real-hardware way (SGB bridge, SNES
`$2000-$2007`) — this was the plan document; everything below has landed.** The
chip is `crates/slopgb-msu1-plugin` (the register interface +
polled-mailbox mode) on the v4 coprocessor bulk channels (proof:
`slopgb-plugin-host/tests/msu1_roundtrip.rs`; SDK surface in
[`ui-state/plugin-api.md`](ui-state/plugin-api.md#coprocessor-plugins-tier-3)). It
loads as `msu1.wasm` from the **plugins dir** and is driven by the SGB coprocessor
(`slopgb-sgb-coprocessor`) at SNES `$2000-$2007` — the same route real Game Boy
MSU-1 hacks take (Super Game Boy mode). Core wiring + mix live in
[`hardware-state/sgb-audio.md`](hardware-state/sgb-audio.md#msu-1-over-the-sgb-bridge);
the register map + status/control bit layout are in the plugin's own module docs
(`crates/slopgb-msu1-plugin/src/lib.rs`).

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

## Mapping decision — resolved

**What shipped: the SGB-bridge LLE.** Real Game Boy MSU-1 hacks (e.g. the Pokémon
Red MSU-1 hack) reach the chip only through **Super Game Boy mode** — the game's
SGB driver uploads a resident **65C816 handler** into SNES WRAM via `DATA_SND`
packets, `JUMP`s to it, and that handler runs each SNES NMI, reads a mailbox the
game fills with more `DATA_SND` packets, and drives the MSU-1 registers at SNES
**`$2000-$2007`**. slopgb already ran that handler (`slopgb-sgb-coprocessor`:
`apply_data_snd` lands packets in SNES WRAM, JUMP runs the 65C816); the missing
piece was MSU-1 at `$2000-$2007`, now wired:

- The **w65c816 plugin** (`mmio.rs`) captures writes to `$2000-$2007` into its MMIO
  ring (drained by the host) and serves reads from a host-fed 8-byte shadow
  (`$2000` = MSU_STATUS, `$2002-$2007` = `S-MSU1` id) via the `HW_MSU` host-window
  (`lib.rs`).
- The **coprocessor** loads `msu1.wasm` from the plugins dir (`attach_msu`), points
  it at a `.pcm` pack (`set_msu_pack`), routes `$2000-$2007` writes to the plugin
  in `apply_mmio`, and each flush (`pump_msu`) advances the chip, refreshes the
  `$2000` read shadow (status + `S-MSU1`), and mixes its 44.1 kHz PCM into the SGB
  output. Presence (`S-MSU1`) is advertised only when ≥1 `.pcm` track loads.
- **Frontend.** `--msu1 <DIR>` / `SLOPGB_MSU1` select the `.pcm` **pack directory**
  and nothing else; absent, the pack defaults to the loaded ROM's own directory
  (the manifest's `$rom_dir` flag default). Threaded via
  `Session::set_plugin_flags` / `apply_sgb_coprocessor` (the value is read back
  with `Session::plugin_flag("msu1")`).
  Requires an SGB model + the coprocessor plugins. There is **no** frontend cart-bus
  bridge: the frontend carries no MSU-1 code of its own, only the pack-dir flag.
- **Mix.** MSU-1 mixes at `2.0/32768` and the GB channels duck (`GB_GAIN`) while a
  track plays, so the music sits above the GB SFX (the game mutes its own GB music
  on SGB).

**Golden-safe.** MSU-1 lives entirely on the SGB side (`Model::Sgb`/`Sgb2` only) as
a wasm plugin attached to the SGB coprocessor (`attach_msu`), which the frontend
injects via `set_audio_coprocessor` (a no-op off SGB); off SGB there is no slot and
the core path is byte-identical. See the swap-seam section in
[`hardware-state/sgb-audio.md`](hardware-state/sgb-audio.md).

## Still deferred (honest coverage)

- **Data-read port `$2001` not live-served over the SGB bridge.** The host
  refreshes the CPU's read shadow only for `$2000` (MSU_STATUS) and `$2002-$2007`
  (the `S-MSU1` id) each flush — those are pure/pre-shadowable. `$2001` (the
  auto-incrementing data-ROM read port) can't be pre-shadowed, so the `.msu` data
  file is **optional and SGB is audio-only** — fine for the SGB use case (SGB games
  use only audio). The plugin's `port_read(1)` data path itself is proven
  (`msu1_roundtrip.rs`, `data_port_walks_the_msu_rom_by_seek`).
- **Muted / fast-forwarded audio is dropped, not paused.** MSU-1 pumps where the SGB
  coprocessor pumps — `AudioCoprocessor::clock` → `flush` → `pump_msu`, batched per
  emulated span — so the chip advances with *emulated* time, never with the audio
  device. A muted or device-less run therefore streams the track normally and throws
  the mixed samples away (`discard_audio` / the device ring), and turbo runs it as
  fast as emulation goes. Only pausing (no cycles) actually stops it.

## Two usage modes (both ride the coprocessor tier)

1. **MSU-1 register interface.** Memory-mapped registers (control / track no. /
   seek / status) → `port_write`/`port_read`; streams a user-supplied `.pcm`
   audio track and reads a `.msu` data ROM by offset.
2. **Resident handler + polled mailbox** (the general homebrew pattern): the game
   uploads code to the coprocessor (`SOU_TRN` / `DATA_SND`+`JUMP`); that code is
   attached to the **per-frame handler** (runs every `run_until` pump); it polls a
   shared memory region each frame; the game writes a play-request into that
   region (via `DATA_SND` / comm packets) when it wants a song. The plugin supports
   this directly: `recv_mailbox` is polled at the top of every `run_until`, so
   resident uploaded code + a game-writable mailbox + per-frame execution need no
   MSU-1 register write at all. MSU-1's fixed registers are a special case of this.

## ABI extensions — landed (shared with the SGB SPC700 work, built once)

- **PCM-drain path** in the tier-3 `Coprocessor` ABI (`drain_pcm`): streams samples
  out + mixes them into the Game Boy output. The SGB integration rides the same path.
- **Bulk data channel** (guest-scratch pattern, like the tool ABI): the host↔guest
  window so (a) the game can write a larger-than-a-few-bytes mailbox / upload data
  into the coprocessor's guest RAM at an offset (`DATA_SND`), and (b) the
  coprocessor can read chunks of a large host-owned file (`.pcm`/`.msu`) by
  offset — scalar comm ports can't carry megabytes. Shipped as the `host_recv` +
  `host_file` imports.
- **Per-frame handler hook**: `run_until` was already pumped each frame; resident
  code runs on that pump, so no extra hook was needed.

## Copyright

MSU-1 is an open homebrew spec (near/byuu). The audio + data packs are
**user-supplied files**; uploaded game code is the game's own. Nothing to
reproduce or clean-room here.

## Placement — resolved

MSU-1 is natively a SNES `$002000` register mapping. slopgb ships it exactly there:
the **SNES-side SGB coprocessor** drives the chip at `$2000-$2007`, matching real
Game Boy MSU-1 hardware (the SGB bridge). There is no Game-Boy-cart-mapped
(`$A000-$A007`) placement and no frontend cart-bus poll: no real GB MSU-1 hack
addresses the cart window, they all go through the SGB bridge.

## Depends on — satisfied

Built on the SGB tier-3 PCM-drain path + the resident-handler chain (both landed —
`slopgb-sgb-coprocessor`). MSU-1 rides the same seam as the SGB N-SPC driver.

## References

- MSU-1 notes (register map, seek/pause/loop/volume semantics, `.pcm`/`.msu`
  file format): <https://zumi.neocities.org/stuff/msu1_notes/>
- MSU-1 docs collection: <https://github.com/Sunlitspace542/MSU-1-Docs>

Both are open MSU-1 documentation — the spec + register behavior the plugin
implements. The audio/data packs themselves stay user-supplied.
