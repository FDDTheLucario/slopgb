# MSU-1 (+ resident-handler streaming) plugin — plan

Forward-looking / queued. An MSU-1-style streaming-audio coprocessor as a slopgb
tier-3 plugin, plus the more general "resident frame-handler + polled mailbox"
custom-music pattern it generalizes.

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

MSU-1 is natively a SNES `$002000` register mapping. In slopgb it lands either as
a SNES-side coprocessor for SGB, or as a Game-Boy-cart-mapped MSU-1 variant for GB
homebrew — the register↔address-space mapping is the one integration decision.
The coprocessor *plugin* is the host in both cases.

## Depends on

The SGB tier-3 PCM-drain path (in progress). Queue the build after that lands — it
unblocks MSU-1 and the SGB driver together.

## References (read before building)

- MSU-1 notes (register map, seek/pause/loop/volume semantics, `.pcm`/`.msu`
  file format): <https://zumi.neocities.org/stuff/msu1_notes/>
- MSU-1 docs collection: <https://github.com/Sunlitspace542/MSU-1-Docs>

Both are open MSU-1 documentation — the spec + register behavior to implement
against. The audio/data packs themselves stay user-supplied.
