# bgb Link cable — TDD plan + golden-safety contract

bgb-1:1 serial Link (main-window **Link** submenu: **Listen / Connect / Disconnect / Cancel listen**,
order + labels from `docs/bgb-reference/menus/main-sub-link.png`), backed by a real `std::net` TCP
transport. No Options Link tab — bgb's Link is menu-only.

## The golden-safety invariant (non-negotiable)

The serial port is the most-validated subsystem (mooneye `serial/*`, gambatte `serial/*`, blargg via
the SB/SC print hook). **Every core change must leave emulation byte-identical when no peer is
connected.** Concretely:

- The link state defaults **off** (`link_in: None`, `link_out: None`, not connected). On every golden
  path (gbtr, mooneye, all unit tests, real play without a peer) the frontend never attaches a peer,
  so these stay `None`.
- The disconnected incoming bit stays the literal `| 1` — the connected branch is taken **only** when
  `link_in.is_some()`, which is unreachable without the frontend.
- `link_in`/`link_out` are **transient, not serialized** (like the debugger watch/prof/exc fields) so
  the save-state format + ROM fingerprint are unchanged.
- Proof: unit test `disconnected_master_transfer_byte_identical` + a full **gbtr golden byte-identical**
  run (stash the unrelated CGB-WRAM working-tree changes first).

## The byte-exchange model (what bgb/SameBoy/gambatte do over TCP)

Not bit-level lockstep — a per-completed-byte swap, which games tolerate via their own handshaking.
Real bit-lockstep matters only for the local no-peer path, which we keep byte-identical.

- **Master** (SC=`0x81`, internal clock, completes locally): incoming bits come from the injected peer
  byte `link_in` MSB-first instead of `1`s; on completion the outgoing byte is queued to `link_out`
  for the frontend to ship to the peer.
- **Slave** (SC bit7 set, bit0 clear, external clock — never completes alone): a new `&mut` method
  `link_slave_transfer(master_byte)` completes it when the frontend delivers the master's byte —
  swap SB↔master_byte, clear SC bit7, raise serial IF (bit3), return the slave's outgoing byte.
- **1-byte latency**: TCP delivers the peer byte a transfer late; bgb has the same property. Games
  (Tetris, Pokémon trade) handshake around it. We do not attempt cycle-bit lockstep.

## Transport (frontend `crates/slopgb/src/link.rs`, std::net — no Cargo dep)

- `Listen` = bind a `TcpListener` on port **8765**, accept one peer on a background thread.
- `Connect` = dial `host:port` (a host:port `InputDialog` modal like Load ROM; bare host defaults 8765).
- `Disconnect` / `Cancel listen` = tear the socket + thread down.
- Background thread ↔ UI via `std::sync::mpsc` so the socket never blocks the paced loop; the App
  per-frame **pump** shuttles bytes between the core link hook and the channels.

## Menu (frontend `windows/mainwin.rs` + `app_menu.rs`)

`SubKind::Link` + `SubChoice::{Listen,Connect,Disconnect,CancelListen}`, routed through `open_submenu`
/ `handle_subchoice` like the other submenus. Rows grey by connection state — **Disconnect** only when
connected, **Cancel listen** only when listening (matches bgb).

## Task order (see the session's /tdd-test-plan output)

Core mechanism (1 link_in inject → 2 link_out send → 3 slave transfer → 4 IF wiring → 5 not-serialized
→ 6 in-process loopback) **then golden-gate** → GameBoy API (7) ‖ transport (8 framing → 9 TCP socket →
10 pump) → menu (11 items → 12 un-grey row → 13 dispatch → 14 Connect modal) → 15 live self-connect.

Models: core bit-order/golden (1) + slave SC/IF (3) + TCP concurrency (9) = opus; the rest sonnet/haiku.

## Verification

- Golden-safety: `disconnected_*` unit tests + gbtr golden byte-identical (CGB-WRAM stashed).
- Two-instance correctness: **in-process** loopback unit test (two Serials, no socket) — byte both ways.
- Transport: localhost self-connect unit test (two `LinkSocket`s on 127.0.0.1) + the protocol framing pair.
- Live: slopgb Listen + slopgb Connect on this machine; **real** captures of both windows connected.
