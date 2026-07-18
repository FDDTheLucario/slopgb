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
  writes `$FE` to `$2140`, and drives the SPC700 through the **standard
  IPL boot-ROM protocol** — which is why the coprocessor boots the
  chip's IPL ROM instead of parking it in the resident square driver.

## The init's full sequence (all pinned from the uploaded bytes)

The loader at `$7F:0171` has two entries: `$016A` (skip-handshake —
continue an open IPL session with the current kick byte at DP `$0C`) and
`$0171` (wait for the 16-bit `$BBAA` announce on `$2140`, then kick
`$CC`). Chain headers are `[len16, dest16]`: `dest == 0` returns (chain
end, nothing sent); `len == 0, dest != 0` sends the **IPL entry command**
(port 1 = 0 + kick — the SPC jumps to `dest`). The init:

1. `JSL $0171` with chain pointer `$7F:01DA` — chain 1: the APU driver
   (`$17BC → $0800`), an echo table (`$64 → $2E00`), a five-entry sample
   directory (`$14 → $2F00`), the BRR bank (`$5A5A → $2F6D`, ending
   byte-exactly at the directory's samples), then **entry `$FFC0`** — the
   IPL restarts and re-announces.
2. `JSL $0171` with pointer `$7F:747C` (= the byte after chain 1's entry
   header) — chain 2: the music/data chains, ending with the entry into
   the uploaded driver at `$0800` (which unmaps the IPL via `CONTROL`).
3. Pad latch `$6004 = $3F` — **Select+Start on the wire**: the GB's poll
   (`$106C`) derives `D751 = $0C`, the phase machine's first content
   request. One shot; the hook's ACK sandwich overwrites it within
   microseconds (why the latch needs ordered delivery, below).
4. `JML $001800` — back to the bootstrap service loop.

## The GB-side phase machine (ROM `$31E0-$3248`, bank 3 templates)

After the second JUMP the GB loops `CALL $106C / LD A,(D767) / CP (D751)`
until the pad byte changes, then dispatches: `$0C` → stream the content
table at `$3343` + `JUMP $7F:2000` (NMI `$7F:2003`); `$0E` → table
`$334A` + `JUMP $7F:2006`; `$0D` → table `$334E`. `D751` is the plain
CPL'd joypad byte (dpad high nibble, buttons low) — `$0C` = Select+Start
— so the **content phases are driven by pad-latch values**: the init's
one-shot `$3F` starts phase 2, and afterwards the *player's* Select+Start
(reaching the GB through the resident BIOS's continuous pad forward)
drives the later dispatches. The 15-block initial stream is table
`$333E`'s count, by design.

## Resolved walls (each was a host-model defect, never the pilot's)

- **`*_TRN` capture clock**: captures fire one GB-frame after the command
  on a machine-clocked window (core `SgbView::trn_countdown`) — the GB's
  line-144 boundary loses latches across LCD-off stretches (blocks 7/10
  duplicated → the chain-1 entry header vanished), and command-time
  capture is too early (the pilot sends `$10` while still drawing;
  block 1 landed with its header bytes blank).
- **APU port mediation**: the 65C816's `$2140-43` writes ring in order
  (cap 16384) and replay per-event with minimum consume/produce slices,
  in rounds inside each flush — an echo-paced upload advances one byte
  per round, so single-round mediation moved one byte per flush and
  aliased the mod-256 index handshake.
- **Pad-latch delivery**: `$6004-$6007` writes ring in order too
  (HW_PAD_RING); the coprocessor feeds one snapshot per dwell to the GB
  and passes the local matrix through when the queue idles — the
  resident BIOS's per-frame pad forward, and the only path for player
  input into a taken-over GB.

With all three in place the pilot runs end to end: both IPL chains
upload, the driver takes the APU (live port-3 dispatch traffic), the
init's `$3F` fires phase 2, the GB streams the stage tables and sends
`JUMP $7F:2000`, the arcade game's own main loop runs (`$7F:207E` +
subroutines, own stack at `$1FFx`), and a real Select+Start press
reaches `D751` and advances the phase (`JUMP $7F:2006` observed).

## Provenance / clean-room note

The `$0800`/`$0A00`/`$1800` listings quoted here are the *game's* code,
observed through slopgb's own packet tee at runtime (the game uploads them
in cleartext DATA_SND packets). No SGB BIOS bytes were read; the BIOS-side
addresses above are treated as an ABI surface the game defines, and the
routines slopgb installs behind them are original.
