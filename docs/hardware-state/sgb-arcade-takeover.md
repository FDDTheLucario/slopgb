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

With the contract in place the chain runs: bootstrap loops the service, the
hook dispatches on the DATA_TRN, the ACK write reaches the pad latch (the
GB-side `joypad_feed` path works — `D751` moves), the dispatcher copies the
staged program, and control enters the arcade program at `$7F:01xx`. The
program then **dies in BRK chaos**: it needs vblank NMI, the `$21xx`/`$42xx`
MMIO surface, and a PPU — none exist yet. In native mode the BRK vector
(`$00:FFE6`) reads zeroed program area, so the crash sprays bank 0 (the
`04 35 00 35` stack-byte pattern over the pad latches is its fingerprint).
Unblocking it is the MMIO-capture / NMI / clocking-loop / PPU work
(`goal.md` phases 2-4). One loose end to re-check then: a byte-0 patch of
the `$7F:0100` payload (`$4C` ↔ `$18`) seen around the transfer — likely the
program's armed/disarmed entry state, judge again once it survives.

## Provenance / clean-room note

The `$0800`/`$0A00`/`$1800` listings quoted here are the *game's* code,
observed through slopgb's own packet tee at runtime (the game uploads them
in cleartext DATA_SND packets). No SGB BIOS bytes were read; the BIOS-side
addresses above are treated as an ABI surface the game defines, and the
routines slopgb installs behind them are original.
