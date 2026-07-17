# SGB arcade takeover — the BIOS-runtime contract (black-box pinned)

How a full-takeover SGB title (pilot: Space Invaders USA, its ARCADE mode)
drives the SNES side, reverse-engineered **entirely from the game's own
packet stream** (the raw-packet tee logs every 16-byte packet) — never from
the SGB BIOS image or any emulator source. Every claim below is pinned by
the pilot's uploaded code, quoted from the observed DATA_SND payloads.

## The observed upload sequence

1. `MLT_REQ on` / `MLT_REQ off` (SGB detection, HLE handles it).
2. A DATA_SND series installs a **hook routine** at `$00:0800-$085D`
   (assembled tail-first, 11 bytes per packet) and a **dispatcher** at
   `$00:0A00-$0A53`; the last write plants `JMP $0A00` at the `$0800` slot.
3. A DATA_SND pair installs a 22-byte **bootstrap** at `$00:1800`:
   `STZ $1700 / STZ $4200 / loop: LDA $00FFDB / BEQ +5 / JSR $BBED /
   BRA loop / JSR $BBF0 / BRA loop`.
4. `JUMP $001800` — the bootstrap becomes the SNES mainloop.
5. `DATA_TRN → $7F:0100` — the 4 KB arcade program proper (its head is a
   `JMP` vector table), delivered as a screen capture.

## The contract the uploaded code assumes (and slopgb now provides)

| Piece | Address | Pinned by |
|---|---|---|
| Packet buffer | `$7E:0600-$060F` | dispatcher reads the DATA_TRN dest from `$0601-$0603` (`LDA $0601 / STA $B0` …) |
| Last command number | `$7E:02C2` | dispatcher `LDA $02C2 / CMP #$10 / BNE` |
| DATA_TRN staging pointer | `$7E:0284/85` (bank `$7E` implied) | dispatcher copy loop `LDA $0284… / STA $98 / LDA #$7E / STA $9A / … LDA [$98],Y / STA [$B0],Y` — it copies the staged payload to the packet's dest itself |
| Main service entries | `$BBED` / `$BBF0` (per-revision pair, chosen on `$00:FFDB`; slopgb keeps `$FFDB = 0` → `$BBED`) | bootstrap `JSR` loop |
| Aux service entries | `$C58D` / `$C590` | dispatcher `JSR` on the DATA_TRN path |
| Hook slot | `$00:0800`, called by the main service | the dispatcher's `PLA PLA / RTS` stack fixup requires exactly two JSR levels (mainloop → service → hook) |
| Native-mode handover | JUMP targets run in native mode | dispatcher `REP #$30` + 16-bit `LDX #$0800` copy loop — impossible in emulation mode |
| The ACK | on `$02C2 == $10` the dispatcher writes `$01` then `$00` to `$6004` (the ICD2 pad latch) — the signal the GB's `$32F4` loop waits for | dispatcher `LDA #$01 / STA $006004 … LDA #$00 / STA $006004` |

slopgb's resident firmware (all original, opcodes from the WDC datasheet):
`JMP` thunks at the four entries (they sit 3 bytes apart — no room for
bodies), a guarded hook-caller body at `$BE00` (`LDA $0800 / BEQ +3 /
JSR $0800 / RTS`), an RTS aux body at `$BE20`, and a `CLC / XCE / JML`
JUMP trampoline at `$BF00`. The host pump maintains the WRAM variables on
every teed packet and stages DATA_TRN payloads at `$7E:D000` behind the
`$0284` pointer (any address works — the game only follows the pointer),
while still copying to the packet's dest directly for programs that expect
the BIOS to have done it.

## Where the pilot stands (2026-07-17)

With the contract in place the chain runs end to end: the ACK handshake
completes (the GB leaves the `$32F4` spin, `D751` = `$FF`), and the pilot
streams one DATA_TRN per GB frame. Everything below is pinned from live
probes of the pilot's own uploaded code (PC-distribution sampling +
disassembly of the teed bytes):

- **Bootstrap (`$1800`, the first JUMP's target)**: `STZ $1700 /
  STZ $4200 / LDA long $FFDB / BEQ+5 / JSR $BBED / BRA` — revision 0
  selects the **second** entry (`$BBF0`/`$C590`) of each service pair, and
  the loop re-disables NMIs every iteration.
- **Hook (`$0800` → `$0A00`)**: on `$02C2 == $10` it writes pad latch
  `$01`, loads `[$B0]` from the packet's dest bytes (`$0601-$0603`), calls
  the **aux service** (`$C590`), writes pad latch `$00`, then copies the
  4 KB staging to `[$B0]`. The aux service is therefore a **wait-for-
  vblank** (the wait holds the `$01` latch across a vblank so the GB
  observes both handshake values); an NMI-enabling variant was probed and
  refuted — see `BIOS_AUX_BODY` in the coprocessor.
- **JUMP carries the NMI vector**: the pilot's first JUMP is
  `PC=$001800 / NMI=$001800`; once streaming is up it sends a second JUMP
  `PC=$7F:0103 / NMI=$7F:0100` (Pan Docs 12h bytes 4-6 → the `$00BB-BD`
  RAM vector).
- **The arcade program head (`$7F:0100`)**: `JMP $0106` (an RTI — the
  disarmed NMI entry; the byte-0 arm patch flips the flow), `JMP $0107`
  (the init entry the second JUMP targets). The init clears DP `$1000+`,
  writes `$FE` to `$2140`, and uploads two sound-data blocks to the SPC700
  through the **standard IPL boot-ROM protocol** (`CMP $2140` for `$BBAA`,
  then the kick/index/ack pump) — which is why the coprocessor boots the
  chip's IPL ROM instead of parking it in the resident square driver.

**Current wall (next session's entry point)**: the delivery pipeline is
fixed (FIFO payload pairing, ping-pong staging, and the guest-side
delivery mailbox the resident main-service body consumes — publishes now
serialize with the hook exactly like the single-threaded real BIOS). The
60 KB program upload survives intact, the second JUMP lands, and the
arcade program **executes**, driving its sound-driver upload through the
SPC700 IPL ROM — until ~byte 244 of block 1, where the flush-batched APU
port mediation deadlocks the per-byte handshake (SNES waiting for echo
`$F4`, the IPL's Y at ~`$60`): repeating mod-256 index values across
sticky per-flush port snapshots alias, and the two sides lose lockstep.
Fix direction: ordered port-write replay — capture the 65C816's
`$2140-$2143` writes in sequence (a ring like the MMIO one) and replay
them to the SPC one at a time, pacing on its echoes, instead of
delivering only each flush's final latch values.

## Provenance / clean-room note

The `$0800`/`$0A00`/`$1800` listings quoted here are the *game's* code,
observed through slopgb's own packet tee at runtime (the game uploads them
in cleartext DATA_SND packets). No SGB BIOS bytes were read; the BIOS-side
addresses above are treated as an ABI surface the game defines, and the
routines slopgb installs behind them are original.
