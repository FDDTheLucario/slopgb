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

## The byte-exchange model — **byte-level lockstep** (shipped)

Byte-level lockstep, not bit-level. The earlier 1-byte-latency model corrupted Pokémon trades: the
frontend pump runs once per emulated *frame*, but a master clocks many serial transfers *within* a
frame, each reading an empty `link_in` → shifting in `0xFF` → uniform garbage. The fix makes a
connected master **stall** at completion until the peer's byte is in hand, and lets `run_frame` yield
mid-frame so the frontend can pump.

- **Master** (SC=`0x81`, internal clock): clocks its 8 bits, ships its outgoing byte to `link_out`
  (once), then — if no peer byte is buffered — **stalls** (`link_master_waiting`): SC bit7 stays set,
  IF withheld, further DIV clocking gated. `GameBoy::link_stalled()` is true; `run_frame` /
  `run_frame_until_breakpoint` return early. `link_push_recv(byte)` completes the stall (SB←byte,
  clear bit7, raise serial IF). A SC rewrite clears the stall; disconnecting mid-stall completes with
  the cable-open `0xFF` + IF so the CPU can't hang. All gated on `link_connected` ⇒ golden-safe.
- **Slave** (SC bit7 set, bit0 clear, external clock — never completes alone): `link_slave_transfer(
  master_byte)` swaps SB↔master_byte, clears SC bit7, raises serial IF (bit3), returns the slave's
  outgoing byte.
- **Frontend lockstep loop** (`link.rs` + `app_pacing.rs`): `pump` ships master bytes as SYNC1, routes
  incoming SYNC1→armed-slave (reply SYNC2) **or** a new frontend `pending_master` buffer (never the
  core `link_in` — no cross-contamination), and SYNC2→`push_recv`. `drain_pending` dispatches buffered
  bytes once the local port is ready (slave arms / master stalls — both-master fed in). `run_one_frame`
  loops run→pump and, when stalled, `pump_blocking` waits ≤16 ms for the reply then resumes, so a whole
  frame's serial traffic resolves in one tick; a dead peer times out and yields the partial frame
  (never completed with garbage).
- **Latency**: a localhost round-trip (sub-ms; socket read poll 2 ms) per stalled transfer. Verified
  zero-corruption by an 8-byte real-socket exchange + a 16-byte no-socket loopback. Timestamp-precise
  bgb-wire lockstep (SYNC3 idle keep-alive + cycle-accurate completion) is still the documented next
  step.

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
